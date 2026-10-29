//! Passive init/done(phase) virtual hooks. Never initialize or destroy anything
//! on behalf of the mod. All originals are forwarded exactly once, including
//! foreign-thread calls. The pump pins forwarding code until process exit.
use crate::{
    api::{self, EngineControl},
    engine_lifecycle::{add, executable},
    engine_render::{flush_calls, read},
    inspection::process::Process,
    profile,
    transition_observation::{self as observation, CAPACITY, Record},
};
use std::{
    ffi::c_void,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::{
        Memory::{PAGE_READWRITE, VirtualProtect},
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
};
type Phase = unsafe extern "thiscall" fn(*mut c_void, u32) -> u32;
type Block = unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> bool;
const SLOTS: [u32; 2] = [profile::CAR_VTABLE + 0x5c, profile::CAR_VTABLE + 0x60];
const FUNCTIONS: [u32; 2] = [0x50f40, 0x515d0];
static STATE: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static SLOTS_AT: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];
static ORIGINALS: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];
static PROTECTION: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
static CALLS: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
// argc/argv/current handler pointer slots, never callbacks/getters to execute.
static MAP_BINDINGS: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static MAP_FLIGHT: AtomicU32 = AtomicU32::new(0);
static ARGV_SITE: AtomicU32 = AtomicU32::new(0);
static HOOKS: AtomicU32 = AtomicU32::new(0);
static FIXTURE: AtomicBool = AtomicBool::new(false);
static LAST_CAR: AtomicU32 = AtomicU32::new(0);
static FOREIGN: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static LOST: AtomicBool = AtomicBool::new(false);
static NEXT_DRAW: AtomicBool = AtomicBool::new(false);
static TRACE: Mutex<Trace> = Mutex::new(Trace::new(0));
struct Trace {
    generation: u32,
    len: usize,
    flags: u32,
    rows: [Record; CAPACITY],
}
impl Trace {
    const fn new(generation: u32) -> Self {
        Self {
            generation,
            len: 0,
            flags: 0,
            rows: [Record {
                kind: 0,
                thread: 0,
                caller: 0,
                this: 0,
                argument: 0,
                depth: 0,
                flushes: 0,
                values: [0; 4],
                readable: 0,
                context: [0; 8],
                returned: 0,
                models: [0; 16],
                command: [0; 32],
            }; CAPACITY],
        }
    }
}
fn record(kind: u32, this: u32, argument: u32, caller: u32, depth: u32, returned: u32) {
    if !ACTIVE.load(Ordering::Acquire) || STATE.load(Ordering::Acquire) != 1 {
        return;
    }
    let Ok(mut trace) = TRACE.try_lock() else {
        LOST.store(true, Ordering::Release);
        ACTIVE.store(false, Ordering::Release);
        return;
    };
    if trace.len == CAPACITY {
        trace.flags |= observation::TRUNCATED;
        ACTIVE.store(false, Ordering::Release);
        return;
    }
    let mut row = Record {
        kind,
        this,
        argument,
        caller,
        depth,
        thread: unsafe { GetCurrentThreadId() },
        flushes: flush_calls(),
        returned,
        ..Record::default()
    };
    let block = (6..=9).contains(&kind);
    let car = if block || kind >= 10 {
        LAST_CAR.load(Ordering::Acquire)
    } else {
        this
    };
    row.context[5] = car;
    for (i, offset) in [0x84, 0x1d0, 0x200, 0x300].into_iter().enumerate() {
        let mut bytes = [0; 4];
        let len = if i == 3 { 1 } else { 4 };
        if car != 0
            && let Some(at) = car.checked_add(offset)
            && read(at, &mut bytes[..len]).is_ok()
        {
            row.values[i] = u32::from_le_bytes(bytes);
            row.readable |= 1 << i;
        }
    }
    if row.readable & 8 != 0 {
        let mut bytes = [0; 64];
        let count = (row.values[3] as usize).min(16);
        if count == 0
            || car
                .checked_add(0x200)
                .is_some_and(|at| read(at, &mut bytes[..count * 4]).is_ok())
        {
            row.readable |= 16;
            for (out, word) in row.models.iter_mut().zip(bytes.as_chunks::<4>().0) {
                *out = u32::from_le_bytes(*word);
            }
        }
    }
    if block {
        for (i, offset) in [8, 0x28, 0x4c, 0x50, 0x58].into_iter().enumerate() {
            if argument != 0
                && let Some(at) = argument.checked_add(offset)
            {
                let mut bytes = [0; 4];
                if read(at, &mut bytes).is_ok() {
                    row.context[i] = u32::from_le_bytes(bytes);
                    row.readable |= 1 << (8 + i);
                }
            }
        }
        for (i, offset) in [(6, 0x3ac), (7, 0x3b0)] {
            let mut bytes = [0; 4];
            if let Some(at) = this.checked_add(offset)
                && read(at, &mut bytes).is_ok()
            {
                row.context[i] = u32::from_le_bytes(bytes);
                row.readable |= 1 << (8 + i);
            }
        }
    }
    if matches!(kind, 10..=12) {
        let mut argv = argument;
        if matches!(kind, 11 | 12) {
            for (i, binding) in MAP_BINDINGS.iter().enumerate() {
                let mut bytes = [0; 4];
                if read(binding.load(Ordering::Acquire), &mut bytes).is_ok() {
                    row.context[i + 1] = u32::from_le_bytes(bytes);
                    row.readable |= 1 << (9 + i);
                }
            }
            // An argc bound authorizes argv[0] only, never argv[1..].
            argv = if row.readable & 0x600 == 0x600 && (1..=32).contains(&row.context[1]) {
                row.context[2]
            } else {
                0
            };
        }
        let (name, flags) = observation::command_name(argv, |at, out| {
            read(at, out).map_err(|_| "command byte unreadable".into())
        });
        row.command = name;
        row.readable |= flags;
        if let Some(at) = this.checked_add(0x14c) {
            let mut bytes = [0; 4];
            if read(at, &mut bytes).is_ok() {
                row.context[0] = u32::from_le_bytes(bytes);
                row.readable |= 1 << 8;
            }
        }
    }
    let index = trace.len;
    trace.rows[index] = row;
    trace.len += 1;
}
/// Called only after the original outer draw, on the proven owner thread.
pub fn after_draw(this: u32) {
    if unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire) {
        LAST_CAR.store(this, Ordering::Release);
        if NEXT_DRAW.swap(false, Ordering::AcqRel) {
            record(5, this, 0, 0, DEPTH.load(Ordering::Acquire), 0);
        }
    }
}
#[unsafe(naked)]
unsafe extern "thiscall" fn init_shim(_: *mut c_void, _: u32) -> u32 {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","push dword ptr [ebp+4]","push dword ptr [ebp+8]","push ecx","push 0","call {implementation}","mov esp,ebp","pop ebp","ret 4",implementation=sym phase_impl);
}
#[unsafe(naked)]
unsafe extern "thiscall" fn done_shim(_: *mut c_void, _: u32) -> u32 {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","push dword ptr [ebp+4]","push dword ptr [ebp+8]","push ecx","push 1","call {implementation}","mov esp,ebp","pop ebp","ret 4",implementation=sym phase_impl);
}
unsafe extern "C" fn phase_impl(which: u32, this: *mut c_void, argument: u32, caller: u32) -> u32 {
    let error = unsafe { GetLastError() };
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    let depth = if owner {
        DEPTH.fetch_add(1, Ordering::AcqRel) + 1
    } else {
        0
    };
    if owner {
        CALLS[which as usize].fetch_add(1, Ordering::Relaxed);
        record(1 + which * 2, this as u32, argument, caller, depth, 0);
    } else {
        FOREIGN.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { SetLastError(error) };
    let original: Phase =
        unsafe { std::mem::transmute(ORIGINALS[which as usize].load(Ordering::Acquire)) };
    let result = unsafe { original(this, argument) };
    let error = unsafe { GetLastError() };
    if owner {
        record(2 + which * 2, this as u32, argument, caller, depth, result);
        NEXT_DRAW.store(true, Ordering::Release);
        DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
    unsafe { SetLastError(error) };
    result
}
#[unsafe(naked)]
unsafe extern "thiscall" fn block_init_shim(_: *mut c_void, _: *mut c_void) -> bool {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","push dword ptr [ebp+4]","push dword ptr [ebp+8]","push ecx","push 2","call {implementation}","mov esp,ebp","pop ebp","ret 4",implementation=sym block_impl);
}
#[unsafe(naked)]
unsafe extern "thiscall" fn destroy_request_shim(_: *mut c_void, _: *mut c_void) -> bool {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","push dword ptr [ebp+4]","push dword ptr [ebp+8]","push ecx","push 3","call {implementation}","mov esp,ebp","pop ebp","ret 4",implementation=sym block_impl);
}
unsafe extern "C" fn block_impl(
    which: u32,
    this: *mut c_void,
    item: *mut c_void,
    caller: u32,
) -> bool {
    let error = unsafe { GetLastError() };
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    let depth = if owner {
        DEPTH.fetch_add(1, Ordering::AcqRel) + 1
    } else {
        0
    };
    let kind = 6 + (which - 2) * 2;
    if owner {
        CALLS[which as usize].fetch_add(1, Ordering::Relaxed);
        record(kind, this as u32, item as u32, caller, depth, 0);
    } else {
        FOREIGN.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { SetLastError(error) };
    let original: Block =
        unsafe { std::mem::transmute(ORIGINALS[which as usize].load(Ordering::Acquire)) };
    let result = unsafe { original(this, item) };
    let error = unsafe { GetLastError() };
    if owner {
        record(
            kind + 1,
            this as u32,
            item as u32,
            caller,
            depth,
            u32::from(result),
        );
        NEXT_DRAW.store(true, Ordering::Release);
        DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
    unsafe { SetLastError(error) };
    result
}
#[unsafe(naked)]
unsafe extern "C" fn argv_shim() -> *const *const u8 {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","sub esp,8","push dword ptr [ebp+4]","push ecx","call {implementation}","mov esp,ebp","pop ebp","ret",implementation=sym argv_impl);
}
unsafe extern "C" fn argv_impl(context: u32, caller: u32) -> *const *const u8 {
    type Argv = unsafe extern "C" fn() -> *const *const u8;
    let original: Argv = unsafe { std::mem::transmute(ORIGINALS[4].load(Ordering::Acquire)) };
    // The original only reads a core global. No diagnostic precedes it.
    let result = unsafe { original() };
    let error = unsafe { GetLastError() };
    if caller == ARGV_SITE.load(Ordering::Acquire) {
        if unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire) {
            let depth = DEPTH.fetch_add(1, Ordering::AcqRel) + 1;
            CALLS[4].fetch_add(1, Ordering::Relaxed);
            record(10, context, result as u32, caller, depth, 0);
            NEXT_DRAW.store(true, Ordering::Release);
            DEPTH.fetch_sub(1, Ordering::AcqRel);
        } else {
            FOREIGN.fetch_add(1, Ordering::Relaxed);
        }
    }
    unsafe { SetLastError(error) };
    result
}
#[unsafe(naked)]
unsafe extern "C" fn map_shim() -> u32 {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","sub esp,12","push dword ptr [ebp+4]","call {implementation}","mov esp,ebp","pop ebp","ret",implementation=sym map_impl);
}
unsafe extern "C" fn map_impl(caller: u32) -> u32 {
    let incoming = unsafe { GetLastError() };
    MAP_FLIGHT.fetch_add(1, Ordering::AcqRel);
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    let mut handler = [0; 4];
    let handler = if read(MAP_BINDINGS[2].load(Ordering::Acquire), &mut handler).is_ok() {
        u32::from_le_bytes(handler)
    } else {
        0
    };
    let descriptor = SLOTS_AT[5].load(Ordering::Acquire) as u32 - 4;
    let depth = if owner {
        DEPTH.fetch_add(1, Ordering::AcqRel) + 1
    } else {
        0
    };
    if owner {
        CALLS[5].fetch_add(1, Ordering::Relaxed);
        record(11, handler, descriptor, caller, depth, 0);
    } else {
        FOREIGN.fetch_add(1, Ordering::Relaxed);
    }
    let original: unsafe extern "C" fn() -> u32 =
        unsafe { std::mem::transmute(ORIGINALS[5].load(Ordering::Acquire)) };
    unsafe { SetLastError(incoming) };
    let returned = unsafe { original() };
    let outgoing = unsafe { GetLastError() };
    if owner {
        record(12, handler, descriptor, caller, depth, returned);
        NEXT_DRAW.store(true, Ordering::Release);
        DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
    MAP_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    unsafe { SetLastError(outgoing) };
    returned
}
fn shims() -> [usize; 6] {
    [
        init_shim as *const () as usize,
        done_shim as *const () as usize,
        block_init_shim as *const () as usize,
        destroy_request_shim as *const () as usize,
        argv_shim as *const () as usize,
        map_shim as *const () as usize,
    ]
}
fn replace(index: usize, expected: usize, desired: usize) -> Result<(), String> {
    let at = SLOTS_AT[index].load(Ordering::Acquire);
    let mut old = 0;
    if unsafe { VirtualProtect(at as *const c_void, 4, PAGE_READWRITE, &mut old) } == 0 {
        return Err("cannot unlock transition slot".into());
    }
    if PROTECTION[index].load(Ordering::Acquire) == 0 {
        PROTECTION[index].store(old, Ordering::Release);
    }
    let slot = unsafe { AtomicUsize::from_ptr(at as *mut usize) };
    let exchanged = slot.compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire);
    let mut ignored = 0;
    if unsafe {
        VirtualProtect(
            at as *const c_void,
            4,
            PROTECTION[index].load(Ordering::Acquire),
            &mut ignored,
        )
    } == 0
    {
        return Err(
            "transition page protection not restored; keep DLL pinned and retry removal".into(),
        );
    }
    PROTECTION[index].store(0, Ordering::Release);
    exchanged
        .map(|_| ())
        .map_err(|_| "transition slot owner changed; refusing overwrite".into())
}
pub fn install(thread: u32, b: &EngineControl) -> Result<(), String> {
    if crate::engine_visual::allocated() {
        return Err("release the visual before passive transition observation".into());
    }
    if STATE.load(Ordering::Acquire) != 0 {
        return Err("transition observer already used; restart process".into());
    }
    let mut pump = EngineControl::new(api::UI_PUMP_STATUS);
    crate::engine_phase::fill(&mut pump);
    if pump.phase != 1 {
        return Err("install the process-pinned pump before transition observation".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let (slots, originals) = if b.command == api::UI_TRANSITION_FIXTURE {
        let module = get("engine_fixture.exe")?;
        for at in [b.fixture[1], b.fixture[3]] {
            executable(at, module)?;
        }
        ([b.fixture[0], b.fixture[2]], [b.fixture[1], b.fixture[3]])
    } else if b.command == api::UI_TRANSITION_ATTACH {
        profile::verify_executable(std::path::Path::new(&get("prism3d.exe")?.path))?;
        let game = get("game.dll")?;
        profile::verify_module(std::path::Path::new(&game.path))?;
        let originals = [add(game, FUNCTIONS[0])?, add(game, FUNCTIONS[1])?];
        for at in originals {
            executable(at, game)?;
        }
        ([add(game, SLOTS[0])?, add(game, SLOTS[1])?], originals)
    } else {
        return Err("invalid transition observer mode".into());
    };
    if slots[0] == slots[1] {
        return Err("transition slots must be distinct".into());
    }
    // Preflight BOTH virtuals before any mutation; fixture addresses are explicit.
    for (slot, original) in slots.into_iter().zip(originals) {
        if slot == 0 || !slot.is_multiple_of(4) {
            return Err("invalid transition slot alignment".into());
        }
        let mut bytes = [0; 4];
        process
            .read(slot.into(), &mut bytes)
            .map_err(|e| e.to_string())?;
        if u32::from_le_bytes(bytes) != original {
            return Err("transition slot already modified or incompatible".into());
        }
    }
    OWNER.store(thread, Ordering::Release);
    FIXTURE.store(b.command == api::UI_TRANSITION_FIXTURE, Ordering::Release);
    for i in 0..2 {
        SLOTS_AT[i].store(slots[i] as usize, Ordering::Release);
        ORIGINALS[i].store(originals[i] as usize, Ordering::Release);
    }
    STATE.store(1, Ordering::Release);
    HOOKS.store(2, Ordering::Release);
    for (i, shim) in shims().into_iter().take(2).enumerate() {
        if let Err(error) = replace(i, originals[i] as usize, shim) {
            let rollback = remove();
            return Err(format!("{error}; rollback={rollback:?}"));
        }
    }
    Ok(())
}
/// Add the game's two profiled imports to the SAME timeline. A destroy request
/// schedules work; it is not evidence that a world has already been destroyed.
pub fn install_blocks(thread: u32, b: &EngineControl) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 1 || HOOKS.load(Ordering::Acquire) != 2 {
        return Err(
            "install the actor transition observer exactly once before block observation".into(),
        );
    }
    if ACTIVE.load(Ordering::Acquire) || DEPTH.load(Ordering::Acquire) != 0 {
        return Err("install block observers before arming, outside callbacks".into());
    }
    if thread != OWNER.load(Ordering::Acquire) {
        return Err("block observation owner thread changed".into());
    }
    let fixture = b.command == api::UI_TRANSITION_BLOCKS_FIXTURE;
    if fixture != FIXTURE.load(Ordering::Acquire) {
        return Err("transition fixture/game mode mismatch".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let (slots, originals) = if fixture {
        let module = get("engine_fixture.exe")?;
        for at in [b.fixture[1], b.fixture[3]] {
            executable(at, module)?;
        }
        ([b.fixture[0], b.fixture[2]], [b.fixture[1], b.fixture[3]])
    } else {
        let game = get("game.dll")?;
        let engine = get("p3shared.dll")?;
        profile::verify_module(std::path::Path::new(&game.path))?;
        profile::verify_engine(std::path::Path::new(&engine.path))?;
        let functions = [add(engine, 0xc270)?, add(engine, 0xc600)?];
        for at in functions {
            executable(at, engine)?;
        }
        ([add(game, 0x124560)?, add(game, 0x1242b4)?], functions)
    };
    if slots[0] == slots[1]
        || slots.iter().any(|&s| {
            SLOTS_AT[..2]
                .iter()
                .any(|old| old.load(Ordering::Acquire) == s as usize)
        })
    {
        return Err("transition slots must be distinct".into());
    }
    for (slot, original) in slots.into_iter().zip(originals) {
        if slot == 0 || !slot.is_multiple_of(4) {
            return Err("invalid block slot alignment".into());
        }
        let mut bytes = [0; 4];
        process
            .read(slot.into(), &mut bytes)
            .map_err(|e| e.to_string())?;
        if u32::from_le_bytes(bytes) != original {
            return Err("block slot already modified or incompatible".into());
        }
    }
    for i in 0..2 {
        SLOTS_AT[i + 2].store(slots[i] as usize, Ordering::Release);
        ORIGINALS[i + 2].store(originals[i] as usize, Ordering::Release);
    }
    HOOKS.store(4, Ordering::Release);
    for i in 2..4 {
        if let Err(error) = replace(i, originals[i - 2] as usize, shims()[i]) {
            let rollback = remove();
            return Err(format!(
                "{error}; rollback={rollback:?}; reopen process before reinstalling"
            ));
        }
    }
    Ok(())
}
pub fn install_commands(thread: u32, b: &EngineControl) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 1 || HOOKS.load(Ordering::Acquire) != 4 {
        return Err("install block observation before command observation, exactly once".into());
    }
    if ACTIVE.load(Ordering::Acquire)
        || DEPTH.load(Ordering::Acquire) != 0
        || thread != OWNER.load(Ordering::Acquire)
    {
        return Err("install command observation before arming, on the owner thread".into());
    }
    let fixture = b.command == api::UI_TRANSITION_COMMANDS_FIXTURE;
    if fixture != FIXTURE.load(Ordering::Acquire) {
        return Err("transition fixture/game mode mismatch".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let (slot, original, site) = if fixture {
        let module = get("engine_fixture.exe")?;
        executable(b.fixture[1], module)?;
        executable(b.fixture[2], module)?;
        (b.fixture[0], b.fixture[1], b.fixture[2])
    } else {
        let core = get("prism3d.exe")?;
        let game = get("game.dll")?;
        profile::verify_executable(std::path::Path::new(&core.path))?;
        profile::verify_module(std::path::Path::new(&game.path))?;
        let original = add(core, 0xe210)?;
        let site = add(game, 0xd3596)?;
        executable(original, core)?;
        executable(site, game)?;
        (add(game, 0x160930)?, original, site)
    };
    if slot == 0
        || !slot.is_multiple_of(4)
        || SLOTS_AT[..4]
            .iter()
            .any(|s| s.load(Ordering::Acquire) == slot as usize)
    {
        return Err("invalid or duplicate command slot".into());
    }
    let mut bytes = [0; 4];
    process
        .read(slot.into(), &mut bytes)
        .map_err(|e| e.to_string())?;
    if u32::from_le_bytes(bytes) != original {
        return Err("command slot already modified or incompatible".into());
    }
    SLOTS_AT[4].store(slot as usize, Ordering::Release);
    ORIGINALS[4].store(original as usize, Ordering::Release);
    ARGV_SITE.store(site, Ordering::Release);
    HOOKS.store(5, Ordering::Release);
    if let Err(error) = replace(4, original as usize, argv_shim as *const () as usize) {
        let rollback = remove();
        return Err(format!(
            "{error}; rollback={rollback:?}; reopen process before reinstalling"
        ));
    }
    Ok(())
}
/// Adds a passive cdecl descriptor hook. Never defers or synthesizes a command.
pub fn install_map(thread: u32, b: &EngineControl) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 1 || HOOKS.load(Ordering::Acquire) != 5 {
        return Err("install command observation before map observation, exactly once".into());
    }
    if ACTIVE.load(Ordering::Acquire)
        || DEPTH.load(Ordering::Acquire) != 0
        || MAP_FLIGHT.load(Ordering::Acquire) != 0
        || thread != OWNER.load(Ordering::Acquire)
    {
        return Err("install map observation before arming, on the owner thread".into());
    }
    let fixture = b.command == api::UI_TRANSITION_MAP_FIXTURE;
    if fixture != FIXTURE.load(Ordering::Acquire) {
        return Err("transition fixture/game mode mismatch".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let (descriptor, original, bindings) = if fixture {
        executable(b.fixture[1], get("engine_fixture.exe")?)?;
        (
            b.fixture[0],
            b.fixture[1],
            [b.fixture[2], b.fixture[3], b.fixture[4]],
        )
    } else {
        let audit = crate::inspection::commands::inspect(&process).map_err(|e| e.to_string())?;
        if !audit.map_registered || !audit.map_profile_matches {
            return Err("map is not registered with the profiled descriptor/callback".into());
        }
        let core = get("prism3d.exe")?;
        let game = get("game.dll")?;
        executable(audit.map_callback, game)?;
        (
            audit.map_descriptor,
            audit.map_callback,
            [
                add(core, profile::COMMAND_ARGC)?,
                add(core, profile::COMMAND_ARGV)?,
                add(game, profile::COMMAND_CONTEXT)?,
            ],
        )
    };
    let slot =
        crate::command_registry::map_observer_slot(descriptor, original, bindings, |at, out| {
            process.read(at, out).map_err(|e| e.to_string())
        })?;
    if SLOTS_AT[..5].iter().any(|s| {
        let at = s.load(Ordering::Acquire) as u32;
        at == slot || bindings.contains(&at)
    }) {
        return Err("map observer aliases an installed transition slot".into());
    }
    SLOTS_AT[5].store(slot as usize, Ordering::Release);
    ORIGINALS[5].store(original as usize, Ordering::Release);
    for (dst, src) in MAP_BINDINGS.iter().zip(bindings) {
        dst.store(src, Ordering::Release);
    }
    HOOKS.store(6, Ordering::Release);
    if let Err(error) = replace(5, original as usize, shims()[5]) {
        let rollback = remove();
        return Err(format!(
            "{error}; rollback={rollback:?}; reopen process before reinstalling"
        ));
    }
    Ok(())
}
pub fn remove() -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 1 {
        return Ok(());
    }
    if DEPTH.load(Ordering::Acquire) != 0 || MAP_FLIGHT.load(Ordering::Acquire) != 0 {
        return Err("transition callback active; retry removal".into());
    }
    for (i, shim) in shims()
        .into_iter()
        .take(HOOKS.load(Ordering::Acquire) as usize)
        .enumerate()
        .rev()
    {
        let at = SLOTS_AT[i].load(Ordering::Acquire);
        let original = ORIGINALS[i].load(Ordering::Acquire);
        let current = unsafe { AtomicUsize::from_ptr(at as *mut usize) }.load(Ordering::Acquire);
        replace(
            i,
            if current == original { original } else { shim },
            original,
        )?;
    }
    ACTIVE.store(false, Ordering::Release);
    STATE.store(2, Ordering::Release);
    Ok(())
}
pub(crate) fn installed() -> bool {
    STATE.load(Ordering::Acquire) == 1
}
pub fn control(b: &mut EngineControl) -> Result<(), String> {
    let mut trace = TRACE.lock().map_err(|_| "transition trace poisoned")?;
    if b.command == api::TRANSITION_ARM {
        if STATE.load(Ordering::Acquire) != 1 {
            return Err("transition observer not installed".into());
        }
        if DEPTH.load(Ordering::Acquire) != 0 || MAP_FLIGHT.load(Ordering::Acquire) != 0 {
            return Err("transition callback active; retry arming".into());
        }
        let generation = trace
            .generation
            .checked_add(1)
            .ok_or("transition generation exhausted")?;
        // Rows are overwritten on append; avoid a large temporary on the game's stack.
        trace.generation = generation;
        trace.len = 0;
        trace.flags = 0;
        LOST.store(false, Ordering::Release);
        NEXT_DRAW.store(true, Ordering::Release);
        ACTIVE.store(true, Ordering::Release);
    }
    if b.command == api::TRANSITION_READ {
        if b.cycles != trace.generation {
            return Err("transition generation changed; retry report".into());
        }
        let row = trace
            .rows
            .get(b.reserved as usize)
            .filter(|_| (b.reserved as usize) < trace.len)
            .ok_or("transition record unavailable")?;
        row.encode(b);
        return Ok(());
    }
    b.object_size = STATE.load(Ordering::Acquire);
    b.entity = OWNER.load(Ordering::Acquire);
    b.cycles = trace.generation;
    b.registry_count = trace.len as u32;
    b.count_before = CALLS[0].load(Ordering::Acquire);
    b.count_created = CALLS[1].load(Ordering::Acquire);
    b.count_after = FOREIGN.load(Ordering::Acquire);
    b.frames[..7].copy_from_slice(&[
        CALLS[2].load(Ordering::Acquire),
        CALLS[3].load(Ordering::Acquire),
        u32::from(HOOKS.load(Ordering::Acquire) >= 4 && STATE.load(Ordering::Acquire) == 1),
        CALLS[4].load(Ordering::Acquire),
        u32::from(HOOKS.load(Ordering::Acquire) >= 5 && STATE.load(Ordering::Acquire) == 1),
        CALLS[5].load(Ordering::Acquire),
        u32::from(HOOKS.load(Ordering::Acquire) == 6 && STATE.load(Ordering::Acquire) == 1),
    ]);
    b.reserved = trace.flags
        | u32::from(ACTIVE.load(Ordering::Acquire))
        | if LOST.load(Ordering::Acquire) {
            observation::LOST
        } else {
            0
        };
    Ok(())
}

//! Passive observation of one path flush. No engine calls beyond forwarding
//! the intercepted original exactly once. Requires the process-pinned pump.
use crate::{
    api::{self, EngineControl},
    engine_lifecycle::{add, executable},
    inspection::process::Process,
    pe_exports::{self, Image},
    profile,
    render_observation::{self as observation, CAPACITY, Record},
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
        Diagnostics::Debug::ReadProcessMemory,
        Memory::{PAGE_READWRITE, VirtualProtect},
        Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId},
    },
};
type Render = unsafe extern "thiscall" fn(*mut c_void, *mut c_void);
type Flush = unsafe extern "C" fn();
static STATE: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static ORIGINAL: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
static SLOTS: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
static PROTECTION: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static HEADS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static FLUSHES: AtomicU32 = AtomicU32::new(0);
static POOLS: AtomicU32 = AtomicU32::new(0);
static FOREIGN: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
pub(crate) fn callback_depth() -> u32 {
    DEPTH.load(Ordering::Acquire)
}
static CAPTURING: AtomicBool = AtomicBool::new(false);
static LOST: AtomicBool = AtomicBool::new(false);
static TRACE: Mutex<Trace> = Mutex::new(Trace::new());
struct Trace {
    generation: u32,
    armed: bool,
    capturing: bool,
    len: usize,
    flags: u32,
    pools: [u32; 16],
    devices: [u32; 16],
    pool_len: usize,
    rows: [Record; CAPACITY],
}
impl Trace {
    const fn new() -> Self {
        Self {
            generation: 0,
            armed: false,
            capturing: false,
            len: 0,
            flags: 0,
            pools: [0; 16],
            devices: [0; 16],
            pool_len: 0,
            rows: [Record {
                kind: 0,
                thread: 0,
                caller: 0,
                device: 0,
                pool: 0,
                prototypes: 0,
                root: 0,
                first: 0,
                first_refs: 0,
                buckets: 0,
                flags: 0,
                matrices: [0; 32],
            }; CAPACITY],
        }
    }
    fn push(&mut self, row: Record) {
        if self.len == CAPACITY {
            self.flags |= 4;
        } else {
            self.rows[self.len] = row;
            self.len += 1;
        }
    }
}
pub(crate) fn read(at: u32, out: &mut [u8]) -> Result<(), ()> {
    if at == 0 || at.checked_add(out.len() as u32).is_none() {
        return Err(());
    }
    let mut done = 0;
    if unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            at as usize as *const _,
            out.as_mut_ptr().cast(),
            out.len(),
            &mut done,
        )
    } == 0
        || done != out.len()
    {
        Err(())
    } else {
        Ok(())
    }
}
fn sample(kind: u32, caller: u32, device: u32, pool: u32) -> Record {
    let mut row = Record {
        kind,
        thread: unsafe { GetCurrentThreadId() },
        caller,
        device,
        pool,
        ..Record::default()
    };
    if pool != 0 {
        observation::queue(&mut row, read);
    }
    for (i, head) in HEADS.iter().enumerate() {
        let mut pointer = [0; 4];
        let mut bytes = [0; 64];
        if read(head.load(Ordering::Acquire), &mut pointer).is_ok()
            && read(u32::from_le_bytes(pointer), &mut bytes).is_ok()
        {
            for (out, v) in row.matrices[i * 16..i * 16 + 16]
                .iter_mut()
                .zip(bytes.as_chunks::<4>().0)
            {
                *out = u32::from_le_bytes(*v);
            }
            row.flags |= 1 << i;
        }
    }
    row
}
fn record(kind: u32, caller: u32, device: u32, pool: u32) {
    if STATE.load(Ordering::Acquire) != 1 {
        return;
    }
    if LOST.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut trace) = TRACE.try_lock() else {
        if CAPTURING.load(Ordering::Acquire) {
            LOST.store(true, Ordering::Release);
        }
        return;
    };
    if trace.capturing {
        if trace.len == CAPACITY {
            trace.flags |= 4;
            return;
        }
        if kind == 2 && !trace.pools[..trace.pool_len].contains(&pool) {
            let i = trace.pool_len;
            if i == trace.pools.len() {
                trace.flags |= 8;
            } else {
                trace.pools[i] = pool;
                trace.devices[i] = device;
                trace.pool_len += 1;
            }
        }
        trace.push(sample(kind, caller, device, pool));
    }
}
#[unsafe(naked)]
unsafe extern "thiscall" fn render_shim(_: *mut c_void, _: *mut c_void) {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","sub esp,4","push dword ptr [ebp+4]","push dword ptr [ebp+8]","push ecx","call {implementation}","mov esp,ebp","pop ebp","ret 4",implementation=sym render_impl);
}
unsafe extern "C" fn render_impl(device: *mut c_void, pool: *mut c_void, caller: u32) {
    let error = unsafe { GetLastError() };
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    if owner {
        POOLS.fetch_add(1, Ordering::Relaxed);
        record(2, caller, device as u32, pool as u32);
    } else {
        FOREIGN.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { SetLastError(error) };
    let original: Render = unsafe { std::mem::transmute(ORIGINAL[0].load(Ordering::Acquire)) };
    unsafe { original(device, pool) };
    let error = unsafe { GetLastError() };
    if owner {
        record(3, caller, device as u32, pool as u32);
    }
    unsafe { SetLastError(error) };
}
#[unsafe(naked)]
unsafe extern "C" fn flush_shim() {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","sub esp,12","push dword ptr [ebp+4]","call {implementation}","mov esp,ebp","pop ebp","ret",implementation=sym flush_impl);
}
unsafe extern "C" fn flush_impl(caller: u32) {
    let error = unsafe { GetLastError() };
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    let depth = if owner {
        DEPTH.fetch_add(1, Ordering::AcqRel) + 1
    } else {
        0
    };
    if owner {
        FLUSHES.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut trace) = TRACE.try_lock() {
            if depth == 1 && trace.armed {
                trace.armed = false;
                trace.capturing = true;
                CAPTURING.store(true, Ordering::Release);
                trace.push(sample(1, caller, 0, 0));
            } else if depth > 1 && trace.capturing {
                trace.flags |= 16;
            } else if depth == 1 && trace.capturing {
                LOST.store(true, Ordering::Release);
            }
        }
    } else {
        FOREIGN.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { SetLastError(error) };
    let original: Flush = unsafe { std::mem::transmute(ORIGINAL[1].load(Ordering::Acquire)) };
    unsafe { original() };
    let error = unsafe { GetLastError() };
    if owner {
        if depth == 1
            && let Ok(mut trace) = TRACE.try_lock()
            && trace.capturing
        {
            for i in 0..trace.pool_len {
                if trace.len == CAPACITY {
                    trace.flags |= 4;
                    break;
                }
                let row = sample(4, caller, trace.devices[i], trace.pools[i]);
                trace.push(row);
            }
            if trace.len < CAPACITY {
                trace.push(sample(5, caller, 0, 0));
            } else {
                trace.flags |= 4;
            }
            trace.capturing = false;
            CAPTURING.store(false, Ordering::Release);
        } else if depth == 1 && CAPTURING.load(Ordering::Acquire) {
            LOST.store(true, Ordering::Release);
        }
        DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
    unsafe { SetLastError(error) };
}
fn shim(index: usize) -> usize {
    if index == 0 {
        render_shim as *const () as usize
    } else {
        flush_shim as *const () as usize
    }
}
fn replace(index: usize, expected: usize, desired: usize) -> Result<(), String> {
    let slot = SLOTS[index].load(Ordering::Acquire);
    let mut old = 0;
    if unsafe { VirtualProtect(slot as *const c_void, 4, PAGE_READWRITE, &mut old) } == 0 {
        return Err("cannot unlock render slot".into());
    }
    if PROTECTION[index].load(Ordering::Acquire) == 0 {
        PROTECTION[index].store(old, Ordering::Release);
    }
    let word = unsafe { AtomicUsize::from_ptr(slot as *mut usize) };
    let exchanged = word.compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire);
    let mut ignored = 0;
    if unsafe {
        VirtualProtect(
            slot as *const c_void,
            4,
            PROTECTION[index].load(Ordering::Acquire),
            &mut ignored,
        )
    } == 0
    {
        return Err(
            "render slot protection not restored; keep module pinned and retry removal".into(),
        );
    }
    PROTECTION[index].store(0, Ordering::Release);
    exchanged
        .map(|_| ())
        .map_err(|_| "render slot owner changed; refusing overwrite".into())
}
pub fn install(thread: u32, b: &EngineControl) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 0 {
        return Err("render observer already used; restart process".into());
    }
    let mut pump = EngineControl::new(api::UI_PUMP_STATUS);
    crate::engine_phase::fill(&mut pump);
    if pump.phase != 1 {
        return Err("install the process-pinned pump before render observation".into());
    }
    let p = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = p.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let (slots, originals, heads) = if b.command == api::UI_RENDER_FIXTURE {
        let host = get("engine_fixture.exe")?;
        executable(b.fixture[1], host)?;
        executable(b.fixture[3], host)?;
        (
            [b.fixture[0], b.fixture[2]],
            [b.fixture[1], b.fixture[3]],
            [b.fixture[4], b.fixture[5]],
        )
    } else if b.command == api::UI_RENDER_ATTACH {
        for (name, verify) in [
            (
                "prism3d.exe",
                profile::verify_executable as fn(&std::path::Path) -> Result<(), String>,
            ),
            ("game.dll", profile::verify_module),
            ("p3shared.dll", profile::verify_engine),
            ("gfxshared.dll", profile::verify_gfx),
        ] {
            verify(std::path::Path::new(&get(name)?.path))?;
        }
        let core = get("prism3d.exe")?;
        let game = get("game.dll")?;
        let gfx = get("gfxshared.dll")?;
        let images = modules
            .iter()
            .map(|m| {
                Ok(Image {
                    name: m.name.clone(),
                    base: m.base,
                    size: u32::try_from(m.size).map_err(|e| e.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (module, name, rva) in [
            (gfx, "?render_pool@r_device_t@@QAEXPAUr_pool_t@@@Z", 0x3690),
            (core, "gfx_path_flush", 0x4820),
        ] {
            let expected = add(module, rva)?;
            executable(expected, module)?;
            if pe_exports::resolve(&images, &module.name, name, |a, b| {
                p.read(a, b).map_err(|e| e.to_string())
            })? != u64::from(expected)
            {
                return Err("render export differs from profile".into());
            }
        }
        (
            [add(core, 0x22240)?, add(game, 0x160ea0)?],
            [add(gfx, 0x3690)?, add(core, 0x4820)?],
            [add(core, 0x5e000)?, add(core, 0x5e848)?],
        )
    } else {
        return Err("invalid render observer mode".into());
    };
    if slots[0] == slots[1] {
        return Err("render slots overlap".into());
    }
    for i in 0..2 {
        if slots[i] == 0 || !slots[i].is_multiple_of(4) {
            return Err("invalid render slot alignment".into());
        }
        let mut bytes = [0; 4];
        p.read(slots[i].into(), &mut bytes)
            .map_err(|e| e.to_string())?;
        if u32::from_le_bytes(bytes) != originals[i] {
            return Err(format!("render slot {i} already modified or incompatible"));
        }
    }
    // The pump has already pinned this very DLL until process exit. Initialize
    // forwarding data before publishing either shim, including partial failure.
    OWNER.store(thread, Ordering::Release);
    for i in 0..2 {
        ORIGINAL[i].store(originals[i] as usize, Ordering::Release);
        SLOTS[i].store(slots[i] as usize, Ordering::Release);
        HEADS[i].store(heads[i], Ordering::Release);
    }
    STATE.store(1, Ordering::Release);
    for (i, original) in originals.into_iter().enumerate() {
        if let Err(error) = replace(i, original as usize, shim(i)) {
            let cleanup = remove();
            return Err(format!("{error}; rollback={cleanup:?}"));
        }
    }
    Ok(())
}
pub(crate) fn installed() -> bool {
    STATE.load(Ordering::Acquire) == 1
}
/// Owner-thread flush count, used by other passive probes to order their events.
pub(crate) fn flush_calls() -> u32 {
    FLUSHES.load(Ordering::Acquire)
}
pub fn remove() -> Result<(), String> {
    if crate::engine_visual::allocated() {
        return Err("release the test visual before removing the render observer".into());
    }
    if STATE.load(Ordering::Acquire) != 1 {
        return Ok(());
    }
    // Pinning permits late calls to shims; ORIGINAL and all static storage stay valid.
    for i in (0..2).rev() {
        let slot = SLOTS[i].load(Ordering::Acquire);
        let original = ORIGINAL[i].load(Ordering::Acquire);
        let current = unsafe { AtomicUsize::from_ptr(slot as *mut usize) }.load(Ordering::Acquire);
        replace(
            i,
            if current == original {
                original
            } else {
                shim(i)
            },
            original,
        )?;
    }
    STATE.store(2, Ordering::Release);
    CAPTURING.store(false, Ordering::Release);
    if let Ok(mut trace) = TRACE.try_lock() {
        trace.armed = false;
        trace.capturing = false;
    }
    Ok(())
}
pub fn control(b: &mut EngineControl) -> Result<(), String> {
    let mut trace = TRACE.lock().map_err(|_| "render trace poisoned")?;
    if LOST.load(Ordering::Acquire) {
        trace.flags |= 32;
        trace.armed = false;
        trace.capturing = false;
        CAPTURING.store(false, Ordering::Release);
    }
    if b.command == api::RENDER_ARM {
        if STATE.load(Ordering::Acquire) != 1 {
            return Err("render observer not installed".into());
        }
        if DEPTH.load(Ordering::Acquire) != 0 {
            return Err("flush in progress; retry arming between flushes".into());
        }
        let generation = trace
            .generation
            .checked_add(1)
            .ok_or("render generation exhausted")?;
        *trace = Trace::new();
        LOST.store(false, Ordering::Release);
        trace.generation = generation;
        trace.armed = true;
    }
    if b.command == api::RENDER_READ {
        if b.cycles != trace.generation {
            return Err("render generation changed; retry report".into());
        }
        let index = b.reserved as usize;
        let row = trace
            .rows
            .get(index)
            .filter(|_| index < trace.len)
            .ok_or("render record unavailable")?;
        row.encode(b, index as u32);
        return Ok(());
    }
    b.object_size = STATE.load(Ordering::Acquire);
    b.cycles = trace.generation;
    b.registry_count = trace.len as u32;
    b.entity = OWNER.load(Ordering::Acquire);
    b.count_before = FLUSHES.load(Ordering::Acquire);
    b.count_created = POOLS.load(Ordering::Acquire);
    b.count_after = FOREIGN.load(Ordering::Acquire);
    b.reserved = trace.flags | u32::from(trace.armed) | (u32::from(trace.capturing) << 1);
    Ok(())
}

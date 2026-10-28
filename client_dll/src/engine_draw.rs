//! Passive observation of actors.car draw. Forwards the intercepted virtual
//! exactly once and only reads engine memory. Requires the process-pinned pump.
use crate::{
    api::{self, EngineControl},
    draw_observation::{self as observation, CAPACITY, Record},
    engine_lifecycle::{add, executable},
    engine_render::{flush_calls, read},
    inspection::process::Process,
    profile,
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
type Draw = unsafe extern "thiscall" fn(*mut c_void);
/// actors.car vtable +0x6c (entity_t::draw); see ROADMAP for the static contract.
const CAR_DRAW_SLOT: u32 = 0x12580c;
const CAR_DRAW: u32 = 0x5a6a0;
const WORLD_HEAD: u32 = 0x5e000;
const CAMERA_HEAD: u32 = 0x5e848;
const PATH_CURRENT: u32 = 0x5ea7c;
const VIEWPORT: u32 = 0x5ee2c;
static STATE: AtomicU32 = AtomicU32::new(0);
static OWNER: AtomicU32 = AtomicU32::new(0);
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static SLOT: AtomicUsize = AtomicUsize::new(0);
static PROTECTION: AtomicU32 = AtomicU32::new(0);
/// World head, camera head, current path (render info lives 8 bytes below), viewport.
static GLOBALS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static DRAWS: AtomicU32 = AtomicU32::new(0);
static FOREIGN: AtomicU32 = AtomicU32::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static LOST: AtomicBool = AtomicBool::new(false);
/// Armed or capturing; idle draws never touch the trace lock.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// 1 game, 2 own fixture; selects the test visual mode.
static MODE: AtomicU32 = AtomicU32::new(0);
static TRACE: Mutex<Trace> = Mutex::new(Trace::new());
struct Trace {
    generation: u32,
    armed: bool,
    capturing: bool,
    len: usize,
    flags: u32,
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
            rows: [Record {
                kind: 0,
                thread: 0,
                caller: 0,
                this: 0,
                vtable: 0,
                state: 0,
                body: 0,
                depth: 0,
                path: 0,
                path_info: 0,
                viewport: 0,
                flushes: 0,
                flags: 0,
                pose: [0; 7],
                matrices: [0; 32],
            }; CAPACITY],
        }
    }
}
fn word(at: u32) -> Result<u32, ()> {
    let mut b = [0; 4];
    read(at, &mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn sample(kind: u32, this: u32, caller: u32) -> Record {
    let [world, camera, path, viewport] = GLOBALS.each_ref().map(|g| g.load(Ordering::Acquire));
    let mut row = Record {
        kind,
        thread: unsafe { GetCurrentThreadId() },
        caller,
        this,
        vtable: word(this).unwrap_or(0),
        flushes: flush_calls(),
        path: word(path).unwrap_or(0),
        path_info: path.checked_sub(8).and_then(|a| word(a).ok()).unwrap_or(0),
        viewport: word(viewport).unwrap_or(0),
        ..Record::default()
    };
    let mut state = [0];
    if read(this.wrapping_add(0x1e9), &mut state).is_ok() {
        row.state = u32::from(state[0]);
    }
    for (i, head) in [world, camera].into_iter().enumerate() {
        let mut bytes = [0; 64];
        if let Ok(top) = word(head)
            && read(top, &mut bytes).is_ok()
        {
            for (out, v) in row.matrices[i * 16..i * 16 + 16]
                .iter_mut()
                .zip(bytes.as_chunks::<4>().0)
            {
                *out = u32::from_le_bytes(*v);
            }
            row.flags |= 1 << i;
            if i == 0
                && let Some(depth) = observation::depth(head, top)
            {
                row.depth = depth;
                row.flags |= observation::DEPTH;
            }
        }
    }
    // Physics body chain observed in the exterior draw: quaternion +0x14, position +0x30.
    let mut pose = [0; 28];
    if let Ok(body) = word(this.wrapping_add(0x1d0))
        && let Ok(state) = word(body.wrapping_add(0x3e4))
        && read(state.wrapping_add(0x14), &mut pose[..16]).is_ok()
        && read(state.wrapping_add(0x30), &mut pose[16..]).is_ok()
    {
        row.body = state;
        for (out, v) in row.pose.iter_mut().zip(pose.as_chunks::<4>().0) {
            *out = u32::from_le_bytes(*v);
        }
        row.flags |= observation::BODY;
    }
    row
}
fn record(kind: u32, this: u32, caller: u32) {
    if STATE.load(Ordering::Acquire) != 1
        || !ACTIVE.load(Ordering::Acquire)
        || LOST.load(Ordering::Acquire)
    {
        return;
    }
    let Ok(mut trace) = TRACE.try_lock() else {
        LOST.store(true, Ordering::Release);
        return;
    };
    if kind == 1 && trace.armed {
        trace.armed = false;
        trace.capturing = true;
    }
    if trace.capturing {
        let i = trace.len;
        if i == CAPACITY {
            trace.flags |= 4;
            trace.capturing = false;
            ACTIVE.store(false, Ordering::Release);
            return;
        }
        trace.rows[i] = sample(kind, this, caller);
        trace.len += 1;
        // Stop after a return when no complete enter/return pair fits anymore.
        if kind == 2 && trace.len + 2 > CAPACITY {
            trace.capturing = false;
            ACTIVE.store(false, Ordering::Release);
        }
    }
}
#[unsafe(naked)]
unsafe extern "thiscall" fn draw_shim(_: *mut c_void) {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","sub esp,8","push dword ptr [ebp+4]","push ecx","call {implementation}","mov esp,ebp","pop ebp","ret",implementation=sym draw_impl);
}
unsafe extern "C" fn draw_impl(this: *mut c_void, caller: u32) {
    let error = unsafe { GetLastError() };
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    let depth = if owner {
        DEPTH.fetch_add(1, Ordering::AcqRel) + 1
    } else {
        0
    };
    if owner {
        DRAWS.fetch_add(1, Ordering::Relaxed);
        if depth == 1 {
            record(1, this as u32, caller);
        } else if let Ok(mut trace) = TRACE.try_lock() {
            trace.flags |= 16;
        }
    } else {
        FOREIGN.fetch_add(1, Ordering::Relaxed);
    }
    unsafe { SetLastError(error) };
    let original: Draw = unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Acquire)) };
    unsafe { original(this) };
    let error = unsafe { GetLastError() };
    if owner {
        if depth == 1 {
            record(2, this as u32, caller);
            crate::engine_transition::after_draw(this as u32);
            crate::engine_visual::after_car_draw(this as u32, globals());
        }
        DEPTH.fetch_sub(1, Ordering::AcqRel);
    }
    unsafe { SetLastError(error) };
}
fn replace(expected: usize, desired: usize) -> Result<(), String> {
    let slot = SLOT.load(Ordering::Acquire);
    let mut old = 0;
    if unsafe { VirtualProtect(slot as *const c_void, 4, PAGE_READWRITE, &mut old) } == 0 {
        return Err("cannot unlock draw slot".into());
    }
    if PROTECTION.load(Ordering::Acquire) == 0 {
        PROTECTION.store(old, Ordering::Release);
    }
    let word = unsafe { AtomicUsize::from_ptr(slot as *mut usize) };
    let exchanged = word.compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire);
    let mut ignored = 0;
    if unsafe {
        VirtualProtect(
            slot as *const c_void,
            4,
            PROTECTION.load(Ordering::Acquire),
            &mut ignored,
        )
    } == 0
    {
        return Err(
            "draw slot protection not restored; keep module pinned and retry removal".into(),
        );
    }
    PROTECTION.store(0, Ordering::Release);
    exchanged
        .map(|_| ())
        .map_err(|_| "draw slot owner changed; refusing overwrite".into())
}
/// Installed observer mode: Some(true) fixture, Some(false) game.
pub(crate) fn mode() -> Option<bool> {
    (STATE.load(Ordering::Acquire) == 1).then(|| MODE.load(Ordering::Acquire) == 2)
}
pub(crate) fn globals() -> [u32; 4] {
    GLOBALS.each_ref().map(|g| g.load(Ordering::Acquire))
}
pub fn install(thread: u32, b: &EngineControl) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 0 {
        return Err("draw observer already used; restart process".into());
    }
    let mut pump = EngineControl::new(api::UI_PUMP_STATUS);
    crate::engine_phase::fill(&mut pump);
    if pump.phase != 1 {
        return Err("install the process-pinned pump before draw observation".into());
    }
    let p = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let modules = p.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    let (slot, original, globals) = if b.command == api::UI_DRAW_FIXTURE {
        executable(b.fixture[1], get("engine_fixture.exe")?)?;
        (
            b.fixture[0],
            b.fixture[1],
            [b.fixture[2], b.fixture[3], b.fixture[4], b.fixture[5]],
        )
    } else if b.command == api::UI_DRAW_ATTACH {
        let core = get("prism3d.exe")?;
        let game = get("game.dll")?;
        profile::verify_executable(std::path::Path::new(&core.path))?;
        profile::verify_module(std::path::Path::new(&game.path))?;
        let original = add(game, CAR_DRAW)?;
        executable(original, game)?;
        (
            add(game, CAR_DRAW_SLOT)?,
            original,
            [
                add(core, WORLD_HEAD)?,
                add(core, CAMERA_HEAD)?,
                add(core, PATH_CURRENT)?,
                add(core, VIEWPORT)?,
            ],
        )
    } else {
        return Err("invalid draw observer mode".into());
    };
    if slot == 0 || !slot.is_multiple_of(4) {
        return Err("invalid draw slot alignment".into());
    }
    let mut bytes = [0; 4];
    p.read(slot.into(), &mut bytes).map_err(|e| e.to_string())?;
    if u32::from_le_bytes(bytes) != original {
        return Err("draw slot already modified or incompatible".into());
    }
    // The pump pinned this DLL; publish forwarding data before the shim.
    OWNER.store(thread, Ordering::Release);
    ORIGINAL.store(original as usize, Ordering::Release);
    SLOT.store(slot as usize, Ordering::Release);
    for (g, v) in GLOBALS.iter().zip(globals) {
        g.store(v, Ordering::Release);
    }
    MODE.store(
        if b.command == api::UI_DRAW_FIXTURE {
            2
        } else {
            1
        },
        Ordering::Release,
    );
    STATE.store(1, Ordering::Release);
    if let Err(error) = replace(original as usize, draw_shim as *const () as usize) {
        let cleanup = remove();
        return Err(format!("{error}; rollback={cleanup:?}"));
    }
    Ok(())
}
pub fn remove() -> Result<(), String> {
    if crate::engine_visual::shown() {
        return Err("hide the test visual before removing the draw observer".into());
    }
    if STATE.load(Ordering::Acquire) != 1 {
        return Ok(());
    }
    // Pinning permits late shim calls; ORIGINAL and the trace remain valid.
    let slot = SLOT.load(Ordering::Acquire);
    let original = ORIGINAL.load(Ordering::Acquire);
    let current = unsafe { AtomicUsize::from_ptr(slot as *mut usize) }.load(Ordering::Acquire);
    replace(
        if current == original {
            original
        } else {
            draw_shim as *const () as usize
        },
        original,
    )?;
    STATE.store(2, Ordering::Release);
    ACTIVE.store(false, Ordering::Release);
    if let Ok(mut trace) = TRACE.try_lock() {
        trace.armed = false;
        trace.capturing = false;
    }
    Ok(())
}
pub fn control(b: &mut EngineControl) -> Result<(), String> {
    let mut trace = TRACE.lock().map_err(|_| "draw trace poisoned")?;
    if LOST.load(Ordering::Acquire) {
        trace.flags |= 32;
        trace.armed = false;
        trace.capturing = false;
        ACTIVE.store(false, Ordering::Release);
    }
    if b.command == api::DRAW_ARM {
        if STATE.load(Ordering::Acquire) != 1 {
            return Err("draw observer not installed".into());
        }
        let generation = trace
            .generation
            .checked_add(1)
            .ok_or("draw generation exhausted")?;
        *trace = Trace::new();
        LOST.store(false, Ordering::Release);
        trace.generation = generation;
        trace.armed = true;
        ACTIVE.store(true, Ordering::Release);
    }
    if b.command == api::DRAW_READ {
        if b.cycles != trace.generation {
            return Err("draw generation changed; retry report".into());
        }
        let index = b.reserved as usize;
        let row = trace
            .rows
            .get(index)
            .filter(|_| index < trace.len)
            .ok_or("draw record unavailable")?;
        row.encode(b, index as u32);
        return Ok(());
    }
    b.object_size = STATE.load(Ordering::Acquire);
    b.cycles = trace.generation;
    b.registry_count = trace.len as u32;
    b.entity = OWNER.load(Ordering::Acquire);
    b.count_before = DRAWS.load(Ordering::Acquire);
    b.count_created = flush_calls();
    b.count_after = FOREIGN.load(Ordering::Acquire);
    b.reserved = trace.flags | u32::from(trace.armed) | (u32::from(trace.capturing) << 1);
    Ok(())
}

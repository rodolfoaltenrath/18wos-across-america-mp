//! One test visual: a retained private PSM instance drawn right after the
//! player's actors.car draw, at a fixed world offset. No physics, collision,
//! HUD, entity or world insertion. Create/release run only on the loop queue;
//! drawing runs only inside the observed draw on the same window thread.
use crate::{
    api::{self, EngineControl},
    draw_observation::{self as observation, Visual},
    engine_info::{self as info, Info, offset, value},
    engine_lifecycle::{add, executable},
    engine_model::ModelName,
    engine_render::{flush_calls, read},
    inspection::process::Process,
    pe_exports::{self, Image},
};
use multiplayer_server::protocol::{Position, Rotation};
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::Threading::{GetCurrentProcessId, GetCurrentThreadId},
};
type Plain = unsafe extern "C" fn();
type Vector = unsafe extern "C" fn(*const f32);
type Draw = unsafe extern "C" fn(*mut c_void, u32, u32);
/// Flushes that must pass after the last draw before the instance may be freed.
pub const SETTLE_FLUSHES: u32 = 8;
static STATE: AtomicU32 = AtomicU32::new(0); // 0 none, 1 allocated
static SHOWN: AtomicBool = AtomicBool::new(false);
static FAULT: AtomicU32 = AtomicU32::new(0);
static ITEM: AtomicUsize = AtomicUsize::new(0);
static INFO: AtomicUsize = AtomicUsize::new(0);
/// psm_draw, gfx_push, gfx_pop, gfx_translate, gfx_qrotate, psm_free, info release.
static FUNCTIONS: [AtomicUsize; 7] = [const { AtomicUsize::new(0) }; 7];
static OFFSET: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static DRAWN: AtomicU32 = AtomicU32::new(0);
static SKIPPED: AtomicU32 = AtomicU32::new(0);
static LAST_FLUSH: AtomicU32 = AtomicU32::new(0);
static HIDDEN_AT: AtomicU32 = AtomicU32::new(0);
/// 0 = local player plus offset; otherwise the relayed player id.
static TARGET: AtomicU32 = AtomicU32::new(0);
/// Older relayed samples are not drawn (no extrapolation yet).
const REMOTE_FRESH: std::time::Duration = std::time::Duration::from_secs(1);
fn word(at: u32) -> Result<u32, ()> {
    let mut b = [0; 4];
    read(at, &mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn floats<const N: usize>(at: u32) -> Result<[f32; N], ()> {
    let mut bytes = [0u8; 64];
    read(at, &mut bytes[..N * 4])?;
    let mut out = [0f32; N];
    for (o, v) in out.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *o = f32::from_le_bytes(*v);
    }
    out.iter().all(|v| v.is_finite()).then_some(out).ok_or(())
}
/// Allocated and therefore not removable with the probes that keep it safe.
pub(crate) fn allocated() -> bool {
    STATE.load(Ordering::Acquire) == 1
}
pub(crate) fn shown() -> bool {
    SHOWN.load(Ordering::Acquire)
}
/// Called by the draw probe after the original returned, on its owner thread,
/// outside nesting. Mirrors the exterior branch: skips interior (4) and the
/// early-return states (9, 12) observed statically and in the match.
fn player_pose(this: u32) -> Option<([f32; 4], [f32; 3])> {
    word(this.wrapping_add(0x1d0))
        .and_then(|b| word(b.wrapping_add(0x3e4)))
        .and_then(|s| {
            Ok((
                floats::<4>(s.wrapping_add(0x14))?,
                floats::<3>(s.wrapping_add(0x30))?,
            ))
        })
        .ok()
}
/// TARGET value that draws every fresh relayed player with the same instance.
pub const ALL_PLAYERS: u32 = u32::MAX;
/// Upper bound on remote draws per frame.
const MAX_REMOTE_DRAWS: usize = 16;
/// Interpolated poses of the selected remote(s), if fresh. Never blocks the
/// render thread: a contended lock skips this frame.
fn remote_poses(target: u32, out: &mut [([f32; 4], [f32; 3]); MAX_REMOTE_DRAWS]) -> Option<usize> {
    let remotes = crate::REMOTES.try_lock().ok()?;
    let now = crate::interpolation::now();
    let mut count = 0;
    for (id, entry) in remotes.iter() {
        if count == out.len() {
            break;
        }
        if (target != ALL_PLAYERS && *id != u64::from(target))
            || entry.seen.elapsed() >= REMOTE_FRESH
        {
            continue;
        }
        if let Some((position, rotation)) = entry.history.pose(now) {
            out[count] = (rotation, position);
            count += 1;
        }
    }
    Some(count)
}
pub(crate) fn after_car_draw(this: u32, [world, _, path, viewport]: [u32; 4]) {
    // Publish the local pose for the Drawn network source; same coordinates as drawing.
    if let Some((q, p)) = player_pose(this) {
        let rotation = Rotation {
            x: q[0],
            y: q[1],
            z: q[2],
            w: q[3],
        };
        if rotation.is_valid()
            && let Ok(mut local) = crate::LOCAL_POSE.try_lock()
        {
            *local = Some((
                Position {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                },
                rotation,
                std::time::Instant::now(),
            ));
        }
    }
    if !SHOWN.load(Ordering::Acquire) {
        return;
    }
    let skip = || {
        SKIPPED.fetch_add(1, Ordering::Relaxed);
    };
    let mut state = [0];
    if read(this.wrapping_add(0x1e9), &mut state).is_err() || matches!(state[0], 4 | 9 | 12) {
        return skip();
    }
    let Ok(top) = word(world) else {
        return skip();
    };
    // Validated context: exterior draw at world depth 0 with a live path/viewport.
    let live = |at: u32| word(at).is_ok_and(|p| p != 0);
    if observation::depth(world, top) != Some(0)
        || !live(path)
        || !live(path.wrapping_sub(8))
        || !live(viewport)
    {
        return skip();
    }
    let target = TARGET.load(Ordering::Acquire);
    let mut poses = [([0f32; 4], [0f32; 3]); MAX_REMOTE_DRAWS];
    let count = if target == 0 {
        player_pose(this).map(|pose| {
            poses[0] = pose;
            1
        })
    } else {
        remote_poses(target, &mut poses)
    };
    let Some(count @ 1..) = count else {
        return skip();
    };
    let f = |i: usize| FUNCTIONS[i].load(Ordering::Acquire);
    let error = unsafe { GetLastError() };
    for (quaternion, mut position) in poses.into_iter().take(count) {
        for (p, o) in position.iter_mut().zip(&OFFSET) {
            *p += f32::from_bits(o.load(Ordering::Acquire));
        }
        // SAFETY: profiled cdecl exports checked at creation; same thread and
        // phase as the game's own exterior draw; one balanced push/pop at depth
        // 0. The game itself draws one wheel instance several times per frame.
        unsafe {
            std::mem::transmute::<usize, Plain>(f(1))();
            std::mem::transmute::<usize, Vector>(f(3))(position.as_ptr());
            std::mem::transmute::<usize, Vector>(f(4))(quaternion.as_ptr());
            std::mem::transmute::<usize, Draw>(f(0))(
                ITEM.load(Ordering::Acquire) as *mut c_void,
                1,
                u32::MAX,
            );
            std::mem::transmute::<usize, Plain>(f(2))();
        }
        DRAWN.fetch_add(1, Ordering::Relaxed);
        if word(world) != Ok(top) {
            // Cannot repair an engine stack; stop drawing and keep the instance.
            FAULT.store(1, Ordering::Release);
            SHOWN.store(false, Ordering::Release);
            HIDDEN_AT.store(flush_calls(), Ordering::Release);
            break;
        }
    }
    unsafe { SetLastError(error) };
    LAST_FLUSH.store(flush_calls(), Ordering::Release);
}
fn draw_functions(
    process: &Process,
    block: &EngineControl,
    fixture: bool,
) -> Result<[u32; 5], String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    if fixture {
        let host = get("engine_fixture.exe")?;
        let functions: [u32; 5] = std::array::from_fn(|i| block.frames[i]);
        for f in functions {
            executable(f, host)?;
        }
        return Ok(functions);
    }
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
    let game = get("game.dll")?;
    let mut out = [0; 5];
    for (i, (module, name, rva, slot)) in [
        ("psm.dll", "psm_draw", 0x6a10, 0x160ba4),
        ("prism3d.exe", "gfx_push", 0x2de0, 0x16104c),
        ("prism3d.exe", "gfx_pop", 0x2eb0, 0x160a98),
        ("prism3d.exe", "gfx_translate", 0x30e0, 0x160fd8),
        ("prism3d.exe", "gfx_qrotate", 0x3140, 0x160fe4),
    ]
    .into_iter()
    .enumerate()
    {
        let owner = get(module)?;
        let expected = add(owner, rva)?;
        executable(expected, owner)?;
        if pe_exports::resolve(&images, &owner.name, name, |a, b| {
            process.read(a, b).map_err(|e| e.to_string())
        })? != u64::from(expected)
            || value(process, add(game, slot)?)? != expected
        {
            return Err(format!("{name} export/game slot differs from profile"));
        }
        out[i] = expected;
    }
    Ok(out)
}
fn set_offset(block: &EngineControl) -> Result<(), String> {
    let values = [block.count_before, block.count_created, block.count_after].map(f32::from_bits);
    if values.iter().any(|v| !v.is_finite() || v.abs() > 10_000.0) {
        return Err("visual offset must be finite and within 100 m".into());
    }
    for (o, v) in OFFSET.iter().zip(values) {
        o.store(v.to_bits(), Ordering::Release);
    }
    Ok(())
}
/// Loop-queue task: create (once) and show, or release after settling.
pub fn run(mut block: EngineControl, fixture: bool) -> Result<EngineControl, String> {
    block.executed_thread = unsafe { GetCurrentThreadId() };
    if block.command == api::VISUAL_RELEASE {
        release()?;
    } else {
        if fixture != (block.command == api::VISUAL_SHOW_FIXTURE) {
            return Err("visual mode does not match the attached pump".into());
        }
        if crate::engine_draw::mode() != Some(fixture) {
            return Err("attach the draw observer in the same mode before showing".into());
        }
        if !crate::engine_render::installed() {
            return Err("attach the render observer; release needs its flush count".into());
        }
        if FAULT.load(Ordering::Acquire) != 0 {
            return Err("visual faulted; release it and restart the process".into());
        }
        set_offset(&block)?;
        TARGET.store(block.entity, Ordering::Release);
        if STATE.load(Ordering::Acquire) != 1 {
            create(&block, fixture)?;
        }
        SHOWN.store(true, Ordering::Release);
    }
    fill(&mut block);
    Ok(block)
}
fn create(block: &EngineControl, fixture: bool) -> Result<(), String> {
    let path = api::text(&block.name)?;
    let model = ModelName::parse(path)?;
    if !fixture && path != "models/ai/truck/models/truck.psm" {
        return Err("test visual currently restricted to the profiled truck model".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let functions = draw_functions(&process, block, fixture)?;
    let (alloc_info, release_info, _) = info::bindings(&process, block, fixture)?;
    let (allocate, free, ops) = crate::engine_instance::bindings(&process, block, fixture)?;
    let pointer = unsafe { alloc_info(model.as_cstr().as_ptr()) };
    if pointer.is_null() {
        return Err("PSM info allocation returned null".into());
    }
    let descriptor = Info {
        pointer,
        release: release_info,
    };
    let at = pointer as u32;
    if value(&process, at)? != 1 || value(&process, offset(at, 4)?)? == 0 {
        return Err("PSM info header differs from contract".into());
    }
    // Profiled cdecl: borrowed held info, no skin overrides, one slot of capacity.
    let item = unsafe { allocate(pointer, ptr::null(), 0, 1) };
    if item.is_null() {
        return Err("PSM instance allocation returned null".into());
    }
    if let Err(e) = crate::engine_instance::check_instance(&process, item as u32, at, ops) {
        unsafe { free(item) };
        return Err(e);
    }
    for (slot, v) in FUNCTIONS.iter().zip(
        functions
            .into_iter()
            .chain([free as usize as u32, release_info as usize as u32]),
    ) {
        slot.store(v as usize, Ordering::Release);
    }
    ITEM.store(item as usize, Ordering::Release);
    INFO.store(pointer as usize, Ordering::Release);
    std::mem::forget(descriptor); // Ownership moves to the retained visual.
    DRAWN.store(0, Ordering::Release);
    SKIPPED.store(0, Ordering::Release);
    STATE.store(1, Ordering::Release);
    Ok(())
}
fn release() -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 1 {
        return Err("no test visual allocated".into());
    }
    if SHOWN.load(Ordering::Acquire) {
        return Err("hide the visual before releasing it".into());
    }
    let settled = HIDDEN_AT
        .load(Ordering::Acquire)
        .max(LAST_FLUSH.load(Ordering::Acquire));
    if DRAWN.load(Ordering::Acquire) != 0 && flush_calls().wrapping_sub(settled) < SETTLE_FLUSHES {
        return Err("queued draws not yet flushed; retry after more frames".into());
    }
    let item = ITEM.load(Ordering::Acquire) as u32;
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    // Strong/residency/access counters back to the allocation contract [1,1,0].
    let counters = [0x4c, 0x50, 0x58].map(|o| offset(item, o).and_then(|a| value(&process, a)));
    if counters.iter().any(|c| c.is_err()) {
        return Err("visual item unreadable; retained".into());
    }
    let counters = counters.map(|c| c.unwrap_or(0));
    if counters != [1, 1, 0] {
        return Err(format!(
            "visual item still referenced {counters:?}; retry after more frames"
        ));
    }
    let f = |i: usize| FUNCTIONS[i].load(Ordering::Acquire);
    unsafe {
        std::mem::transmute::<usize, info::Release>(f(5))(item as usize as *mut c_void);
        std::mem::transmute::<usize, info::Release>(f(6))(
            INFO.load(Ordering::Acquire) as *mut c_void
        );
    }
    ITEM.store(0, Ordering::Release);
    INFO.store(0, Ordering::Release);
    STATE.store(0, Ordering::Release);
    Ok(())
}
fn fill(b: &mut EngineControl) {
    let item = ITEM.load(Ordering::Acquire) as u32;
    let access = if item == 0 {
        0
    } else {
        word(item + 0x58).unwrap_or(u32::MAX)
    };
    Visual {
        state: STATE.load(Ordering::Acquire),
        shown: u32::from(SHOWN.load(Ordering::Acquire)),
        fault: FAULT.load(Ordering::Acquire),
        item,
        drawn: DRAWN.load(Ordering::Acquire),
        skipped: SKIPPED.load(Ordering::Acquire),
        access,
        last_draw_flush: LAST_FLUSH.load(Ordering::Acquire),
        flushes: flush_calls(),
        offset: OFFSET
            .each_ref()
            .map(|o| f32::from_bits(o.load(Ordering::Acquire))),
        target: TARGET.load(Ordering::Acquire),
    }
    .encode(b);
}
/// Remote-thread commands: hide (atomic only) and status.
pub fn control(b: &mut EngineControl) -> Result<(), String> {
    if b.command == api::VISUAL_HIDE && SHOWN.swap(false, Ordering::AcqRel) {
        HIDDEN_AT.store(flush_calls(), Ordering::Release);
    }
    fill(b);
    Ok(())
}

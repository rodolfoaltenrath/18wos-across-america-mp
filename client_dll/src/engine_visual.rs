//! One test visual: a retained private PSM instance drawn right after the
//! player's actors.car draw, at a fixed world offset. No physics, collision,
//! HUD, entity or world insertion. Create/release run only on the loop queue;
//! drawing runs only inside the observed draw on the same window thread.
use crate::{
    api::{self, EngineControl},
    draw_observation::{self as observation, Visual},
    engine_info::{self as info, Info, offset, value},
    engine_lifecycle::{add, executable, executable_memory},
    engine_model::ModelName,
    engine_render::{flush_calls, read},
    inspection::process::Process,
    pe_exports::{self, Image},
    release_policy::{self, Evidence, Refusal, Report},
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
/// psm_set_shader(item, part, path): mm_precache + set_shader_item, which
/// releases the part's previous shader and consumes the new reference.
type SetShader = unsafe extern "C" fn(*mut c_void, u32, *const std::ffi::c_char) -> *mut c_void;
type LightCallback = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type SetLight = unsafe extern "C" fn(*mut c_void, Option<LightCallback>, *mut c_void);
static SET_LIGHT: AtomicUsize = AtomicUsize::new(0);
static GET_LIGHT: AtomicUsize = AtomicUsize::new(0);
/// Getter targets must belong to the profiled game/engine (or the own fixture).
static LIGHT_RANGES: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static STATE: AtomicU32 = AtomicU32::new(0); // 0 none, 1 allocated
static SHOWN: AtomicBool = AtomicBool::new(false);
static FAULT: AtomicU32 = AtomicU32::new(0);
static ITEM: AtomicUsize = AtomicUsize::new(0);
static INFO: AtomicUsize = AtomicUsize::new(0);
/// psm_draw, gfx_push, gfx_pop, gfx_translate, gfx_qrotate, psm_free, info release.
static FUNCTIONS: [AtomicUsize; 7] = [const { AtomicUsize::new(0) }; 7];
static OFFSET: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static DRAWN: AtomicU32 = AtomicU32::new(0);
static EVER_DRAWN: AtomicBool = AtomicBool::new(false);
static SKIPPED: AtomicU32 = AtomicU32::new(0);
static LAST_FLUSH: AtomicU32 = AtomicU32::new(0);
static HIDDEN_AT: AtomicU32 = AtomicU32::new(0);
static SKINNED: AtomicU32 = AtomicU32::new(0);
/// Player entity seen by the last draw; a change means the game reloaded it.
static LAST_THIS: AtomicU32 = AtomicU32::new(0);
/// Hidden after an entity change; the loop recreates it once release is safe.
static RECREATE: AtomicBool = AtomicBool::new(false);
static RECREATED: AtomicU32 = AtomicU32::new(0);
/// Distinct player entities seen by the draw: the load gate's world epoch.
static ACTOR_EPOCH: AtomicU32 = AtomicU32::new(0);
/// Draw cost (ns, EMA and max) around our engine calls, and exterior frame
/// interval (us, EMA) between local exterior draws. Diagnostics only.
static COST_AVG: AtomicU32 = AtomicU32::new(0);
static COST_MAX: AtomicU32 = AtomicU32::new(0);
static FRAME_AVG: AtomicU32 = AtomicU32::new(0);
static LAST_FRAME: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
/// Exponential moving average with weight 1/16, seeded by the first sample.
fn ema(slot: &AtomicU32, sample: u32) {
    let old = slot.load(Ordering::Relaxed);
    let next = if old == 0 {
        sample
    } else {
        old - old / 16 + sample / 16
    };
    slot.store(next.max(1), Ordering::Relaxed);
}
/// Show block used for creation, replayed on recreation (model, skin, mode).
static CREATION: std::sync::Mutex<Option<EngineControl>> = std::sync::Mutex::new(None);
/// 0 = local player plus offset; otherwise the relayed player id.
static TARGET: AtomicU32 = AtomicU32::new(0);
/// Older relayed samples are not drawn, even after limited extrapolation.
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
    let previous = LAST_THIS.swap(this, Ordering::AcqRel);
    if previous != this {
        ACTOR_EPOCH.fetch_add(1, Ordering::AcqRel);
    }
    if previous != 0 && previous != this && SHOWN.swap(false, Ordering::AcqRel) {
        // Reloaded player entity: stop drawing now; maintain() recreates later.
        HIDDEN_AT.store(flush_calls(), Ordering::Release);
        RECREATE.store(true, Ordering::Release);
    }
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
    if let Ok(mut last) = LAST_FRAME.try_lock() {
        let now = std::time::Instant::now();
        if let Some(previous) = last.replace(now) {
            let micros = now.duration_since(previous).as_micros();
            if micros < 1_000_000 {
                ema(&FRAME_AVG, micros as u32);
            }
        }
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
    let light_context = if SKINNED.load(Ordering::Acquire) != 0 {
        let scene = word(this.wrapping_add(0x84)).ok().filter(|p| *p != 0);
        let getter = scene
            .and_then(|s| word(s).ok())
            .and_then(|v| v.checked_add(0x48))
            .and_then(|a| word(a).ok());
        let ranges = LIGHT_RANGES.each_ref().map(|r| r.load(Ordering::Acquire));
        if !getter.is_some_and(|f| {
            ranges
                .as_chunks::<2>()
                .0
                .iter()
                .any(|r| (r[0]..r[1]).contains(&f))
                && executable_memory(f).is_ok()
        }) {
            return skip();
        }
        scene.unwrap_or(0)
    } else {
        0
    };
    let started = std::time::Instant::now();
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
            let set_light: SetLight = std::mem::transmute(SET_LIGHT.load(Ordering::Acquire));
            if light_context != 0 {
                // Borrow only for this synchronous draw, like game+0x641c0.
                set_light(
                    ITEM.load(Ordering::Acquire) as *mut c_void,
                    Some(std::mem::transmute::<usize, LightCallback>(
                        GET_LIGHT.load(Ordering::Acquire),
                    )),
                    light_context as *mut c_void,
                );
            }
            std::mem::transmute::<usize, Plain>(f(1))();
            std::mem::transmute::<usize, Vector>(f(3))(position.as_ptr());
            std::mem::transmute::<usize, Vector>(f(4))(quaternion.as_ptr());
            std::mem::transmute::<usize, Draw>(f(0))(
                ITEM.load(Ordering::Acquire) as *mut c_void,
                1,
                u32::MAX,
            );
            std::mem::transmute::<usize, Plain>(f(2))();
            if light_context != 0 {
                // No scene pointer survives between frames or a reload.
                set_light(
                    ITEM.load(Ordering::Acquire) as *mut c_void,
                    None,
                    ptr::null_mut(),
                );
            }
        }
        DRAWN.fetch_add(1, Ordering::Relaxed);
        EVER_DRAWN.store(true, Ordering::Release);
        if word(world) != Ok(top) {
            // Cannot repair an engine stack; stop drawing and keep the instance.
            FAULT.store(1, Ordering::Release);
            SHOWN.store(false, Ordering::Release);
            HIDDEN_AT.store(flush_calls(), Ordering::Release);
            break;
        }
    }
    let cost = u32::try_from(started.elapsed().as_nanos()).unwrap_or(u32::MAX);
    ema(&COST_AVG, cost);
    COST_MAX.fetch_max(cost, Ordering::Relaxed);
    unsafe { SetLastError(error) };
    LAST_FLUSH.store(flush_calls(), Ordering::Release);
}
fn draw_functions(
    process: &Process,
    block: &EngineControl,
    fixture: bool,
) -> Result<[u32; 8], String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    if fixture {
        let host = get("engine_fixture.exe")?;
        let functions: [u32; 8] = std::array::from_fn(|i| block.frames[i]);
        for f in functions {
            executable(f, host)?;
        }
        for (slot, v) in LIGHT_RANGES
            .iter()
            .zip([host.base as u32, (host.base + host.size) as u32].repeat(2))
        {
            slot.store(v, Ordering::Release);
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
    let mut out = [0; 8];
    for (i, (module, name, rva, slot)) in [
        ("psm.dll", "psm_draw", 0x6a10, 0x160ba4),
        ("prism3d.exe", "gfx_push", 0x2de0, 0x16104c),
        ("prism3d.exe", "gfx_pop", 0x2eb0, 0x160a98),
        ("prism3d.exe", "gfx_translate", 0x30e0, 0x160fd8),
        ("prism3d.exe", "gfx_qrotate", 0x3140, 0x160fe4),
        ("psm.dll", "psm_set_shader", 0x3a60, 0x16102c),
        ("psm.dll", "psm_set_light_callback", 0x38c0, 0x160de0),
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
    out[7] = add(game, 0x641c0)?;
    executable(out[7], game)?;
    let engine = get("p3shared.dll")?;
    for (slot, v) in LIGHT_RANGES.iter().zip([
        game.base as u32,
        (game.base + game.size) as u32,
        engine.base as u32,
        (engine.base + engine.size) as u32,
    ]) {
        slot.store(v, Ordering::Release);
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
    if block.command == api::VISUAL_CHECK {
        release_report().encode(&mut block);
        return Ok(block);
    }
    if block.command == api::VISUAL_RELEASE {
        release().map_err(|r| r.to_string())?;
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
        if crate::engine_load_gate::busy() {
            return Err("deferred load in progress; the gate recreates the visual".into());
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
        RECREATE.store(false, Ordering::Release);
        if let Ok(mut creation) = CREATION.lock() {
            *creation = Some(block);
        }
    }
    fill(&mut block);
    Ok(block)
}
fn create(block: &EngineControl, fixture: bool) -> Result<(), String> {
    if crate::engine_transition::installed() {
        return Err(
            "passive transition observation requires no retained visual; remove it first".into(),
        );
    }
    let path = api::text(&block.name)?;
    let model = ModelName::parse(path)?;
    if !fixture && path != "models/ai/truck/models/truck.psm" {
        return Err("test visual currently restricted to the profiled truck model".into());
    }
    // Optional skin in `message` (input only; errors overwrite it on return).
    let shader = api::text(&block.message)?;
    let shader = (!shader.is_empty())
        .then(|| {
            if !fixture && shader != "models/ai/truck/shaders/truck.shd" {
                return Err("test visual skin restricted to the AI truck shader".to_string());
            }
            ModelName::shader(shader)
        })
        .transpose()?;
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
    let mut skinned = 0;
    if let Some(shader) = &shader {
        let set: SetShader = unsafe { std::mem::transmute(functions[5] as usize) };
        let parts = value(&process, item as u32 + 8).and_then(|data| value(&process, data + 4));
        let result = parts.and_then(|parts| {
            for part in 0..parts.min(256) {
                // SAFETY: profiled cdecl export; part < model part count; owned CString.
                if unsafe { set(item, part, shader.as_cstr().as_ptr()) }.is_null() {
                    return Err(format!("psm_set_shader failed on part {part}"));
                }
                skinned += 1;
            }
            Ok(())
        });
        if let Err(e) = result {
            // Never drawn yet: freeing releases every shader already set.
            unsafe { free(item) };
            return Err(e);
        }
    }
    for (slot, v) in FUNCTIONS.iter().zip(
        functions[..5]
            .iter()
            .copied()
            .chain([free as usize as u32, release_info as usize as u32]),
    ) {
        slot.store(v as usize, Ordering::Release);
    }
    ITEM.store(item as usize, Ordering::Release);
    INFO.store(pointer as usize, Ordering::Release);
    SKINNED.store(skinned, Ordering::Release);
    SET_LIGHT.store(functions[6] as usize, Ordering::Release);
    GET_LIGHT.store(functions[7] as usize, Ordering::Release);
    std::mem::forget(descriptor); // Ownership moves to the retained visual.
    DRAWN.store(0, Ordering::Release);
    EVER_DRAWN.store(false, Ordering::Release);
    SKIPPED.store(0, Ordering::Release);
    STATE.store(1, Ordering::Release);
    Ok(())
}
pub(crate) fn release_report() -> Report {
    let item = ITEM.load(Ordering::Acquire) as u32;
    let allocated = STATE.load(Ordering::Acquire) == 1;
    let counters = (allocated && item != 0)
        .then(|| [0x4c, 0x50, 0x58].map(|o| item.checked_add(o).ok_or(()).and_then(word)))
        .and_then(|values| match values {
            [Ok(strong), Ok(residency), Ok(access)] => Some([strong, residency, access]),
            _ => None,
        });
    release_policy::assess(
        crate::engine_phase::release_context(),
        Evidence {
            allocated,
            shown: SHOWN.load(Ordering::Acquire),
            item,
            drawn: DRAWN.load(Ordering::Acquire),
            ever_drawn: EVER_DRAWN.load(Ordering::Acquire),
            flushes: flush_calls(),
            last_draw_flush: LAST_FLUSH.load(Ordering::Acquire),
            hidden_at: HIDDEN_AT.load(Ordering::Acquire),
            counters,
            bindings_present: INFO.load(Ordering::Acquire) != 0
                && FUNCTIONS[5].load(Ordering::Acquire) != 0
                && FUNCTIONS[6].load(Ordering::Acquire) != 0,
        },
    )
}
fn release() -> Result<(), Refusal> {
    let report = release_report();
    if let Some(reason) = report.refusal {
        return Err(reason);
    }
    let item = report.evidence.item;
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
        skinned_parts: SKINNED.load(Ordering::Acquire),
        recreated: RECREATED.load(Ordering::Acquire),
        recreate_pending: u32::from(RECREATE.load(Ordering::Acquire)),
        draw_cost_avg_ns: COST_AVG.load(Ordering::Relaxed),
        draw_cost_max_ns: COST_MAX.load(Ordering::Relaxed),
        frame_interval_avg_us: FRAME_AVG.load(Ordering::Relaxed),
    }
    .encode(b);
}
/// Remote-thread commands: hide (atomic only) and status.
pub fn control(b: &mut EngineControl) -> Result<(), String> {
    if b.command == api::VISUAL_HIDE {
        // An explicit hide also cancels a pending automatic recreation.
        RECREATE.store(false, Ordering::Release);
        if SHOWN.swap(false, Ordering::AcqRel) {
            HIDDEN_AT.store(flush_calls(), Ordering::Release);
        }
    }
    fill(b);
    Ok(())
}
/// Loop-thread maintenance after each pump pass: once a reload-triggered hide
/// has settled, release and recreate with the original show parameters.
pub(crate) fn maintain(fixture: bool) {
    if crate::engine_load_gate::busy() {
        return;
    }
    if !RECREATE.load(Ordering::Acquire) {
        return;
    }
    if STATE.load(Ordering::Acquire) != 1 || crate::engine_draw::mode() != Some(fixture) {
        RECREATE.store(false, Ordering::Release);
        return;
    }
    if let Err(e) = release() {
        if !e.retryable() {
            // Unexpected refusal: keep the instance hidden and stop automating.
            FAULT.store(2, Ordering::Release);
            RECREATE.store(false, Ordering::Release);
        }
        return;
    }
    let block = CREATION.lock().ok().and_then(|c| *c);
    match block.map(|b| create(&b, fixture)) {
        Some(Ok(())) => {
            RECREATED.fetch_add(1, Ordering::AcqRel);
            LAST_THIS.store(0, Ordering::Release);
            SHOWN.store(true, Ordering::Release);
        }
        _ => FAULT.store(2, Ordering::Release),
    }
    RECREATE.store(false, Ordering::Release);
}
/// Last drawn player entity and how many distinct ones the draw has seen.
pub(crate) fn witness() -> (u32, u32) {
    (
        LAST_THIS.load(Ordering::Acquire),
        ACTOR_EPOCH.load(Ordering::Acquire),
    )
}
/// Stop drawing for a deferred load (atomic, any phase) and cancel a pending
/// automatic recreation. True when the gate should recreate it afterwards.
pub(crate) fn suspend_for_load() -> bool {
    let shown = SHOWN.swap(false, Ordering::AcqRel);
    if shown {
        HIDDEN_AT.store(flush_calls(), Ordering::Release);
    }
    let pending = RECREATE.swap(false, Ordering::AcqRel);
    (shown || pending) && CREATION.lock().is_ok_and(|c| c.is_some())
}
/// Loop-queue release for the gate; revalidates the whole policy first.
pub(crate) fn release_for_load() -> Result<(), Refusal> {
    release()
}
/// Loop-queue recreation in the NEW world with the original show block.
pub(crate) fn recreate_for_load(fixture: bool) -> Result<(), String> {
    if FAULT.load(Ordering::Acquire) != 0 {
        return Err("visual faulted; not recreated".into());
    }
    if STATE.load(Ordering::Acquire) == 1 {
        return Err("visual still allocated; not recreated".into());
    }
    let block = CREATION
        .lock()
        .ok()
        .and_then(|c| *c)
        .ok_or("no show parameters to recreate")?;
    create(&block, fixture)?;
    RECREATED.fetch_add(1, Ordering::AcqRel);
    SHOWN.store(true, Ordering::Release);
    Ok(())
}

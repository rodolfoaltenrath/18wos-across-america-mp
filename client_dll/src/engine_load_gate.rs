//! ACTIVE world-command gate on the profiled descriptor words (`descriptor+4`).
//!
//! The shared game handler (`game+0xd3570`, entered through the thunk
//! `+0xd2e90`) deletes the scene for `map`, `preview`, `edit`, `exit`, `menu`
//! and `quit`; `map` even builds the new world synchronously with no flush in
//! between, so the retained visual cannot be released from the callback.
//! While the visual is allocated, the hook classifies argv[0], copies argc/argv,
//! swallows the call and admits one `deferred_load::Gate` job. The outer loop
//! (PeekMessageA pump, depth 1) hides, waits for the release policy, frees,
//! then replays exactly what `cmd_execute` does (`prism3d+0xe010..0xe01c`):
//! publish argc and an argv array in the core globals and call the ORIGINAL
//! callback once. After `map` the visual is recreated when a new player entity
//! is drawn; after the other commands (no player world follows) the job ends on
//! return and the visual is restored on the next new player entity, except
//! after `quit`.
//!
//! Without a retained visual every call is forwarded unchanged. Arguments are
//! never logged; replayed argv frames stay allocated for the process lifetime
//! because no proof exists that the game drops pointers into them.
use crate::{
    api::{self, EngineControl},
    deferred_load::{Ack, Action, ActionKind, Gate, OwnedCommand, Phase, World},
    engine_lifecycle::{add, executable},
    engine_render::read,
    hook_transaction::{self, Memory, Patch},
    inspection::process::Process,
    load_gate::{self, Entry, EntryState, Note, Status, WORLD_COMMANDS, WorldCommand},
    profile,
};
use std::{
    ffi::{CString, c_void},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    System::{
        Memory::{PAGE_READWRITE, VirtualProtect},
        Threading::{GetCurrentProcessId, GetCurrentThreadId},
    },
};
type Callback = unsafe extern "C" fn() -> u32;
const MAX_SLOTS: usize = WORLD_COMMANDS.len();
static STATE: AtomicU32 = AtomicU32::new(0); // 0 never, 1 installed, 2 removed
static OWNER: AtomicU32 = AtomicU32::new(0);
static FIXTURE: AtomicBool = AtomicBool::new(false);
static TIMEOUT: AtomicU32 = AtomicU32::new(load_gate::DEFAULT_TIMEOUT_MS);
/// Hooked `descriptor+4` words; all share the profiled thunk as original.
static SLOTS: [AtomicUsize; MAX_SLOTS] = [const { AtomicUsize::new(0) }; MAX_SLOTS];
static SLOT_COUNT: AtomicU32 = AtomicU32::new(0);
/// Retained until callbacks AND temporary page protections are restored.
static PATCHES: Mutex<Vec<Patch>> = Mutex::new(Vec::new());
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// argc word, argv word and current-handler word. Never functions to call.
static BINDINGS: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
static REPLAYING: AtomicBool = AtomicBool::new(false);
/// Hook invocations still inside this module, including forwarded originals.
static FLIGHT: AtomicU32 = AtomicU32::new(0);
/// A request was dropped with the visual retained: release it, do not load.
static FALLBACK: AtomicBool = AtomicBool::new(false);
static GATE: Mutex<Option<Gate>> = Mutex::new(None);
static CLOCK: OnceLock<Instant> = OnceLock::new();
/// Replayed argv frames: the game may keep pointers to the save name.
static RETAINED: Mutex<Vec<Frame>> = Mutex::new(Vec::new());
/// Current job: visual shown at admission, and whether a player world follows.
static JOB_RESTORE: AtomicBool = AtomicBool::new(false);
static JOB_AWAITS: AtomicBool = AtomicBool::new(true);
/// Restore after a non-player command, once the draw epoch passes the stamp.
static RESTORE_PENDING: AtomicBool = AtomicBool::new(false);
static RESTORE_EPOCH: AtomicU32 = AtomicU32::new(0);
// Published snapshot of the gate for lock-free hook decisions and status.
static JOB_ACTIVE: AtomicBool = AtomicBool::new(false);
static PHASE: AtomicU32 = AtomicU32::new(0);
static JOB: AtomicU32 = AtomicU32::new(0);
static FLAGS: AtomicU32 = AtomicU32::new(0);
static REASON: AtomicU32 = AtomicU32::new(0);
static LAST_NOTE: AtomicU32 = AtomicU32::new(0);
static LAST_COMMAND: AtomicU32 = AtomicU32::new(0);
static LAST_RETURN: AtomicU32 = AtomicU32::new(0);
static WITNESS: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
#[derive(Clone, Copy)]
enum Count {
    Calls,
    Deferred,
    Forwarded,
    Foreign,
    ReplayNested,
    SwallowedBusy,
    Dropped,
    Replays,
    ArgumentsRestored,
    ArgumentsRetained,
    Recreated,
    FallbackReleases,
}
static COUNTS: [AtomicU32; 12] = [const { AtomicU32::new(0) }; 12];
fn bump(c: Count) {
    COUNTS[c as usize].fetch_add(1, Ordering::AcqRel);
}
fn count(c: Count) -> u32 {
    COUNTS[c as usize].load(Ordering::Acquire)
}
fn now() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64
}
fn word(at: u32) -> Result<u32, ()> {
    let mut b = [0; 4];
    read(at, &mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn store(at: u32, value: u32) {
    // SAFETY: argc/argv words validated at install as aligned, readable and
    // distinct; the core writes them the same way inside cmd_execute.
    unsafe { AtomicU32::from_ptr(at as *mut u32) }.store(value, Ordering::Release);
}
/// argv[0] as published by cmd_execute; at most 15 bytes plus NUL are read.
fn current_command() -> Option<WorldCommand> {
    let argv = word(BINDINGS[1].load(Ordering::Acquire)).ok()?;
    let first = word(argv).ok().filter(|p| *p != 0)?;
    let mut name = [0u8; 16];
    for (i, byte) in name.iter_mut().enumerate() {
        read(first.checked_add(i as u32)?, std::slice::from_mut(byte)).ok()?;
        if *byte == 0 {
            return WorldCommand::classify(&name[..i]);
        }
    }
    None
}
/// Owned argc/argv frame published while the original runs.
struct Frame {
    _values: Vec<CString>,
    pointers: Box<[u32]>,
}
impl Frame {
    fn new(command: &OwnedCommand) -> Self {
        let argc = command.with_argv(|argc, _| argc) as usize;
        let values: Vec<CString> = (0..argc)
            .map(|i| CString::new(command.argument(i).unwrap_or_default()).unwrap_or_default())
            .collect();
        let pointers = values.iter().map(|v| v.as_ptr() as u32).collect();
        Self {
            _values: values,
            pointers,
        }
    }
    fn argc(&self) -> u32 {
        self.pointers.len() as u32
    }
    fn argv(&self) -> u32 {
        self.pointers.as_ptr() as u32
    }
}
/// Draw-observed actor/epoch and the scene of the current command handler.
fn world() -> World {
    let (actor, epoch) = crate::engine_visual::witness();
    let scene = word(BINDINGS[2].load(Ordering::Acquire))
        .ok()
        .filter(|h| *h != 0)
        .and_then(|h| h.checked_add(0x14c))
        .and_then(|at| word(at).ok())
        .unwrap_or(0);
    for (slot, v) in WITNESS.iter().zip([actor, scene, epoch]) {
        slot.store(v, Ordering::Release);
    }
    World {
        epoch: u64::from(epoch),
        scene,
        actor,
        ready: actor != 0 && scene != 0,
    }
}
fn publish(gate: &Gate) {
    let s = gate.snapshot();
    PHASE.store(load_gate::phase_code(s.phase), Ordering::Release);
    JOB.store(s.id.unwrap_or(0) as u32, Ordering::Release);
    FLAGS.store(
        u32::from(s.in_flight)
            | u32::from(s.execute_started) << 1
            | u32::from(s.release_confirmed) << 2,
        Ordering::Release,
    );
    REASON.store(load_gate::reason_code(s.reason), Ordering::Release);
    JOB_ACTIVE.store(gate.active(), Ordering::Release);
}
/// True while the gate owns the visual's lifecycle (job, fallback or replay).
pub(crate) fn busy() -> bool {
    STATE.load(Ordering::Acquire) == 1
        && (JOB_ACTIVE.load(Ordering::Acquire)
            || FALLBACK.load(Ordering::Acquire)
            || REPLAYING.load(Ordering::Acquire))
}
fn original() -> Callback {
    unsafe { std::mem::transmute(ORIGINAL.load(Ordering::Acquire)) }
}
fn ours(slot: usize) -> bool {
    unsafe { AtomicUsize::from_ptr(slot as *mut usize) }.load(Ordering::Acquire)
        == gate_shim as *const () as usize
}
fn slots() -> impl Iterator<Item = usize> {
    SLOTS
        .iter()
        .take(SLOT_COUNT.load(Ordering::Acquire) as usize)
        .map(|s| s.load(Ordering::Acquire))
}
#[unsafe(naked)]
unsafe extern "C" fn gate_shim() -> u32 {
    core::arch::naked_asm!("push ebp","mov ebp,esp","and esp,-16","call {implementation}","mov esp,ebp","pop ebp","ret",implementation=sym gate_impl);
}
/// Profiled thunk ABI: cdecl, no arguments (it reads the handler global and
/// tail-jumps to the member). EAX is returned raw; swallowed calls return 0,
/// the value observed for a real map call, and cmd_execute ignores it.
unsafe extern "C" fn gate_impl() -> u32 {
    let incoming = unsafe { GetLastError() };
    FLIGHT.fetch_add(1, Ordering::AcqRel);
    bump(Count::Calls);
    let owner = unsafe { GetCurrentThreadId() } == OWNER.load(Ordering::Acquire);
    let (argc, command) = if owner {
        (
            word(BINDINGS[0].load(Ordering::Acquire)).unwrap_or(0),
            current_command(),
        )
    } else {
        (0, None)
    };
    let entry = load_gate::decide(EntryState {
        owner_thread: owner,
        replaying: REPLAYING.load(Ordering::Acquire),
        job_active: JOB_ACTIVE.load(Ordering::Acquire),
        command,
        argc,
        visual_allocated: crate::engine_visual::allocated(),
    });
    let note = match entry {
        Entry::Forward(note) | Entry::Swallow(note) => note,
        Entry::Defer => Note::None,
    };
    if owner {
        LAST_NOTE.store(note as u32, Ordering::Release);
    }
    let result = match entry {
        Entry::Forward(note) => {
            bump(match note {
                Note::Foreign => Count::Foreign,
                Note::Replay => Count::ReplayNested,
                _ => Count::Forwarded,
            });
            unsafe { SetLastError(incoming) };
            let returned = unsafe { original()() };
            let outgoing = unsafe { GetLastError() };
            FLIGHT.fetch_sub(1, Ordering::AcqRel);
            unsafe { SetLastError(outgoing) };
            return returned;
        }
        Entry::Swallow(_) => {
            bump(Count::SwallowedBusy);
            0
        }
        Entry::Defer => {
            let command = command.expect("deferred calls are classified");
            LAST_COMMAND.store(command as u32, Ordering::Release);
            // Stop drawing first: whatever happens next, no new queued draw.
            let restore = crate::engine_visual::suspend_for_load();
            match admit(command, argc, restore) {
                Ok(()) => {
                    FALLBACK.store(false, Ordering::Release);
                    bump(Count::Deferred);
                }
                Err(note) => {
                    LAST_NOTE.store(note as u32, Ordering::Release);
                    bump(Count::Dropped);
                    FALLBACK.store(true, Ordering::Release);
                }
            }
            0
        }
    };
    FLIGHT.fetch_sub(1, Ordering::AcqRel);
    unsafe { SetLastError(incoming) };
    result
}
fn admit(command: WorldCommand, argc: u32, restore: bool) -> Result<(), Note> {
    let argv = word(BINDINGS[1].load(Ordering::Acquire)).map_err(|_| Note::CaptureFailed)?;
    // Copies argv[0..argc] before cmd_execute's stack frame returns.
    let owned = OwnedCommand::capture_world(argc, argv, |at, out| {
        read(at, out).map_err(|_| String::new())
    })
    .map_err(|_| Note::CaptureFailed)?;
    if owned.name() != command.name() {
        return Err(Note::CaptureFailed);
    }
    let world = world();
    if !(world.epoch != 0 && world.ready) {
        return Err(Note::InvalidWorld);
    }
    let mut guard = GATE.try_lock().map_err(|_| Note::AdmitFailed)?;
    let gate = guard.as_mut().ok_or(Note::AdmitFailed)?;
    // A dropped job that hid the visual hands its restoration to this request.
    let inherited = gate
        .discard_terminal()
        .map_err(|_| Note::AdmitFailed)?
        .is_some_and(|(phase, restore)| {
            restore || (phase != Phase::Complete && JOB_RESTORE.load(Ordering::Acquire))
        });
    let restore = (restore || inherited || RESTORE_PENDING.swap(false, Ordering::AcqRel))
        && command.restorable();
    let awaits = command.awaits_player();
    let admitted = gate.admit_with(
        OWNER.load(Ordering::Acquire),
        owned,
        world,
        restore,
        awaits,
        now(),
        u64::from(TIMEOUT.load(Ordering::Acquire)),
    );
    if admitted.is_ok() {
        JOB_RESTORE.store(restore, Ordering::Release);
        JOB_AWAITS.store(awaits, Ordering::Release);
    }
    publish(gate);
    admitted.map(drop).map_err(|_| Note::AdmitFailed)
}
enum Step {
    Again,
    Stop,
    Execute(Action),
}
fn finish_complete(gate: &mut Gate) {
    let s = gate.snapshot();
    if s.phase == Phase::Complete
        && let Some(id) = s.id
        && gate.finish(id).is_ok()
        && !JOB_AWAITS.load(Ordering::Acquire)
        && JOB_RESTORE.swap(false, Ordering::AcqRel)
    {
        // No player world follows: restore on the next distinct actor.
        RESTORE_EPOCH.store(crate::engine_visual::witness().1, Ordering::Release);
        RESTORE_PENDING.store(true, Ordering::Release);
    }
}
fn step(gate: &mut Gate, fixture: bool) -> Step {
    let context = crate::engine_phase::release_context();
    let current = world();
    let action = match gate.poll(context, current, now()) {
        Ok(Some(action)) => action,
        _ => return Step::Stop,
    };
    let ack = match action.kind {
        ActionKind::Hide => {
            crate::engine_visual::suspend_for_load();
            Ack::Hidden
        }
        ActionKind::Check => Ack::Checked(crate::engine_visual::release_report()),
        ActionKind::Release => match crate::engine_visual::release_for_load() {
            Ok(()) => Ack::Released,
            Err(_) => Ack::ReleaseRefused,
        },
        ActionKind::Execute => {
            if gate
                .validate_execute(&action, context, current, now())
                .is_err()
            {
                Ack::NotInvoked
            } else {
                return Step::Execute(action);
            }
        }
        ActionKind::Recreate => match crate::engine_visual::recreate_for_load(fixture) {
            Ok(()) => {
                bump(Count::Recreated);
                Ack::Recreated
            }
            Err(_) => Ack::Unknown,
        },
    };
    let checked = action.kind == ActionKind::Check;
    if gate.acknowledge(action.token, ack, now()).is_err() {
        return Step::Stop;
    }
    let s = gate.snapshot();
    finish_complete(gate);
    // A retryable check waits for more flushes: one check per pump pass.
    if checked && s.phase == Phase::Check {
        Step::Stop
    } else {
        Step::Again
    }
}
/// Replay outside cmd_execute, at the outer loop, exactly once.
fn replay(command: &OwnedCommand) -> Ack {
    let hooked: Vec<usize> = slots().collect();
    if !hooked.iter().all(|s| ours(*s))
        || !matches!(word(BINDINGS[2].load(Ordering::Acquire)), Ok(h) if h != 0)
    {
        return Ack::NotInvoked;
    }
    let [argc_at, argv_at] = [0, 1].map(|i| BINDINGS[i].load(Ordering::Acquire));
    let (Ok(old_argc), Ok(old_argv)) = (word(argc_at), word(argv_at)) else {
        return Ack::NotInvoked;
    };
    let frame = Frame::new(command);
    store(argc_at, frame.argc());
    store(argv_at, frame.argv());
    REPLAYING.store(true, Ordering::Release);
    let error = unsafe { GetLastError() };
    let returned = unsafe { original()() };
    unsafe { SetLastError(error) };
    REPLAYING.store(false, Ordering::Release);
    bump(Count::Replays);
    LAST_RETURN.store(returned, Ordering::Release);
    if word(argc_at) == Ok(frame.argc()) && word(argv_at) == Ok(frame.argv()) {
        store(argc_at, old_argc);
        store(argv_at, old_argv);
        bump(Count::ArgumentsRestored);
    } else {
        // A nested command rewrote the globals: leave its values in place.
        bump(Count::ArgumentsRetained);
    }
    if let Ok(mut retained) = RETAINED.lock() {
        retained.push(frame);
    } else {
        std::mem::forget(frame);
    }
    if hooked.iter().all(|s| ours(*s)) {
        Ack::ExecuteReturned
    } else {
        // Another owner replaced a descriptor during the load: no recreation.
        Ack::Unknown
    }
}
/// After exit/menu/edit/preview: recreate once a new player entity is drawn.
fn restore_pending(fixture: bool) {
    if !RESTORE_PENDING.load(Ordering::Acquire) || JOB_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    if crate::engine_visual::allocated() {
        // Shown again by other means: nothing left to restore.
        RESTORE_PENDING.store(false, Ordering::Release);
        return;
    }
    let current = world();
    if !current.ready || current.epoch <= u64::from(RESTORE_EPOCH.load(Ordering::Acquire)) {
        return;
    }
    RESTORE_PENDING.store(false, Ordering::Release);
    if crate::engine_visual::recreate_for_load(fixture).is_ok() {
        bump(Count::Recreated);
    }
}
/// Pump pass at depth 1, before the visual's own maintenance.
pub(crate) fn maintain(fixture: bool) {
    if STATE.load(Ordering::Acquire) != 1 || REPLAYING.load(Ordering::Acquire) {
        return;
    }
    if FALLBACK.load(Ordering::Acquire) {
        match crate::engine_visual::release_for_load() {
            Ok(()) => {
                bump(Count::FallbackReleases);
                FALLBACK.store(false, Ordering::Release);
            }
            Err(r) if r.retryable() => {}
            // NotAllocated: nothing left; other refusals keep it hidden.
            Err(_) => FALLBACK.store(false, Ordering::Release),
        }
    }
    let Ok(mut guard) = GATE.try_lock() else {
        return;
    };
    let mut pending = None;
    if let Some(gate) = guard.as_mut() {
        for _ in 0..6 {
            match step(gate, fixture) {
                Step::Again => continue,
                Step::Stop => break,
                Step::Execute(action) => {
                    pending = Some(action);
                    break;
                }
            }
        }
        // AwaitWorld completes inside poll when no recreation was requested.
        finish_complete(gate);
        publish(gate);
    }
    let Some(action) = pending else {
        drop(guard);
        if !FALLBACK.load(Ordering::Acquire) {
            restore_pending(fixture);
        }
        return;
    };
    // The original may pump nested messages; no gate lock is held meanwhile.
    drop(guard);
    let ack = replay(action.command().expect("execute action owns its command"));
    if let Ok(mut guard) = GATE.lock()
        && let Some(gate) = guard.as_mut()
    {
        let _ = gate.acknowledge(action.token, ack, now());
        finish_complete(gate);
        publish(gate);
    }
}
struct DescriptorMemory;
impl Memory for DescriptorMemory {
    fn load(&mut self, address: usize) -> Result<usize, String> {
        word(address as u32)
            .map(|v| v as usize)
            .map_err(|_| "command descriptor unreadable".into())
    }
    fn writable(&mut self, address: usize) -> Result<u32, String> {
        let mut old = 0;
        if unsafe { VirtualProtect(address as *const c_void, 4, PAGE_READWRITE, &mut old) } == 0 {
            return Err("cannot unlock command descriptor".into());
        }
        Ok(old)
    }
    fn exchange(&mut self, address: usize, expected: usize, desired: usize) -> Result<(), String> {
        // SAFETY: aligned descriptor word, validated before journaling, with
        // page protection made writable by the transaction immediately above.
        unsafe { AtomicUsize::from_ptr(address as *mut usize) }
            .compare_exchange(expected, desired, Ordering::AcqRel, Ordering::Acquire)
            .map(drop)
            .map_err(|_| "command descriptor owner changed; refusing overwrite".into())
    }
    fn protect(&mut self, address: usize, protection: u32) -> Result<(), String> {
        let mut ignored = 0;
        if unsafe { VirtualProtect(address as *const c_void, 4, protection, &mut ignored) } == 0 {
            return Err("command descriptor protection not restored; retry removal".into());
        }
        Ok(())
    }
}
/// Descriptors to hook with their expected names, the shared callback and the
/// argc/argv/handler words; validated by the caller.
struct Targets {
    list: Vec<(u32, &'static str)>,
    callback: u32,
    bindings: [u32; 3],
}
fn descriptors(process: &Process, b: &EngineControl, fixture: bool) -> Result<Targets, String> {
    let modules = process.modules().map_err(|e| e.to_string())?;
    let get = |name: &str| {
        modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("{name} missing"))
    };
    if fixture {
        executable(b.fixture[1], get("engine_fixture.exe")?)?;
        let mut list = vec![(b.fixture[0], "map")];
        // The own fixture may register one extra non-player command (exit).
        if b.fixture[5] != 0 {
            list.push((b.fixture[5], "exit"));
        }
        return Ok(Targets {
            list,
            callback: b.fixture[1],
            bindings: [b.fixture[2], b.fixture[3], b.fixture[4]],
        });
    }
    let audit = crate::inspection::commands::inspect(process).map_err(|e| e.to_string())?;
    if !audit.map_registered || !audit.map_profile_matches {
        return Err("map is not registered with the profiled descriptor/callback".into());
    }
    let core = get("prism3d.exe")?;
    let game = get("game.dll")?;
    executable(audit.map_callback, game)?;
    let list = crate::command_registry::world_gate_descriptors(&audit)?
        .into_iter()
        .map(|(command, address)| (address, command.name()))
        .collect();
    Ok(Targets {
        list,
        callback: audit.map_callback,
        bindings: [
            add(core, profile::COMMAND_ARGC)?,
            add(core, profile::COMMAND_ARGV)?,
            add(game, profile::COMMAND_CONTEXT)?,
        ],
    })
}
/// Window-thread install, after pump/render/draw in the same mode.
pub fn install(thread: u32, b: &EngineControl) -> Result<(), String> {
    if STATE.load(Ordering::Acquire) == 1 {
        return Err("command gate already installed".into());
    }
    let context = crate::engine_phase::release_context();
    if !context.pump_installed || context.owner_thread != thread {
        return Err("attach the pump on this window thread before the command gate".into());
    }
    let fixture = b.command == api::UI_LOAD_GATE_FIXTURE;
    if crate::engine_draw::mode() != Some(fixture) || !crate::engine_render::installed() {
        return Err("attach draw and render observers in the same mode before the gate".into());
    }
    let process = Process::open(unsafe { GetCurrentProcessId() }).map_err(|e| e.to_string())?;
    let Targets {
        list,
        callback,
        bindings,
    } = descriptors(&process, b, fixture)?;
    let mut hooked = Vec::new();
    for (descriptor, name) in list {
        let slot = crate::command_registry::command_slot(
            descriptor,
            name.as_bytes(),
            callback,
            bindings,
            |at, out| process.read(at, out).map_err(|e| e.to_string()),
        )?;
        if hooked.contains(&(slot as usize)) || bindings.contains(&slot) {
            return Err("aliased command descriptors".into());
        }
        hooked.push(slot as usize);
    }
    let timeout = match b.cycles {
        0 => load_gate::DEFAULT_TIMEOUT_MS,
        t => t.clamp(1_000, 600_000),
    };
    let gate = Gate::new(thread).map_err(|e| e.to_string())?;
    {
        let mut guard = GATE.lock().map_err(|_| "command gate state poisoned")?;
        publish(&gate);
        *guard = Some(gate);
    }
    ORIGINAL.store(callback as usize, Ordering::Release);
    for (dst, src) in BINDINGS.iter().zip(bindings) {
        dst.store(src, Ordering::Release);
    }
    OWNER.store(thread, Ordering::Release);
    FIXTURE.store(fixture, Ordering::Release);
    TIMEOUT.store(timeout, Ordering::Release);
    FALLBACK.store(false, Ordering::Release);
    RESTORE_PENDING.store(false, Ordering::Release);
    SLOT_COUNT.store(0, Ordering::Release);
    {
        let mut patches = PATCHES
            .lock()
            .map_err(|_| "command patch journal poisoned")?;
        *patches = hooked
            .iter()
            .map(|slot| Patch::new(*slot, callback as usize, gate_shim as *const () as usize))
            .collect();
    }
    // Publish every validated slot BEFORE any write, including a write whose
    // following VirtualProtect call may fail. Never forget a live callback.
    for (i, slot) in hooked.iter().enumerate() {
        SLOTS[i].store(*slot, Ordering::Release);
    }
    SLOT_COUNT.store(hooked.len() as u32, Ordering::Release);
    STATE.store(1, Ordering::Release);
    for index in 0..hooked.len() {
        let result = PATCHES
            .lock()
            .map_err(|_| "command patch journal poisoned")?[index]
            .install(&mut DescriptorMemory);
        if let Err(error) = result {
            let rollback = unhook();
            // An incomplete rollback remains installed/removable. A subsequent
            // attach must not erase the journal of a still-live hook.
            if rollback.is_ok() {
                STATE.store(2, Ordering::Release);
            }
            return Err(format!("{error}; rollback={rollback:?}"));
        }
    }
    Ok(())
}
fn unhook() -> Result<(), String> {
    let mut patches = PATCHES
        .lock()
        .map_err(|_| "command patch journal poisoned")?;
    hook_transaction::restore_all(&mut patches, &mut DescriptorMemory)
}
/// Refuses while a deferred command may still act; terminal jobs are dropped.
pub fn remove() -> Result<(), String> {
    if STATE.load(Ordering::Acquire) != 1 {
        return Ok(());
    }
    if FLIGHT.load(Ordering::Acquire) != 0 || REPLAYING.load(Ordering::Acquire) {
        return Err("command gate callback active; retry removal".into());
    }
    if busy() {
        return Err("deferred command still active; wait for load-gate-status to settle".into());
    }
    unhook()?;
    STATE.store(2, Ordering::Release);
    if let Ok(mut guard) = GATE.lock() {
        *guard = None;
    }
    JOB_ACTIVE.store(false, Ordering::Release);
    RESTORE_PENDING.store(false, Ordering::Release);
    Ok(())
}
pub(crate) fn installed() -> bool {
    STATE.load(Ordering::Acquire) == 1
}
/// Remote-thread status from published atomics only.
pub fn control(b: &mut EngineControl) -> Result<(), String> {
    let flags = FLAGS.load(Ordering::Acquire);
    let state = STATE.load(Ordering::Acquire);
    Status {
        installed: state == 1,
        fixture: FIXTURE.load(Ordering::Acquire),
        owner_thread: OWNER.load(Ordering::Acquire),
        timeout_ms: TIMEOUT.load(Ordering::Acquire),
        phase: if state == 1 {
            load_gate::phase_from(PHASE.load(Ordering::Acquire))
        } else {
            Some(Phase::Idle)
        },
        job: JOB.load(Ordering::Acquire),
        in_flight: flags & 1 != 0,
        execute_started: flags & 2 != 0,
        release_confirmed: flags & 4 != 0,
        reason: load_gate::REASONS
            .get(REASON.load(Ordering::Acquire) as usize)
            .copied()
            .unwrap_or("other"),
        calls: count(Count::Calls),
        deferred: count(Count::Deferred),
        forwarded: count(Count::Forwarded),
        foreign: count(Count::Foreign),
        replay_nested: count(Count::ReplayNested),
        swallowed_busy: count(Count::SwallowedBusy),
        dropped: count(Count::Dropped),
        last_note: Note::from_u32(LAST_NOTE.load(Ordering::Acquire)),
        replays: count(Count::Replays),
        last_return: LAST_RETURN.load(Ordering::Acquire),
        arguments_restored: count(Count::ArgumentsRestored),
        arguments_retained: count(Count::ArgumentsRetained),
        recreated: count(Count::Recreated),
        fallback_releases: count(Count::FallbackReleases),
        fallback_pending: FALLBACK.load(Ordering::Acquire),
        restore_pending: RESTORE_PENDING.load(Ordering::Acquire),
        last_command: WorldCommand::from_u32(LAST_COMMAND.load(Ordering::Acquire)),
        actor: WITNESS[0].load(Ordering::Acquire),
        scene: WITNESS[1].load(Ordering::Acquire),
        epoch: WITNESS[2].load(Ordering::Acquire),
        automatic_load_supported: false,
    }
    .encode(b);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READONLY,
        VirtualAlloc, VirtualFree, VirtualQuery,
    };

    struct Page(*mut c_void);
    impl Drop for Page {
        fn drop(&mut self) {
            unsafe { VirtualFree(self.0, 0, MEM_RELEASE) };
        }
    }
    struct FaultMemory {
        memory: DescriptorMemory,
        failure: Option<usize>,
    }
    impl Memory for FaultMemory {
        fn load(&mut self, address: usize) -> Result<usize, String> {
            self.memory.load(address)
        }
        fn writable(&mut self, address: usize) -> Result<u32, String> {
            self.memory.writable(address)
        }
        fn exchange(
            &mut self,
            address: usize,
            expected: usize,
            desired: usize,
        ) -> Result<(), String> {
            self.memory.exchange(address, expected, desired)
        }
        fn protect(&mut self, address: usize, protection: u32) -> Result<(), String> {
            if self.failure == Some(address) {
                return Err("fixture protection failure".into());
            }
            self.memory.protect(address, protection)
        }
    }
    #[test]
    fn actual_x86_pages_keep_original_protection_after_failed_install_and_rollback_retry() {
        // Own data only, with all six callback words on ONE real read-only
        // page. No engine image, injection, commercial callback or global gate.
        for index in 0..MAX_SLOTS {
            let page = Page(unsafe {
                VirtualAlloc(ptr::null(), 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
            });
            assert!(!page.0.is_null());
            for offset in 0..MAX_SLOTS {
                unsafe { page.0.cast::<usize>().add(offset).write(10) };
            }
            let mut memory = FaultMemory {
                memory: DescriptorMemory,
                failure: None,
            };
            memory.protect(page.0 as usize, PAGE_READONLY).unwrap();
            let addresses: Vec<_> = (0..MAX_SLOTS)
                .map(|offset| page.0 as usize + offset * 4)
                .collect();
            let mut patches: Vec<_> = addresses
                .iter()
                .map(|address| Patch::new(*address, 10, 20))
                .collect();
            memory.failure = Some(addresses[index]);
            for (offset, patch) in patches.iter_mut().enumerate() {
                let result = patch.install(&mut memory);
                if offset == index {
                    assert!(result.is_err());
                    break;
                }
                result.unwrap();
            }
            assert!(hook_transaction::restore_all(&mut patches, &mut memory).is_err());
            // Callback words can already be original while protection is still
            // pending. Retrying must restore READONLY rather than temporary RW.
            for address in &addresses {
                assert_eq!(memory.load(*address).unwrap(), 10);
            }
            memory.failure = None;
            hook_transaction::restore_all(&mut patches, &mut memory).unwrap();
            let mut info = unsafe { std::mem::zeroed::<MEMORY_BASIC_INFORMATION>() };
            assert_ne!(
                unsafe { VirtualQuery(page.0, &mut info, std::mem::size_of_val(&info)) },
                0
            );
            assert_eq!(info.Protect, PAGE_READONLY);
        }
    }
}

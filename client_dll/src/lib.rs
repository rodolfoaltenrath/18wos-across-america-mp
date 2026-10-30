//! Win32 DLL prototype. Call client_start AFTER LoadLibrary returns and call
//! client_stop BEFORE FreeLibrary. DllMain deliberately performs no work.
use multiplayer_server::protocol::{
    self, ClientUpdate, MAX_PACKET, Position, Rotation, TICK_HZ, VERSION,
};
use std::{
    collections::BTreeMap,
    net::{SocketAddr, UdpSocket},
    panic::{self, AssertUnwindSafe},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub mod addresses;
pub mod api;
pub mod command_registry;
#[cfg(windows)]
pub mod controller;
pub mod deferred_load;
pub mod doctor;
pub mod draw_observation;
#[cfg(windows)]
mod engine_bridge;
pub mod engine_contract;
pub mod engine_discovery;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_draw;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_info;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_instance;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_item;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_lifecycle;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_load_gate;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_model;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_phase;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_render;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_transition;
pub mod engine_values;
#[cfg(all(windows, target_arch = "x86"))]
mod engine_visual;
#[cfg(any(all(windows, target_arch = "x86"), test))]
mod hook_transaction;
#[cfg(windows)]
pub mod inspection;
pub mod interpolation;
pub mod lan_kit;
pub mod load_gate;
mod memory;
pub mod network_conditions;
pub mod pe_exports;
#[cfg(any(all(windows, target_arch = "x86"), test))]
mod phase_queue;
pub mod presentation;
pub mod profile;
pub mod release_policy;
pub mod render_observation;
pub mod resource_bindings;
pub mod transition_analysis;
pub mod transition_observation;
pub mod udp_proxy;

static WORKER: Mutex<Option<JoinHandle<Result<(), String>>>> = Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);
static STATUS: AtomicU32 = AtomicU32::new(0);
static SENT: AtomicU64 = AtomicU64::new(0);
static RECEIVED: AtomicU64 = AtomicU64::new(0);
static SAMPLE_STATE: AtomicU32 = AtomicU32::new(0);
static READ_FAILURES: AtomicU64 = AtomicU64::new(0);
static MESSAGE: Mutex<String> = Mutex::new(String::new());
static SESSION: Mutex<Option<multiplayer_server::session_client::Status>> = Mutex::new(None);
static NETWORK_TIMING: Mutex<Option<multiplayer_server::session_timing::Timing>> = Mutex::new(None);
/// Latest relayed state per player. The render thread only uses try_lock.
pub(crate) static REMOTES: Mutex<BTreeMap<u64, RemoteEntry>> = Mutex::new(BTreeMap::new());
/// Local pose observed by the draw probe (same coordinates the game draws with).
pub(crate) static LOCAL_POSE: Mutex<Option<(Position, Rotation, Instant)>> = Mutex::new(None);
pub(crate) struct RemoteEntry {
    pub(crate) state: RemoteState,
    /// Read by the x86 test visual; other targets only relay it.
    #[cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code))]
    pub(crate) rotation: Option<Rotation>,
    pub(crate) seen: Instant,
    /// Recent samples for delayed interpolation, newest last.
    #[cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code))]
    pub(crate) history: interpolation::History,
}

/// C ABI result layout. Coordinates are the most recent relayed samples;
/// rendering and interpolation are future game integration work.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RemoteState {
    pub player_id: u64,
    pub sequence: u64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

enum Source {
    Simulated {
        phase: f32,
    },
    Memory {
        addresses: [addresses::Address; 3],
    },
    Game {
        verified_base: usize,
    },
    /// Pose published by the actors.car draw observer, including rotation.
    Drawn,
}

#[derive(Debug)]
enum SampleError {
    Unavailable(String),
    Fatal(String),
}

fn diagnostic(message: &str) {
    if let Ok(mut value) = MESSAGE.lock() {
        if *value != message {
            eprintln!("client_dll: {message}");
        }
        *value = message.into();
    }
}

impl Source {
    fn sample(&mut self, elapsed: f32) -> Result<(Position, Option<Rotation>), SampleError> {
        match self {
            Self::Simulated { phase } => Ok((
                Position {
                    x: (elapsed + *phase).cos() * 100.0,
                    y: 0.0,
                    z: (elapsed + *phase).sin() * 100.0,
                },
                None,
            )),
            Self::Memory { addresses } => memory::position(addresses)
                .map(|p| (p, None))
                .map_err(SampleError::Unavailable),
            Self::Game { verified_base } => memory::game_position(verified_base).map(|p| (p, None)),
            Self::Drawn => {
                let pose = *LOCAL_POSE
                    .lock()
                    .map_err(|_| SampleError::Fatal("local pose mutex poisoned".into()))?;
                match pose {
                    Some((position, rotation, seen))
                        if seen.elapsed() < Duration::from_millis(500) =>
                    {
                        Ok((position, Some(rotation)))
                    }
                    _ => Err(SampleError::Unavailable(
                        "waiting for a drawn exterior pose (game in foreground)".into(),
                    )),
                }
            }
        }
    }
}

fn configuration() -> Result<(SocketAddr, Source), String> {
    let server = std::env::var("AA_MP_SERVER")
        .unwrap_or_else(|_| protocol::DEFAULT_SERVER.into())
        .parse()
        .map_err(|_| "AA_MP_SERVER must be an IP:port".to_string())?;
    let mode = std::env::var("AA_MP_MODE").unwrap_or_else(|_| "simulated".into());
    let source = match mode.as_str() {
        "simulated" => {
            let phase: f32 = std::env::var("AA_MP_SIM_PHASE")
                .unwrap_or_else(|_| "0".into())
                .parse()
                .map_err(|_| "invalid simulation phase".to_string())?;
            if !phase.is_finite() {
                return Err("simulation phase must be finite".into());
            }
            Source::Simulated { phase }
        }
        "memory" => {
            let parse = |key| -> Result<addresses::Address, String> {
                let text = std::env::var(key).map_err(|_| {
                    format!("missing {key}; memory mode requires explicit addresses")
                })?;
                text.parse().map_err(|error| format!("{key}: {error}"))
            };
            Source::Memory {
                addresses: [parse("AA_MP_X")?, parse("AA_MP_Y")?, parse("AA_MP_Z")?],
            }
        }
        _ => return Err("AA_MP_MODE must be simulated or memory".into()),
    };
    Ok((server, source))
}

fn publish_remote(update: protocol::ServerUpdate, reset_history: bool) -> Result<(), String> {
    let mut remotes = REMOTES.lock().map_err(|_| "remote state mutex poisoned")?;
    let is_new = reset_history
        || remotes
            .get(&update.player_id)
            .is_none_or(|previous| update.sequence > previous.state.sequence);
    if is_new && (remotes.contains_key(&update.player_id) || remotes.len() < 64) {
        let previous = remotes.remove(&update.player_id);
        let mut history = if reset_history {
            interpolation::History::default()
        } else {
            previous.map(|entry| entry.history).unwrap_or_default()
        };
        let r = update.rotation.unwrap_or(Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        });
        history.push(interpolation::Sample {
            at: interpolation::now(),
            position: [update.position.x, update.position.y, update.position.z],
            rotation: [r.x, r.y, r.z, r.w],
        });
        remotes.insert(
            update.player_id,
            RemoteEntry {
                history,
                state: RemoteState {
                    player_id: update.player_id,
                    sequence: update.sequence,
                    x: update.position.x,
                    y: update.position.y,
                    z: update.position.z,
                },
                rotation: update.rotation,
                seen: Instant::now(),
            },
        );
        RECEIVED.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}
fn session_events(events: Vec<multiplayer_server::session_client::Event>) -> Result<(), String> {
    use multiplayer_server::session_client::Event;
    for event in events {
        match event {
            Event::Reset => REMOTES
                .lock()
                .map_err(|_| "remote state mutex poisoned")?
                .clear(),
            Event::Roster(ids) => REMOTES
                .lock()
                .map_err(|_| "remote state mutex poisoned")?
                .retain(|id, _| u32::try_from(*id).is_ok_and(|id| ids.contains(&id))),
            Event::Pose {
                pose,
                reset_history,
            } => publish_remote(
                protocol::ServerUpdate {
                    version: VERSION,
                    player_id: u64::from(pose.player),
                    sequence: pose.sequence,
                    position: pose.position,
                    rotation: pose.rotation,
                },
                reset_history,
            )?,
        }
    }
    Ok(())
}
fn transmit(socket: &UdpSocket, bytes: &[u8]) -> Result<(), String> {
    match socket.send(bytes) {
        Ok(_) => {
            SENT.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::ConnectionRefused
            ) || e.raw_os_error() == Some(10054) =>
        {
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}
fn session_snapshot(client: &multiplayer_server::session_client::Client) -> Result<(), String> {
    *SESSION
        .lock()
        .map_err(|_| "session status mutex poisoned")? = Some(client.status());
    *NETWORK_TIMING
        .lock()
        .map_err(|_| "network timing mutex poisoned")? = Some(client.timing());
    Ok(())
}
fn run(
    socket: UdpSocket,
    mut source: Source,
    options: Option<multiplayer_server::session_client::Options>,
) -> Result<(), String> {
    use multiplayer_server::{
        session_client::{Client, Phase},
        session_protocol as wire,
    };
    let mut session = options
        .map(|o| Client::new(o, multiplayer_server::identity::random_nonce()?))
        .transpose()?;
    let tick = Duration::from_secs_f64(1.0 / f64::from(TICK_HZ));
    let start = Instant::now();
    let mut next_tick = start;
    let mut sequence = 0u64;
    let mut buffer = [0; MAX_PACKET + 1];
    while !STOP.load(Ordering::Acquire) {
        if let Some(client) = &mut session {
            let (frames, events) = client.poll(start.elapsed().as_millis() as u64);
            session_events(events)?;
            for frame in frames {
                transmit(&socket, &wire::encode_client(&frame)?)?;
            }
            session_snapshot(client)?;
            if client.status().phase == Phase::Rejected {
                return Err(format!("session rejected: {:?}", client.status().refusal));
            }
        }
        let sample = match source.sample(start.elapsed().as_secs_f32()) {
            Ok(value) => {
                if SAMPLE_STATE.swap(1, Ordering::AcqRel) != 1 {
                    diagnostic("coordinate capture available");
                }
                Some(value)
            }
            Err(SampleError::Unavailable(message)) => {
                READ_FAILURES.fetch_add(1, Ordering::Relaxed);
                SAMPLE_STATE.store(2, Ordering::Release);
                diagnostic(&message);
                None
            }
            Err(SampleError::Fatal(message)) => {
                SAMPLE_STATE.store(3, Ordering::Release);
                return Err(message);
            }
        };
        if let Some((position, rotation)) = sample {
            if !position.is_valid() || !rotation.is_none_or(Rotation::is_valid) {
                return Err("coordinate sample contains NaN/infinity".into());
            }
            let bytes = if let Some(client) = &mut session {
                client
                    .pose(position, rotation, start.elapsed().as_millis() as u64)
                    .map(|frame| wire::encode_client(&frame))
                    .transpose()?
            } else {
                Some(protocol::encode(&ClientUpdate {
                    version: VERSION,
                    sequence,
                    position,
                    rotation,
                })?)
            };
            if let Some(bytes) = bytes {
                transmit(&socket, &bytes)?;
            }
            sequence = sequence.checked_add(1).ok_or("sequence exhausted")?;
        }
        // Bound work per tick so incoming traffic cannot prevent shutdown.
        for _ in 0..256 {
            match socket.recv(&mut buffer) {
                Ok(size) => {
                    if let Some(client) = &mut session {
                        session_events(
                            client.ingest(&buffer[..size], start.elapsed().as_millis() as u64),
                        )?;
                        continue;
                    }
                    let Some(update) = protocol::decode_server(&buffer[..size]) else {
                        continue;
                    };
                    publish_remote(update, false)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error)
                    if matches!(error.raw_os_error(), Some(10040 | 10054))
                        || error.kind() == std::io::ErrorKind::ConnectionRefused =>
                {
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        REMOTES
            .lock()
            .map_err(|_| "remote state mutex poisoned")?
            .retain(|_, entry| entry.seen.elapsed() < Duration::from_secs(5));
        if let Some(client) = &session {
            session_snapshot(client)?;
        }
        next_tick += tick;
        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        } else {
            next_tick = now;
        } // Skip missed ticks instead of sending a burst.
    }
    if let Some(client) = &mut session {
        client.close(start.elapsed().as_millis() as u64);
        let closing = Instant::now();
        while closing.elapsed() < Duration::from_millis(220)
            && client.status().phase != Phase::Closed
        {
            let (frames, _) = client.poll(start.elapsed().as_millis() as u64);
            for frame in frames {
                transmit(&socket, &wire::encode_client(&frame)?)?;
            }
            for _ in 0..32 {
                match socket.recv(&mut buffer) {
                    Ok(size) => {
                        let _ = client.ingest(&buffer[..size], start.elapsed().as_millis() as u64);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error)
                        if matches!(error.raw_os_error(), Some(10040 | 10054))
                            || error.kind() == std::io::ErrorKind::ConnectionRefused =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        session_snapshot(client)?;
    }
    Ok(())
}

fn start() -> Result<(), String> {
    let (server, source) = configuration()?;
    let options = match (
        std::env::var_os("AA_MP_ROOM"),
        std::env::var_os("AA_MP_IDENTITY_FILE"),
    ) {
        (None, None) => None,
        (Some(room), Some(path)) => Some(multiplayer_server::session_client::Options {
            room: room.into_string().map_err(|_| "room must be UTF-8")?,
            identity: multiplayer_server::identity::load_or_create(std::path::Path::new(&path))?,
            profile: profile::ID.into(),
        }),
        _ => return Err("set both AA_MP_ROOM and AA_MP_IDENTITY_FILE, or neither".into()),
    };
    start_source_session(server, source, options)
}

fn explicit_configuration(config: &api::ClientConfig) -> Result<(SocketAddr, Source), String> {
    if config.version != api::ABI_VERSION
        || config.size != std::mem::size_of::<api::ClientConfig>() as u32
    {
        return Err("incompatible configuration ABI version/size".into());
    }
    let server: SocketAddr = api::text(&config.server)?
        .parse()
        .map_err(|_| "server must be IP:port")?;
    if server.port() == 0 || server.ip().is_unspecified() || server.ip().is_multicast() {
        return Err("server must be a unicast destination with nonzero port".into());
    }
    let source = match config.mode {
        api::SIMULATED if config.phase.is_finite() => Source::Simulated {
            phase: config.phase,
        },
        api::GAME => {
            if api::text(&config.profile)? != profile::ID {
                return Err("unknown game profile".into());
            }
            if !cfg!(all(windows, target_pointer_width = "32")) {
                return Err("game profile requires Windows x86 DLL".into());
            }
            Source::Game { verified_base: 0 }
        }
        api::DRAWN => {
            if !cfg!(all(windows, target_pointer_width = "32")) {
                return Err("drawn pose requires the Windows x86 DLL".into());
            }
            Source::Drawn
        }
        api::MEMORY => Source::Memory {
            addresses: [
                api::text(&config.addresses[0])?.parse()?,
                api::text(&config.addresses[1])?.parse()?,
                api::text(&config.addresses[2])?.parse()?,
            ],
        },
        _ => return Err("invalid source mode or simulation phase".into()),
    };
    eprintln!(
        "client_dll: server={server} mode={} profile={}",
        config.mode,
        api::text(&config.profile)?
    );
    Ok((server, source))
}

fn start_source(server: SocketAddr, source: Source) -> Result<(), String> {
    start_source_session(server, source, None)
}
fn start_source_session(
    server: SocketAddr,
    source: Source,
    options: Option<multiplayer_server::session_client::Options>,
) -> Result<(), String> {
    if let Some(options) = &options {
        options.validate()?;
    }
    let mut worker = WORKER.lock().map_err(|_| "worker mutex poisoned")?;
    if worker.as_ref().is_some_and(|handle| !handle.is_finished()) {
        return Err("client already running".into());
    }
    if let Some(handle) = worker.take() {
        let _ = handle.join();
    }
    let bind = if server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind).map_err(|e| e.to_string())?;
    socket.connect(server).map_err(|e| e.to_string())?;
    socket.set_nonblocking(true).map_err(|e| e.to_string())?;
    REMOTES
        .lock()
        .map_err(|_| "remote state mutex poisoned")?
        .clear();
    STOP.store(false, Ordering::Release);
    SENT.store(0, Ordering::Relaxed);
    RECEIVED.store(0, Ordering::Relaxed);
    READ_FAILURES.store(0, Ordering::Relaxed);
    SAMPLE_STATE.store(0, Ordering::Release);
    *SESSION
        .lock()
        .map_err(|_| "session status mutex poisoned")? = None;
    *NETWORK_TIMING
        .lock()
        .map_err(|_| "network timing mutex poisoned")? = None;
    diagnostic("starting client");
    STATUS.store(1, Ordering::Release);
    *worker = Some(
        thread::Builder::new()
            .name("aa-multiplayer".into())
            .spawn(move || {
                let result = panic::catch_unwind(AssertUnwindSafe(|| run(socket, source, options)))
                    .unwrap_or_else(|_| Err("worker panicked".into()));
                STATUS.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
                if let Err(error) = &result {
                    diagnostic(error);
                }
                result
            })
            .map_err(|e| {
                STATUS.store(3, Ordering::Release);
                e.to_string()
            })?,
    );
    Ok(())
}

/// Start outside DllMain/loader lock. Returns 0 on success, 1 on error.
#[unsafe(no_mangle)]
pub extern "system" fn client_start() -> i32 {
    match panic::catch_unwind(start) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            diagnostic(&error);
            1
        }
        Err(_) => 1,
    }
}

/// Stop and JOIN outside DllMain. The host must call this before unloading.
#[unsafe(no_mangle)]
pub extern "system" fn client_stop() -> i32 {
    panic::catch_unwind(|| {
        let Ok(mut worker) = WORKER.lock() else {
            return 1;
        };
        STOP.store(true, Ordering::Release);
        let result = match worker.take().map(|handle| handle.join()) {
            None | Some(Ok(Ok(()))) => 0,
            _ => 1,
        };
        if let Ok(mut remotes) = REMOTES.lock() {
            remotes.clear();
        }
        #[cfg(windows)]
        if let Err(error) = engine_bridge::detach() {
            diagnostic(&error);
            STATUS.store(3, Ordering::Release);
            return 1;
        }
        STATUS.store(2, Ordering::Release);
        SAMPLE_STATE.store(0, Ordering::Release);
        result
    })
    .unwrap_or(1)
}

/// 0 = never started; 1 = running; 2 = stopped; 3 = worker/dispatcher failure.
#[unsafe(no_mangle)]
pub extern "system" fn client_status() -> u32 {
    STATUS.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
pub extern "system" fn client_packets_sent() -> u64 {
    SENT.load(Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "system" fn client_packets_received() -> u64 {
    RECEIVED.load(Ordering::Relaxed)
}

/// Explicit control, callable as a Win32 remote-thread procedure. No Rust
/// allocations or pointers cross the ABI. Never call from DllMain.
///
/// # Safety
/// `block` must be aligned, exclusively owned writable storage for a Control,
/// alive until return. A mismatched version/size returns before reading payload.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn client_control(block: *mut api::Control) -> u32 {
    if block.is_null() || !(block as usize).is_multiple_of(std::mem::align_of::<api::Control>()) {
        return 1;
    }
    // SAFETY: caller guarantees readable header; payload is read only after validation.
    let header = block.cast::<u32>();
    if unsafe { header.read() } != api::ABI_VERSION
        || unsafe { header.add(1).read() } != std::mem::size_of::<api::Control>() as u32
    {
        return 1;
    }
    panic::catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: validated header and caller's exclusive writable buffer contract.
        let block = unsafe { &mut *block };
        let result = match block.command {
            api::START => explicit_configuration(&block.config)
                .and_then(|(server, source)| start_source(server, source)),
            api::START_SESSION => (|| {
                let options: multiplayer_server::session_client::Options =
                    serde_json::from_str(api::text(&block.message)?)
                        .map_err(|_| "invalid session options JSON")?;
                if options.profile != api::text(&block.config.profile)? {
                    return Err("session/source profile mismatch".into());
                }
                let (server, source) = explicit_configuration(&block.config)?;
                start_source_session(server, source, Some(options))
            })(),
            api::SESSION_STATUS => Ok(()),
            api::NETWORK_TIMING => Ok(()),
            api::PRESENTATION_SET => serde_json::from_str::<interpolation::Config>(
                api::text(&block.message).unwrap_or(""),
            )
            .map_err(|e| format!("invalid presentation configuration: {e}"))
            .and_then(presentation::configure),
            api::PRESENTATION_STATUS => Ok(()),
            api::STATUS => Ok(()),
            api::STOP => {
                if client_stop() == 0 {
                    Ok(())
                } else {
                    Err(MESSAGE
                        .lock()
                        .map(|m| format!("stop incomplete: {m}"))
                        .unwrap_or_else(|_| "stop failed".into()))
                }
            }
            _ => Err("unknown control command".into()),
        };
        block.result = u32::from(result.is_err());
        if let Err(message) = result {
            diagnostic(&message);
        }
        block.status = client_status();
        block.sample_state = SAMPLE_STATE.load(Ordering::Acquire);
        block.sent = client_packets_sent();
        block.received = client_packets_received();
        block.read_failures = READ_FAILURES.load(Ordering::Relaxed);
        if block.command == api::PRESENTATION_STATUS
            || (block.command == api::PRESENTATION_SET && block.result == 0)
        {
            if serde_json::to_string(&presentation::status())
                .map_err(|e| e.to_string())
                .and_then(|json| api::put(&mut block.message, &json))
                .is_err()
            {
                block.result = 1;
            }
        } else if block.command == api::NETWORK_TIMING {
            let snapshot = NETWORK_TIMING
                .lock()
                .map_err(|_| "network timing mutex poisoned".to_string())
                .and_then(|s| serde_json::to_string(&*s).map_err(|e| e.to_string()));
            if snapshot
                .and_then(|s| api::put(&mut block.message, &s))
                .is_err()
            {
                block.result = 1;
            }
        } else if block.command == api::SESSION_STATUS {
            let snapshot = SESSION
                .lock()
                .map_err(|_| "session status mutex poisoned".to_string())
                .and_then(|s| serde_json::to_string(&*s).map_err(|e| e.to_string()));
            if snapshot
                .and_then(|s| api::put(&mut block.message, &s))
                .is_err()
            {
                block.result = 1;
            }
        } else if let Ok(message) = MESSAGE.lock() {
            let mut end = message.len().min(block.message.len() - 1);
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            let _ = api::put(&mut block.message, &message[..end]);
        }
        block.result
    }))
    .unwrap_or(1)
}

/// Optional UI-thread extension. The caller owns a complete, aligned block
/// until return; no engine constructor is exposed by this API.
///
/// # Safety
/// `block` must be exclusive writable storage for an EngineControl. Do not
/// call under loader lock or unload this module until client_stop succeeds.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn client_engine_control(block: *mut api::EngineControl) -> u32 {
    if block.is_null()
        || !(block as usize).is_multiple_of(std::mem::align_of::<api::EngineControl>())
    {
        return 1;
    }
    let header = block.cast::<u32>();
    if unsafe { header.read() } != api::ENGINE_ABI_VERSION
        || unsafe { header.add(1).read() } != std::mem::size_of::<api::EngineControl>() as u32
    {
        return 1;
    }
    panic::catch_unwind(AssertUnwindSafe(|| {
        let block = unsafe { &mut *block };
        let result = if cfg!(target_pointer_width = "32") {
            engine_bridge::control(block)
        } else {
            Err("engine extension requires Windows x86".into())
        };
        block.result = u32::from(result.is_err());
        if let Err(error) = result {
            let mut end = error.len().min(block.message.len() - 1);
            while !error.is_char_boundary(end) {
                end -= 1;
            }
            let _ = api::put(&mut block.message, &error[..end]);
        }
        block.result
    }))
    .unwrap_or(1)
}

/// Fill one relayed player row (index in `reserved`) for diagnostics and listing.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn remote_row(b: &mut api::EngineControl) -> Result<(), String> {
    let remotes = REMOTES.lock().map_err(|_| "remote state mutex poisoned")?;
    let count = u32::try_from(remotes.len()).map_err(|e| e.to_string())?;
    let index = b.reserved;
    let row = match remotes.values().nth(index as usize) {
        Some(entry) => {
            let r = entry.rotation.unwrap_or(Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            });
            draw_observation::Remote {
                index,
                count,
                player_id: u32::try_from(entry.state.player_id).unwrap_or(u32::MAX),
                sequence: entry.state.sequence as u32,
                age_ms: u32::try_from(entry.seen.elapsed().as_millis()).unwrap_or(u32::MAX),
                samples: entry.history.len() as u32,
                has_rotation: entry.rotation.is_some(),
                position: [entry.state.x, entry.state.y, entry.state.z],
                rotation: [r.x, r.y, r.z, r.w],
            }
        }
        None if count == 0 => draw_observation::Remote::default(),
        None => return Err("no relayed player at that index".into()),
    };
    row.encode(b);
    Ok(())
}

/// Copy a remote player by sorted index. Returns 1 when found, 0 otherwise.
///
/// # Safety
/// `output` must point to writable, aligned storage for one RemoteState.
/// Call only after loading the DLL and before unloading it, outside DllMain.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn client_get_remote(index: u32, output: *mut RemoteState) -> i32 {
    if output.is_null() {
        return 0;
    }
    panic::catch_unwind(|| {
        let Ok(remotes) = REMOTES.lock() else {
            return 0;
        };
        let Some(entry) = remotes.values().nth(index as usize) else {
            return 0;
        };
        // SAFETY: the caller guarantees valid writable storage as documented.
        unsafe {
            output.write(entry.state);
        }
        1
    })
    .unwrap_or(0)
}

/// Windows loader entry point: no sockets, allocation, waiting or threads here.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllMain(
    _module: windows_sys::Win32::Foundation::HINSTANCE,
    _reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_simulated_samples() {
        assert!("0x1234".parse::<addresses::Address>().is_ok());
        assert!("0".parse::<addresses::Address>().is_err());
        assert!("not-an-address".parse::<addresses::Address>().is_err());
        assert!(
            Source::Simulated { phase: 0.0 }
                .sample(1.0)
                .unwrap()
                .0
                .is_valid()
        );
    }
    #[test]
    fn drawn_source_requires_a_fresh_published_pose_and_carries_rotation() {
        let mut source = Source::Drawn;
        *LOCAL_POSE.lock().unwrap() = None;
        assert!(matches!(
            source.sample(0.0),
            Err(SampleError::Unavailable(_))
        ));
        let position = Position {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        };
        let rotation = Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        *LOCAL_POSE.lock().unwrap() = Some((position, rotation, Instant::now()));
        let (p, r) = source.sample(0.0).unwrap();
        assert_eq!((p, r), (position, Some(rotation)));
        let old = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        *LOCAL_POSE.lock().unwrap() = Some((position, rotation, old));
        assert!(matches!(
            source.sample(0.0),
            Err(SampleError::Unavailable(_))
        ));
        *LOCAL_POSE.lock().unwrap() = None;
    }
}

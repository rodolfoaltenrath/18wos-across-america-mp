//! Deterministic robustness run over every in-memory decoder that receives
//! bytes from the network, a remote process, our ABI blocks or saved reports.
//! Properties: no panic, bounded time, and accepted inputs re-encode to an
//! input the same decoder accepts again with the same meaning.
use crate::api::EngineControl;
use multiplayer_server::{
    fuzz::{Rng, mutate},
    protocol::{self, ClientUpdate, Position, Rotation, ServerUpdate},
    session_protocol::{
        self as w, ClientFrame, ClientMessage as C, Credentials, EventBody, PlayerState,
        ServerFrame, ServerMessage as S,
    },
};
use serde::Serialize;
use std::{
    panic::{self, AssertUnwindSafe},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Rejected,
}
/// A failed property, distinct from a rejected input.
pub type Check = Result<Outcome, String>;
pub struct Target {
    pub name: &'static str,
    pub seeds: Vec<Vec<u8>>,
    pub run: fn(&[u8]) -> Check,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct TargetReport {
    pub name: &'static str,
    pub inputs: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub panics: u64,
    pub property_failures: u64,
    pub max_micros: u64,
    /// First failing input, hex, truncated to 256 bytes (own data only).
    pub first_failure: Option<String>,
    pub first_message: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub seed: u64,
    pub iterations_per_target: u64,
    pub targets: Vec<TargetReport>,
    pub passed: bool,
}

fn credentials() -> Credentials {
    Credentials {
        session: 7,
        connection: 9,
        token: "c".repeat(32),
    }
}
fn position() -> Position {
    Position {
        x: 1.5,
        y: -2.0,
        z: 3.25,
    }
}
fn rotation() -> Option<Rotation> {
    Some(Rotation {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    })
}
fn v2_client_seeds() -> Vec<Vec<u8>> {
    [
        C::Join {
            request: 3,
            room: "lan".into(),
            identity: "a".repeat(32),
            profile: w::PROFILE.into(),
            resume: Some(credentials()),
            admission: None,
        },
        C::Pose {
            credentials: credentials(),
            sequence: 5,
            client_ms: 10,
            position: position(),
            rotation: rotation(),
        },
        C::Ping {
            credentials: credentials(),
            sequence: 1,
            client_ms: 2,
        },
        C::Leave {
            credentials: credentials(),
        },
        C::Event {
            credentials: credentials(),
            stream: 4,
            id: 1,
            ttl_ms: 1000,
            body: EventBody::State {
                state: PlayerState::Loading,
            },
        },
        C::RelayAck {
            credentials: credentials(),
            event: 8,
        },
    ]
    .into_iter()
    .map(|m| w::encode_client(&ClientFrame::new(m)).unwrap())
    .collect()
}
fn v2_server_seeds() -> Vec<Vec<u8>> {
    let pose = w::Pose {
        player: 3,
        generation: 3,
        sequence: 4,
        received_ms: 5,
        position: position(),
        rotation: rotation(),
    };
    [
        S::Accepted {
            request: 1,
            credentials: credentials(),
            player: 2,
            revision: 3,
            timeout_ms: 5000,
            server_ms: 4,
        },
        S::Rejected {
            request: 1,
            reason: w::Refusal::Full,
            supported: 2,
        },
        S::Pong {
            session: 7,
            connection: 9,
            sequence: 3,
            client_ms: 4,
            server_ms: 5,
        },
        S::Roster {
            session: 7,
            revision: 2,
            players: vec![3, 4, w::ECHO_BASE + 1],
        },
        S::Snapshot {
            session: 7,
            revision: 2,
            tick: 3,
            server_ms: 9,
            poses: vec![pose, w::Pose { player: 4, ..pose }],
        },
        S::Left {
            session: 7,
            connection: 9,
        },
        S::Closing {
            session: 7,
            connection: 9,
        },
        S::EventAck {
            session: 7,
            connection: 9,
            stream: 3,
            id: 4,
        },
        S::Relay {
            session: 7,
            event: 2,
            origin: 3,
            server_ms: 4,
            body: EventBody::Marker { code: 9 },
        },
    ]
    .into_iter()
    .map(|m| w::encode_server(&ServerFrame::new(m)).unwrap())
    .collect()
}
/// Accepted frames must survive our own encoder and decode to the same value.
fn stable<T: Serialize>(
    decoded: Option<T>,
    encode: impl Fn(&T) -> Result<Vec<u8>, String>,
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Check {
    let Some(value) = decoded else {
        return Ok(Outcome::Rejected);
    };
    let bytes = encode(&value).map_err(|e| format!("accepted but not re-encodable: {e}"))?;
    let again = decode(&bytes).ok_or("re-encoded frame was rejected")?;
    let (a, b) = (
        serde_json::to_value(&value).map_err(|e| e.to_string())?,
        serde_json::to_value(&again).map_err(|e| e.to_string())?,
    );
    if a != b {
        return Err("round trip changed the decoded value".into());
    }
    Ok(Outcome::Accepted)
}
fn block(bytes: &[u8]) -> EngineControl {
    let mut b = EngineControl::new(0);
    let n = bytes.len().min(std::mem::size_of::<EngineControl>());
    // SAFETY: EngineControl is repr(C) and made only of integers/byte arrays,
    // so every bit pattern is valid; we copy at most its size.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), (&mut b as *mut EngineControl).cast(), n)
    };
    b
}
fn block_bytes(b: &EngineControl) -> Vec<u8> {
    // SAFETY: plain-old-data view of an initialized repr(C) value.
    unsafe {
        std::slice::from_raw_parts(
            (b as *const EngineControl).cast::<u8>(),
            std::mem::size_of::<EngineControl>(),
        )
    }
    .to_vec()
}
/// Remote-memory style reader over the input; address 0 is never mapped.
fn reader(bytes: &[u8]) -> impl FnMut(u32, &mut [u8]) -> Result<(), String> + '_ {
    move |at, out| {
        let start = at as usize;
        let end = start.checked_add(out.len()).ok_or("overflow")?;
        if start == 0 {
            return Err("null".into());
        }
        out.copy_from_slice(bytes.get(start..end).ok_or("unmapped")?);
        Ok(())
    }
}

pub fn targets() -> Vec<Target> {
    let v1_client = protocol::encode(&ClientUpdate {
        version: protocol::VERSION,
        sequence: 7,
        position: position(),
        rotation: rotation(),
    })
    .unwrap();
    let v1_server = protocol::encode(&ServerUpdate {
        version: protocol::VERSION,
        player_id: 2,
        sequence: 7,
        position: position(),
        rotation: rotation(),
    })
    .unwrap();
    let evidence = serde_json::json!({
        "client":{"status":1,"sample_state":1,"sent":100,"received":80,"read_failures":2},
        "draw":{"draw_calls":90,"flush_calls":360,"foreign_calls":0,"flags":0},
        "visual":{"item":123,"shown":1,"drawn":50,"fault":0},
        "network_session":null,"network_timing":null,
        "presentation":{"config":{"delay_ms":100,"extrapolation_ms":200,"timebase":"server"},"revision":1,"buffering":0,"interpolated":50,"predicted":0,"capped":0,"held":0,"snapped":0,"stale":1,"empty_queries":0,"lock_misses":0},
        "remotes":[{"player_id":4,"age_ms":72,"has_rotation":true}],
        "session_events":{"local":"driving","reliability":{"queued":1,"sent":1,"retransmitted":0,"acked":1,"expired":0,"refused":0,"pending":0,"relays":2,"relay_duplicates":0,"pings_sent":4,"pongs":4,"pings_lost":0},"pending_markers":0,"markers_refused":0,"states":[[2,"loading"]],"markers":[[2,7,30]],"truncated":false}
    });
    let stop = serde_json::json!({"client_status":2,"released":true,"files":{"unchanged":8},
        "slots":[{"actual":"0x1000","expected":"0x1000","restored":true}],"preflight_checks":1,"release_attempts":1});
    let mut record = crate::transition_observation::Record {
        kind: 11,
        argument: 64,
        depth: 1,
        ..Default::default()
    };
    record.context[..4].copy_from_slice(&[10, 2, 128, 99]);
    record.readable = 0xf00;
    let transition = serde_json::json!({"summary":{"state":1,"generation":3,"records":1,"owner_thread":7,"flags":1},"records":[record.json()]});
    let mut gate = EngineControl::new(crate::api::LOAD_GATE_STATUS);
    crate::load_gate::Status {
        installed: true,
        reason: "deadline_expired",
        ..Default::default()
    }
    .encode(&mut gate);
    let mut argv = vec![0u8; 64];
    argv[4..8].copy_from_slice(&2u32.to_le_bytes());
    argv[8..12].copy_from_slice(&16u32.to_le_bytes());
    argv[16..20].copy_from_slice(&32u32.to_le_bytes());
    argv[20..24].copy_from_slice(&40u32.to_le_bytes());
    argv[32..36].copy_from_slice(b"map\0");
    argv[40..45].copy_from_slice(b"save\0");
    vec![
        Target {
            name: "v1_client",
            seeds: vec![v1_client],
            run: |b| {
                stable(
                    protocol::decode_client(b),
                    protocol::encode,
                    protocol::decode_client,
                )
            },
        },
        Target {
            name: "v1_server",
            seeds: vec![v1_server],
            run: |b| {
                stable(
                    protocol::decode_server(b),
                    protocol::encode,
                    protocol::decode_server,
                )
            },
        },
        Target {
            name: "v2_client",
            seeds: v2_client_seeds(),
            run: |b| stable(w::decode_client(b), w::encode_client, w::decode_client),
        },
        Target {
            name: "v2_server",
            seeds: v2_server_seeds(),
            run: |b| stable(w::decode_server(b), w::encode_server, w::decode_server),
        },
        Target {
            name: "v2_client_machine",
            seeds: v2_server_seeds(),
            run: |b| {
                // Arbitrary server bytes into a connected client never panic
                // and never surface our own player as a remote.
                let mut c = multiplayer_server::session_client::Client::new(
                    multiplayer_server::session_client::Options {
                        admission: None,
                        room: "lan".into(),
                        identity: "a".repeat(32),
                        profile: w::PROFILE.into(),
                    },
                    1,
                )
                .map_err(|e| e.to_string())?;
                let _ = c.poll(0);
                let accepted = w::encode_server(&ServerFrame::new(S::Accepted {
                    request: 1,
                    credentials: credentials(),
                    player: 2,
                    revision: 1,
                    timeout_ms: 5000,
                    server_ms: 0,
                }))?;
                c.ingest(&accepted, 1);
                let events = c.ingest(b, 2);
                let _ = c.poll(3);
                if events.iter().any(|e| {
                    matches!(e, multiplayer_server::session_client::Event::Pose { pose, .. } if pose.player == 2)
                }) {
                    return Err("own player surfaced as remote".into());
                }
                Ok(if events.is_empty() {
                    Outcome::Rejected
                } else {
                    Outcome::Accepted
                })
            },
        },
        Target {
            name: "session_evidence",
            seeds: vec![
                serde_json::to_vec(&evidence).unwrap(),
                serde_json::to_vec(&stop).unwrap(),
            ],
            run: |b| match crate::session_evidence::parse(b) {
                Ok(capture) => {
                    let _ = crate::session_evidence::sanitize(b).map_err(|e| e.to_string())?;
                    let _ = crate::session_evidence::analyze(vec![capture]);
                    Ok(Outcome::Accepted)
                }
                Err(_) => Ok(Outcome::Rejected),
            },
        },
        Target {
            name: "transition_analysis",
            seeds: vec![serde_json::to_vec(&transition).unwrap()],
            run: |b| {
                Ok(match crate::transition_analysis::analyze(b) {
                    Ok(_) => Outcome::Accepted,
                    Err(_) => Outcome::Rejected,
                })
            },
        },
        Target {
            name: "engine_abi_decoders",
            seeds: vec![block_bytes(&gate)],
            run: |b| {
                let b = block(b);
                let _ = crate::draw_observation::Record::decode(&b).json();
                let _ = crate::draw_observation::Summary::decode(&b);
                let _ = crate::draw_observation::Visual::decode(&b);
                let _ = crate::draw_observation::Remote::decode(&b);
                let _ = crate::render_observation::Record::decode(&b).json();
                let _ = crate::render_observation::Summary::decode(&b);
                let _ = crate::transition_observation::Record::decode(&b).json();
                let _ = crate::transition_observation::Summary::decode(&b);
                let _ = crate::release_policy::Report::decode(&b);
                Ok(match crate::load_gate::Status::decode(&b) {
                    Ok(status) => {
                        let mut again = EngineControl::new(0);
                        status.encode(&mut again);
                        if crate::load_gate::Status::decode(&again).as_ref() != Ok(&status) {
                            return Err("gate status round trip changed".into());
                        }
                        Outcome::Accepted
                    }
                    Err(_) => Outcome::Rejected,
                })
            },
        },
        Target {
            name: "argv_capture",
            seeds: vec![argv],
            run: |b| {
                let word = |at: usize| {
                    b.get(at..at + 4)
                        .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
                        .unwrap_or(0)
                };
                let (argc, argv) = (word(4), word(8));
                let map = crate::deferred_load::OwnedCommand::capture(argc, argv, reader(b));
                let world =
                    crate::deferred_load::OwnedCommand::capture_world(argc, argv, reader(b));
                for c in [&map, &world].into_iter().flatten() {
                    let n = c.with_argv(|n, _| n) as usize;
                    if !(1..=crate::deferred_load::MAX_ARGUMENTS).contains(&n)
                        || (0..n)
                            .map(|i| c.argument(i).map_or(0, <[u8]>::len) + 1)
                            .sum::<usize>()
                            > crate::deferred_load::MAX_ARGUMENT_BYTES
                    {
                        return Err("captured command exceeds its bounds".into());
                    }
                }
                Ok(if map.is_ok() || world.is_ok() {
                    Outcome::Accepted
                } else {
                    Outcome::Rejected
                })
            },
        },
    ]
}

/// Runs every target `iterations` times (seeds included, then mutations).
pub fn run(seed: u64, iterations: u64) -> Report {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let mut rng = Rng::new(seed);
    let mut reports = Vec::new();
    for target in targets() {
        let mut r = TargetReport {
            name: target.name,
            ..Default::default()
        };
        for i in 0..iterations {
            let input = match target.seeds.get(i as usize) {
                Some(seed) => seed.clone(),
                None => {
                    let pick = rng.below(target.seeds.len());
                    mutate(&mut rng, &target.seeds[pick])
                }
            };
            let started = Instant::now();
            let result = panic::catch_unwind(AssertUnwindSafe(|| (target.run)(&input)));
            r.max_micros = r.max_micros.max(started.elapsed().as_micros() as u64);
            r.inputs += 1;
            let failure = match result {
                Ok(Ok(Outcome::Accepted)) => {
                    r.accepted += 1;
                    None
                }
                Ok(Ok(Outcome::Rejected)) => {
                    r.rejected += 1;
                    None
                }
                Ok(Err(message)) => {
                    r.property_failures += 1;
                    Some(message)
                }
                Err(payload) => {
                    r.panics += 1;
                    Some(
                        payload
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "panic".into()),
                    )
                }
            };
            if let Some(message) = failure
                && r.first_failure.is_none()
            {
                r.first_failure =
                    Some(input.iter().take(256).map(|b| format!("{b:02x}")).collect());
                r.first_message = Some(message);
            }
        }
        reports.push(r);
    }
    panic::set_hook(previous);
    let passed = reports
        .iter()
        .all(|r| r.panics == 0 && r.property_failures == 0 && r.accepted > 0);
    Report {
        schema: 1,
        seed,
        iterations_per_target: iterations,
        targets: reports,
        passed,
    }
}

//! Actual DLL's v2 receiver: server stamps, generation and timebase boundaries.
//! Own process and protocol server only; no game, hooks, graphics or assets.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control, EngineControl},
    controller::Controller,
    draw_observation::Remote,
};
use multiplayer_server::{
    protocol::{Position, Rotation},
    session_client::{Options, Status},
    session_protocol::{self as w, ClientMessage as C, ServerMessage as S},
};
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Authority {
    stop: Arc<AtomicBool>,
    task: Option<thread::JoinHandle<()>>,
}
impl Drop for Authority {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.task.take() {
            t.join().unwrap();
        }
    }
}
fn authority() -> (SocketAddr, Arc<AtomicU32>, Authority) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let address = socket.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let command = Arc::new(AtomicU32::new(0));
    let requested = command.clone();
    let task = thread::spawn(move || {
        let started = Instant::now();
        let mut peer = None;
        let mut previous_stamp = 0u64;
        let credentials = w::Credentials {
            session: 11,
            connection: 22,
            token: "a".repeat(32),
        };
        let send = |to, message| {
            socket
                .send_to(
                    &w::encode_server(&w::ServerFrame::new(message)).unwrap(),
                    to,
                )
                .unwrap()
        };
        let mut buffer = [0; 1201];
        while !done.load(Ordering::Acquire) {
            let now = started.elapsed().as_millis() as u64;
            for _ in 0..256 {
                match socket.recv_from(&mut buffer) {
                    Ok((len, from)) => {
                        if let Some(frame) = w::decode_client(&buffer[..len]) {
                            match frame.message {
                                C::Join { request, .. } => {
                                    peer = Some(from);
                                    send(
                                        from,
                                        S::Accepted {
                                            request,
                                            credentials: credentials.clone(),
                                            player: 1,
                                            revision: 1,
                                            timeout_ms: 5000,
                                            server_ms: now,
                                        },
                                    );
                                }
                                C::Ping {
                                    sequence,
                                    client_ms,
                                    ..
                                } => {
                                    send(
                                        from,
                                        S::Pong {
                                            session: 11,
                                            connection: 22,
                                            sequence,
                                            client_ms,
                                            server_ms: now,
                                        },
                                    );
                                }
                                C::Event { stream, id, .. } => {
                                    send(
                                        from,
                                        S::EventAck {
                                            session: 11,
                                            connection: 22,
                                            stream,
                                            id,
                                        },
                                    );
                                }
                                C::Leave { .. } => {
                                    send(
                                        from,
                                        S::Left {
                                            session: 11,
                                            connection: 22,
                                        },
                                    );
                                }
                                _ => (),
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.raw_os_error() == Some(10054) => continue,
                    Err(e) => panic!("own authority: {e}"),
                }
            }
            if let Some(to) = peer {
                let stage = requested.swap(0, Ordering::AcqRel);
                if stage > 0 {
                    let received_ms = if stage == 4 {
                        previous_stamp.saturating_sub(1)
                    } else {
                        now.max(previous_stamp + 1)
                    };
                    previous_stamp = received_ms;
                    let (generation, sequence) = match stage {
                        1 => (1, 100),
                        2..=4 => (2, u64::from(stage - 1)),
                        _ => (3, u64::from(stage - 5)),
                    };
                    send(
                        to,
                        S::Snapshot {
                            session: 11,
                            revision: 1,
                            tick: u64::from(stage),
                            server_ms: now.max(received_ms),
                            poses: vec![w::Pose {
                                player: 7,
                                generation,
                                sequence,
                                received_ms,
                                position: Position {
                                    x: stage as f32 * 10.0,
                                    y: 0.0,
                                    z: 0.0,
                                },
                                rotation: Some(Rotation {
                                    x: 0.0,
                                    y: 0.0,
                                    z: 0.0,
                                    w: 1.0,
                                }),
                            }],
                        },
                    );
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    (
        address,
        command,
        Authority {
            stop,
            task: Some(task),
        },
    )
}
#[test]
fn dll_uses_raw_server_stamps_and_drops_history_at_generation_and_clock_boundaries() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .arg("--ansi")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    BufReader::new(host.0.stdout.take().unwrap())
        .read_line(&mut String::new())
        .unwrap();
    let c = Controller::open(host.0.id()).unwrap();
    c.load(&dll).unwrap();
    let tune = |clock: &str| {
        let mut b = Control::new(api::PRESENTATION_SET);
        api::put(
            &mut b.message,
            &format!("{{\"delay_ms\":100,\"extrapolation_ms\":200,\"timebase\":\"{clock}\"}}"),
        )
        .unwrap();
        c.control(&dll, &mut b).unwrap();
    };
    tune("server");
    let (server, pulse, _authority) = authority();
    let mut start = Control::new(api::START_SESSION);
    start.config.mode = api::SIMULATED;
    api::put(&mut start.config.server, &server.to_string()).unwrap();
    api::put(&mut start.config.profile, w::PROFILE).unwrap();
    api::put(
        &mut start.message,
        &serde_json::to_string(&Options {
            admission: None,
            room: "lan".into(),
            identity: "b".repeat(32),
            profile: w::PROFILE.into(),
        })
        .unwrap(),
    )
    .unwrap();
    c.control(&dll, &mut start).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut status = Control::new(api::SESSION_STATUS);
        c.control(&dll, &mut status).unwrap();
        let status: Option<Status> =
            serde_json::from_str(api::text(&status.message).unwrap()).unwrap();
        if status.is_some_and(|s| s.clock_offset_ms.is_some()) {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let receive = |stage, sequence| {
        pulse.store(stage, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let mut b = EngineControl::new(api::REMOTE_LIST);
            c.engine_control(&dll, &mut b).unwrap();
            let r = Remote::decode(&b);
            if r.count == 1 && r.sequence == sequence {
                break r;
            }
            assert!(Instant::now() < deadline, "stage {stage}: {r:?}");
            thread::sleep(Duration::from_millis(10));
        }
    };
    receive(1, 100); // Establish local presentation epoch before testing server mapping.
    thread::sleep(Duration::from_millis(100));
    assert_eq!(receive(2, 1).samples, 1);
    assert_eq!(receive(3, 2).samples, 2);
    assert_eq!(
        receive(4, 3).samples,
        1,
        "regressing raw server stamp restarted history"
    );
    let mut stats = Control::new(api::PRESENTATION_STATUS);
    c.control(&dll, &mut stats).unwrap();
    let stats: client_dll::presentation::Status =
        serde_json::from_str(api::text(&stats.message).unwrap()).unwrap();
    assert!(stats.server_timed >= 3);
    assert_eq!(
        receive(5, 0).samples,
        1,
        "generation reset also reset clock anchor"
    );
    tune("arrival");
    assert_eq!(receive(6, 1).samples, 1);
    assert_eq!(receive(7, 2).samples, 2);
    tune("server");
    assert_eq!(receive(8, 3).samples, 1);
    assert_eq!(receive(9, 4).samples, 2);
    c.control(&dll, &mut Control::new(api::STOP)).unwrap();
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

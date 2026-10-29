//! Real x86 DLL, own memory fixture and v2 authority over real loopback UDP.
//! No game process, render hooks or commercial data.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control, EngineControl},
    controller::Controller,
    draw_observation::Remote,
};
use multiplayer_server::{
    identity,
    protocol::Position,
    session_client::{Client, Options, Phase, Status},
    session_hub::{Hub, Settings},
    session_protocol as w,
};
use std::{
    io::{self, BufRead, BufReader, Write},
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Runner {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Runner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
struct Server {
    address: SocketAddr,
    restart: Arc<AtomicBool>,
    _runner: Runner,
}
fn settings() -> Settings {
    Settings {
        room: "lan".into(),
        profile: w::PROFILE.into(),
        max_clients: 4,
        timeout_ms: 2000,
        echo: false,
    }
}
fn server() -> Server {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let address = socket.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let restart = Arc::new(AtomicBool::new(false));
    let restarted = restart.clone();
    let thread = thread::spawn(move || {
        let start = Instant::now();
        let mut hub = Hub::new(settings(), 11).unwrap();
        let mut buffer = [0; 1201];
        let mut tick = 0;
        while !stopped.load(Ordering::Acquire) {
            let now = start.elapsed().as_millis() as u64;
            if restarted.swap(false, Ordering::AcqRel) {
                hub = Hub::new(settings(), 22).unwrap();
            }
            for _ in 0..256 {
                match socket.recv_from(&mut buffer) {
                    Ok((len, address)) => {
                        for (destination, frame) in hub
                            .ingest(address, &buffer[..len], now, identity::random_id)
                            .unwrap()
                        {
                            let _ = socket.send_to(&w::encode_server(&frame).unwrap(), destination);
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.raw_os_error() == Some(10054) => continue,
                    Err(e) => panic!("own authority socket: {e}"),
                }
            }
            if now >= tick {
                for (destination, frame) in hub.tick(now) {
                    let _ = socket.send_to(&w::encode_server(&frame).unwrap(), destination);
                }
                tick = now + 16;
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    Server {
        address,
        restart,
        _runner: Runner {
            stop,
            thread: Some(thread),
        },
    }
}
struct Peer {
    reconnect: Arc<AtomicBool>,
    accepted: Arc<AtomicU64>,
    session: Arc<AtomicU64>,
    runner: Runner,
}
fn peer(server: SocketAddr) -> Peer {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let reconnect = Arc::new(AtomicBool::new(false));
    let requested = reconnect.clone();
    let accepted = Arc::new(AtomicU64::new(0));
    let accepts = accepted.clone();
    let session = Arc::new(AtomicU64::new(0));
    let current = session.clone();
    let thread = thread::spawn(move || {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(server).unwrap();
        socket.set_nonblocking(true).unwrap();
        let mut client = Client::new(
            Options {
                room: "lan".into(),
                identity: "b".repeat(32),
                profile: w::PROFILE.into(),
            },
            99,
        )
        .unwrap();
        let start = Instant::now();
        let mut buffer = [0; 1201];
        while !stopped.load(Ordering::Acquire) {
            let now = start.elapsed().as_millis() as u64;
            if requested.swap(false, Ordering::AcqRel) {
                client.request_reconnect();
            }
            let mut output = client.poll(now).0;
            if let Some(frame) = client.pose(
                Position {
                    x: 1000.0,
                    y: 0.0,
                    z: 0.0,
                },
                None,
                now,
            ) {
                output.push(frame);
            }
            for frame in output {
                socket.send(&w::encode_client(&frame).unwrap()).unwrap();
            }
            for _ in 0..256 {
                match socket.recv(&mut buffer) {
                    Ok(len) => {
                        client.ingest(&buffer[..len], start.elapsed().as_millis() as u64);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("own peer socket: {e}"),
                }
            }
            accepts.store(client.status().accepted, Ordering::Release);
            current.store(client.status().session, Ordering::Release);
            thread::sleep(Duration::from_millis(16));
        }
        client.close(start.elapsed().as_millis() as u64);
        let end = Instant::now();
        while end.elapsed() < Duration::from_millis(220) && client.status().phase != Phase::Closed {
            for frame in client.poll(start.elapsed().as_millis() as u64).0 {
                let _ = socket.send(&w::encode_client(&frame).unwrap());
            }
            if let Ok(len) = socket.recv(&mut buffer) {
                client.ingest(&buffer[..len], start.elapsed().as_millis() as u64);
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    Peer {
        reconnect,
        accepted,
        session,
        runner: Runner {
            stop,
            thread: Some(thread),
        },
    }
}
fn control(c: &Controller, dll: &Path, command: u32) -> Control {
    let mut b = Control::new(command);
    c.control(dll, &mut b).unwrap();
    b
}
fn status(c: &Controller, dll: &Path) -> Option<Status> {
    let b = control(c, dll, api::SESSION_STATUS);
    serde_json::from_str(api::text(&b.message).unwrap()).unwrap()
}
fn remote(c: &Controller, dll: &Path) -> Remote {
    let mut b = EngineControl::new(api::REMOTE_LIST);
    c.engine_control(dll, &mut b).unwrap();
    Remote::decode(&b)
}
fn until(
    c: &Controller,
    dll: &Path,
    what: &str,
    done: impl Fn(&Control, &Status, &Remote) -> bool,
) -> (Status, Remote) {
    let end = Instant::now() + Duration::from_secs(9);
    loop {
        let state = control(c, dll, api::STATUS);
        assert_eq!(
            state.status,
            1,
            "{what}: {}",
            api::text(&state.message).unwrap()
        );
        if let Some(session) = status(c, dll) {
            let row = remote(c, dll);
            if done(&state, &session, &row) {
                return (session, row);
            }
            assert!(Instant::now() < end, "{what}: {session:?}, {row:?}");
        } else {
            assert!(Instant::now() < end, "session machine not published");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn dll_heartbeats_without_coordinates_restarts_histories_recovers_server_and_removes_leavers() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut fixture = Fixture(
        Command::new(env!("CARGO_BIN_EXE_memory_tool"))
            .arg("fixture")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut output = BufReader::new(fixture.0.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).unwrap();
    let c = Controller::open(fixture.0.id()).unwrap();
    c.load(&dll).unwrap();
    let server = server();
    let peer = peer(server.address);
    let mut start = Control::new(api::START_SESSION);
    start.config.mode = api::MEMORY;
    api::put(&mut start.config.server, &server.address.to_string()).unwrap();
    api::put(&mut start.config.profile, w::PROFILE).unwrap();
    for i in 0..3 {
        api::put(
            &mut start.config.addresses[i],
            info["addresses"][i].as_str().unwrap(),
        )
        .unwrap();
    }
    api::put(
        &mut start.message,
        &serde_json::to_string(&Options {
            room: "lan".into(),
            identity: "a".repeat(32),
            profile: w::PROFILE.into(),
        })
        .unwrap(),
    )
    .unwrap();
    c.control(&dll, &mut start).unwrap();
    let (initial, row) = until(&c, &dll, "room connected", |_, s, r| {
        s.phase == Phase::Connected && r.count == 1 && r.sequence >= 50
    });
    assert_eq!(row.position[0], 1000.0);
    assert!(row.samples <= 16);
    writeln!(fixture.0.stdin.as_mut().unwrap(), "unavailable").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "unavailable");
    let waiting = Instant::now();
    while waiting.elapsed() < Duration::from_millis(2300) {
        let state = control(&c, &dll, api::STATUS);
        assert_eq!(state.status, 1);
        assert_eq!(status(&c, &dll).unwrap().phase, Phase::Connected);
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(control(&c, &dll, api::STATUS).sample_state, 2);
    assert_eq!(
        status(&c, &dll).unwrap().connection,
        initial.connection,
        "heartbeat must keep the original connection alive"
    );
    let old = remote(&c, &dll).sequence;
    peer.reconnect.store(true, Ordering::Release);
    let (_, new) = until(&c, &dll, "remote generation reset", |_, _, r| {
        peer.accepted.load(Ordering::Acquire) >= 2 && r.count == 1 && r.sequence < old
    });
    assert!(
        new.samples <= new.sequence + 1,
        "new generation retained old samples: {new:?}"
    );
    server.restart.store(true, Ordering::Release);
    let (recovered, row) = until(&c, &dll, "authority restart", |_, s, r| {
        s.phase == Phase::Connected
            && s.session == 22
            && r.count == 1
            && peer.session.load(Ordering::Acquire) == 22
    });
    assert!(recovered.reconnects > 0);
    assert_eq!(row.position[0], 1000.0);
    writeln!(fixture.0.stdin.as_mut().unwrap(), "set 7.25 -2.5 30").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "updated");
    until(&c, &dll, "coordinates resumed", |state, _, _| {
        state.sample_state == 1
    });
    drop(peer.runner);
    until(&c, &dll, "leaver removed by roster", |_, s, r| {
        s.phase == Phase::Connected && r.count == 0
    });
    let stopped = control(&c, &dll, api::STOP);
    assert_eq!(stopped.status, 2);
    let closed = status(&c, &dll).unwrap();
    assert_eq!(closed.phase, Phase::Closed);
    assert!(closed.leave_confirmed);
    c.unload(&dll).unwrap();
    assert!(c.control(&dll, &mut Control::new(api::STATUS)).is_err());
}

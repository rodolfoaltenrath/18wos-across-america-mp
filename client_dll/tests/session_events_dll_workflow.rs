//! Real x86 DLL with reliable v2 events over loopback UDP: markers from the
//! Control ABI, debounced local state and the bounded events report. Own
//! memory fixture and authority only; no game process or render hooks.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control},
    controller::Controller,
    session_events::EventsReport,
};
use multiplayer_server::{
    identity,
    protocol::Position,
    session_client::{Client, Event, Options, Phase},
    session_hub::{Hub, Settings},
    session_protocol::{self as w, EventBody, PlayerState},
};
use std::{
    io::{self, BufRead, BufReader, Write},
    net::UdpSocket,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
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
struct Runner(Arc<AtomicBool>, Option<JoinHandle<()>>);
impl Drop for Runner {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        if let Some(t) = self.1.take() {
            let _ = t.join();
        }
    }
}
fn authority() -> (std::net::SocketAddr, Runner) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_nonblocking(true).unwrap();
    let address = socket.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = thread::spawn(move || {
        let start = Instant::now();
        let mut hub = Hub::new(
            Settings {
                room: "lan".into(),
                profile: w::PROFILE.into(),
                max_clients: 4,
                timeout_ms: 2000,
                echo: false,
            },
            31,
        )
        .unwrap();
        let mut buffer = [0; 1201];
        let mut tick = 0;
        while !stopped.load(Ordering::Acquire) {
            let now = start.elapsed().as_millis() as u64;
            for _ in 0..256 {
                match socket.recv_from(&mut buffer) {
                    Ok((len, from)) => {
                        for (to, frame) in hub
                            .ingest(from, &buffer[..len], now, identity::random_id)
                            .unwrap()
                        {
                            let _ = socket.send_to(&w::encode_server(&frame).unwrap(), to);
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) if e.raw_os_error() == Some(10054) => continue,
                    Err(e) => panic!("own authority socket: {e}"),
                }
            }
            if now >= tick {
                for (to, frame) in hub.tick(now) {
                    let _ = socket.send_to(&w::encode_server(&frame).unwrap(), to);
                }
                tick = now + 16;
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    (address, Runner(stop, Some(thread)))
}
type Received = Arc<Mutex<Vec<(u32, EventBody)>>>;
/// Pure client peer: sends poses, records relays and queues marker 77 on request.
fn peer(server: std::net::SocketAddr) -> (Received, Arc<AtomicBool>, Arc<AtomicU32>, Runner) {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let received: Received = Arc::new(Mutex::new(Vec::new()));
    let log = received.clone();
    let send_marker = Arc::new(AtomicBool::new(false));
    let marker = send_marker.clone();
    let player = Arc::new(AtomicU32::new(0));
    let me = player.clone();
    let thread = thread::spawn(move || {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.connect(server).unwrap();
        socket.set_nonblocking(true).unwrap();
        let mut client = Client::new(
            Options {
                admission: None,
                room: "lan".into(),
                identity: "b".repeat(32),
                profile: w::PROFILE.into(),
            },
            98,
        )
        .unwrap();
        let start = Instant::now();
        let mut buffer = [0; 1201];
        while !stopped.load(Ordering::Acquire) {
            let now = start.elapsed().as_millis() as u64;
            if client.connected() && marker.swap(false, Ordering::AcqRel) {
                client
                    .event(EventBody::Marker { code: 77 }, 10_000, now)
                    .unwrap();
            }
            let mut output = client.poll(now).0;
            output.extend(client.pose(
                Position {
                    x: 1000.0,
                    y: 0.0,
                    z: 0.0,
                },
                None,
                now,
            ));
            for frame in output {
                socket.send(&w::encode_client(&frame).unwrap()).unwrap();
            }
            for _ in 0..256 {
                match socket.recv(&mut buffer) {
                    Ok(len) => {
                        for event in
                            client.ingest(&buffer[..len], start.elapsed().as_millis() as u64)
                        {
                            if let Event::Relay { origin, body, .. } = event {
                                log.lock().unwrap().push((origin, body));
                            }
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("own peer socket: {e}"),
                }
            }
            me.store(client.status().player, Ordering::Release);
            thread::sleep(Duration::from_millis(16));
        }
    });
    (received, send_marker, player, Runner(stop, Some(thread)))
}
fn control(c: &Controller, dll: &Path, command: u32, message: Option<&str>) -> Control {
    let mut b = Control::new(command);
    if let Some(m) = message {
        api::put(&mut b.message, m).unwrap();
    }
    let _ = c.control(dll, &mut b);
    b
}
fn events(c: &Controller, dll: &Path) -> EventsReport {
    let b = control(c, dll, api::SESSION_EVENTS, None);
    assert_eq!(b.result, 0);
    let report: EventsReport = serde_json::from_str(api::text(&b.message).unwrap()).unwrap();
    report.validate().unwrap();
    report
}
fn wait(what: &str, mut done: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(9);
    while !done() {
        assert!(Instant::now() < end, "{what}");
        thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn dll_relays_markers_and_debounced_states_and_reports_peer_events() {
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
    // Without a session the marker is refused and nothing is queued.
    let refused = control(&c, &dll, api::SESSION_MARKER, Some(r#"{"code":1}"#));
    assert_eq!(refused.result, 1);
    let (server, _authority) = authority();
    let (received, send_marker, peer_player, _peer) = peer(server);
    let mut start = Control::new(api::START_SESSION);
    start.config.mode = api::MEMORY;
    api::put(&mut start.config.server, &server.to_string()).unwrap();
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
            admission: None,
            room: "lan".into(),
            identity: "a".repeat(32),
            profile: w::PROFILE.into(),
        })
        .unwrap(),
    )
    .unwrap();
    c.control(&dll, &mut start).unwrap();
    let mine = || {
        let b = control(&c, &dll, api::SESSION_STATUS, None);
        let s: Option<multiplayer_server::session_client::Status> =
            serde_json::from_str(api::text(&b.message).unwrap()).unwrap();
        s.filter(|s| s.phase == Phase::Connected).map(|s| s.player)
    };
    wait("connected", || {
        mine().is_some() && peer_player.load(Ordering::Acquire) != 0
    });
    let me = mine().unwrap();
    let has = |body: EventBody| received.lock().unwrap().contains(&(me, body));
    wait("driving state relayed", || {
        has(EventBody::State {
            state: PlayerState::Driving,
        })
    });
    // Invalid payloads are refused; a valid marker returns the report.
    assert_eq!(
        control(
            &c,
            &dll,
            api::SESSION_MARKER,
            Some(r#"{"code":1,"note":"x"}"#)
        )
        .result,
        1
    );
    let queued = control(&c, &dll, api::SESSION_MARKER, Some(r#"{"code":5}"#));
    assert_eq!(queued.result, 0);
    let report: EventsReport = serde_json::from_str(api::text(&queued.message).unwrap()).unwrap();
    report.validate().unwrap();
    wait("marker relayed", || has(EventBody::Marker { code: 5 }));
    // Coordinates unavailable: the state follows after the debounce.
    writeln!(fixture.0.stdin.as_mut().unwrap(), "unavailable").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "unavailable");
    wait("unavailable state relayed", || {
        has(EventBody::State {
            state: PlayerState::Unavailable,
        })
    });
    send_marker.store(true, Ordering::Release);
    let other = peer_player.load(Ordering::Acquire);
    wait("peer marker reported by the DLL", || {
        events(&c, &dll)
            .markers
            .iter()
            .any(|(origin, code, _)| *origin == other && *code == 77)
    });
    let report = events(&c, &dll);
    assert_eq!(report.local, Some(PlayerState::Unavailable));
    assert!(report.reliability.acked >= 3, "{report:?}");
    assert_eq!(report.reliability.expired, 0);
    assert!(!report.truncated);
    // Every relayed body arrived once per event.
    let log = received.lock().unwrap().clone();
    assert_eq!(
        log.iter()
            .filter(|(o, b)| *o == me && *b == EventBody::Marker { code: 5 })
            .count(),
        1
    );
    assert_eq!(control(&c, &dll, api::STOP, None).status, 2);
    // After stop the report remains readable and markers are refused again.
    assert_eq!(events(&c, &dll).local, Some(PlayerState::Unavailable));
    assert_eq!(
        control(&c, &dll, api::SESSION_MARKER, Some(r#"{"code":6}"#)).result,
        1
    );
    c.lifecycle(&dll).unwrap().require_unload().unwrap();
    c.unload(&dll).unwrap();
}

//! Real x86 DLL, own draw fixture and own v2 authority over loopback UDP: a
//! remote that announces `loading` and stops sending is hidden well before
//! the 1 s pose expiry, and drawn again when it resumes. No game process.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control, EngineControl},
    controller::Controller,
    draw_observation::Visual,
    presentation::Status as Presentation,
};
use multiplayer_server::{
    identity,
    protocol::{Position, Rotation},
    session_client::{Client, Options, Phase},
    session_hub::{Hub, Settings},
    session_protocol::{self as w, EventBody, PlayerState},
};
use std::{
    io::{self, BufRead, BufReader, Write},
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Host(Child);
impl Drop for Host {
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
fn authority() -> (SocketAddr, Runner) {
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
                timeout_ms: 5000,
                echo: false,
            },
            41,
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
/// Pure client peer: sends poses while `moving`; queues a state on change.
struct Peer {
    moving: Arc<AtomicBool>,
    state: Arc<AtomicU32>,
    player: Arc<AtomicU32>,
    _runner: Runner,
}
fn peer(server: SocketAddr) -> Peer {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let moving = Arc::new(AtomicBool::new(true));
    let move_flag = moving.clone();
    let state = Arc::new(AtomicU32::new(0));
    let state_flag = state.clone();
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
            97,
        )
        .unwrap();
        let start = Instant::now();
        let mut buffer = [0; 1201];
        let mut announced = 0;
        while !stopped.load(Ordering::Acquire) {
            let now = start.elapsed().as_millis() as u64;
            let wanted = state_flag.load(Ordering::Acquire);
            if client.connected() && wanted != announced {
                let state = match wanted {
                    1 => PlayerState::Driving,
                    2 => PlayerState::Loading,
                    _ => PlayerState::Unavailable,
                };
                client.event(EventBody::State { state }, 5000, now).unwrap();
                announced = wanted;
            }
            let mut output = client.poll(now).0;
            if move_flag.load(Ordering::Acquire) {
                output.extend(client.pose(
                    Position {
                        x: 100.0 + now as f32 * 0.01,
                        y: 0.0,
                        z: 0.0,
                    },
                    Some(Rotation {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: 1.0,
                    }),
                    now,
                ));
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
            if client.status().phase == Phase::Connected {
                me.store(client.status().player, Ordering::Release);
            }
            thread::sleep(Duration::from_millis(16));
        }
    });
    Peer {
        moving,
        state,
        player,
        _runner: Runner(stop, Some(thread)),
    }
}
fn send(host: &mut Host, output: &mut impl BufRead, command: &str) -> String {
    writeln!(host.0.stdin.as_mut().unwrap(), "{command}").unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    line.trim().into()
}
fn visual(c: &Controller, dll: &Path) -> Visual {
    let mut b = EngineControl::new(api::VISUAL_STATUS);
    c.engine_control(dll, &mut b).unwrap();
    Visual::decode(&b)
}
fn presentation(c: &Controller, dll: &Path) -> Presentation {
    let mut b = Control::new(api::PRESENTATION_STATUS);
    c.control(dll, &mut b).unwrap();
    serde_json::from_str(api::text(&b.message).unwrap()).unwrap()
}
/// Drawn count still advancing over a short window.
fn drawing(c: &Controller, dll: &Path) -> bool {
    let before = visual(c, dll).drawn;
    thread::sleep(Duration::from_millis(120));
    visual(c, dll).drawn > before
}
fn wait(what: &str, limit: Duration, mut done: impl FnMut() -> bool) -> Duration {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < limit, "{what}");
        thread::sleep(Duration::from_millis(10));
    }
    started.elapsed()
}
#[test]
fn quiet_remote_announced_as_loading_is_hidden_before_expiry_and_returns() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .args(["--ansi", "--peek", "--render", "--draw"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut output = BufReader::new(host.0.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).unwrap();
    let words = |key: &str, i: usize| info[key][i].as_u64().unwrap() as u32;
    let c = Controller::open(host.0.id()).unwrap();
    c.load(&dll).unwrap();
    let mut b = EngineControl::new(api::UI_ATTACH);
    b.window = info["window"].as_u64().unwrap() as u32;
    c.engine_control(&dll, &mut b).unwrap();
    b = EngineControl::new(api::UI_PUMP_FIXTURE);
    b.entity = info["peek_slot"].as_u64().unwrap() as u32;
    c.engine_control(&dll, &mut b).unwrap();
    for (command, key) in [
        (api::UI_RENDER_FIXTURE, "render_descriptors"),
        (api::UI_DRAW_FIXTURE, "draw_descriptors"),
    ] {
        b = EngineControl::new(command);
        b.fixture = std::array::from_fn(|i| words(key, i));
        c.engine_control(&dll, &mut b).unwrap();
    }
    assert_eq!(send(&mut host, &mut output, "draw-shallow"), "armed");
    let (server, _authority) = authority();
    let remote = peer(server);
    let mut start = Control::new(api::START_SESSION);
    start.config.mode = api::DRAWN;
    api::put(&mut start.config.server, &server.to_string()).unwrap();
    api::put(&mut start.config.profile, w::PROFILE).unwrap();
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
    wait("peer connected", Duration::from_secs(9), || {
        remote.player.load(Ordering::Acquire) != 0
    });
    remote.state.store(1, Ordering::Release);
    let mut show = EngineControl::new(api::VISUAL_SHOW_FIXTURE);
    show.fixture = std::array::from_fn(|i| words("instance_descriptors", i));
    for i in 0..8 {
        show.frames[i] = words("visual_descriptors", i);
    }
    api::put(&mut show.name, "models/fixture/truck.psm").unwrap();
    show.entity = remote.player.load(Ordering::Acquire);
    c.engine_control(&dll, &mut show).unwrap();
    wait("remote drawn", Duration::from_secs(9), || drawing(&c, &dll));
    assert_eq!(presentation(&c, &dll).state_hidden, 0);

    // Loading + no more poses: hidden after the quiet period, not after 1 s.
    remote.state.store(2, Ordering::Release);
    remote.moving.store(false, Ordering::Release);
    let hidden_after = wait("hidden while loading", Duration::from_secs(3), || {
        presentation(&c, &dll).state_hidden > 0 && !drawing(&c, &dll)
    });
    assert!(
        hidden_after < Duration::from_millis(900),
        "took {hidden_after:?}, the 1 s expiry would have done the same"
    );
    // Resumed poses and driving state: drawn again.
    remote.moving.store(true, Ordering::Release);
    remote.state.store(1, Ordering::Release);
    wait("drawn again", Duration::from_secs(5), || drawing(&c, &dll));
    // A driving remote with fresh poses is never hidden by the rule.
    let hidden = presentation(&c, &dll).state_hidden;
    thread::sleep(Duration::from_millis(300));
    assert_eq!(presentation(&c, &dll).state_hidden, hidden);

    // Release the instance safely before stopping, like session-stop.
    let mut hide = EngineControl::new(api::VISUAL_HIDE);
    c.engine_control(&dll, &mut hide).unwrap();
    wait("visual released", Duration::from_secs(5), || {
        let mut release = EngineControl::new(api::VISUAL_RELEASE);
        let _ = c.engine_control(&dll, &mut release);
        visual(&c, &dll).state == 0
    });
    // Same order as session-stop: remove the probes, then stop the worker.
    b = EngineControl::new(api::UI_PUMP_REMOVE);
    c.engine_control(&dll, &mut b).unwrap();
    let mut stop = Control::new(api::STOP);
    c.control(&dll, &mut stop).unwrap();
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

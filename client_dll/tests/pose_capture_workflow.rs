//! Actual x86 worker, own physics-shaped memory and UDP; no engine hooks/game.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control},
    controller::Controller,
};
use multiplayer_server::protocol;
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn state(c: &Controller, dll: &std::path::Path) -> Control {
    let mut b = Control::new(api::STATUS);
    c.control(dll, &mut b).unwrap();
    b
}
#[test]
fn pose_worker_continues_without_draw_and_recovers_invalid_body_and_actor_change() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .args(["--ansi", "--draw", "--peek"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut output = BufReader::new(host.0.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).unwrap();
    let pid = host.0.id();
    let mut send = |text: &str| {
        writeln!(host.0.stdin.as_mut().unwrap(), "{text}").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "armed");
    };
    let c = Controller::open(pid).unwrap();
    c.load(&dll).unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut start = Control::new(api::START);
    start.config.mode = api::POSE_FIXTURE;
    api::put(
        &mut start.config.server,
        &socket.local_addr().unwrap().to_string(),
    )
    .unwrap();
    for i in 0..2 {
        api::put(
            &mut start.config.addresses[i],
            &format!("{:x}", info["capture_descriptors"][i].as_u64().unwrap()),
        )
        .unwrap();
    }
    c.control(&dll, &mut start).unwrap();
    let mut bytes = [0; protocol::MAX_PACKET];
    let (n, _) = socket.recv_from(&mut bytes).unwrap();
    let first = protocol::decode_client(&bytes[..n]).unwrap();
    assert_eq!(first.rotation.unwrap().w, 1.);
    assert_eq!(first.position.y, 2.);
    send("draw-suspend");
    std::thread::sleep(Duration::from_millis(700));
    let running = state(&c, &dll);
    assert_eq!(running.sample_state, 1);
    assert!(running.sent > 20);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let (n, _) = socket.recv_from(&mut bytes).unwrap();
        let p = protocol::decode_client(&bytes[..n]).unwrap();
        if p.sequence > first.sequence + 20 {
            assert!(p.position.x > first.position.x + 5.);
            assert!(p.rotation.is_some());
            break;
        }
        assert!(Instant::now() < deadline);
    }
    send("draw-invalid");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if state(&c, &dll).sample_state == 2 {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let paused = state(&c, &dll);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(state(&c, &dll).sent, paused.sent);
    send("draw-valid");
    send("draw-respawn");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let s = state(&c, &dll);
        if s.sample_state == 1 && s.sent > paused.sent {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    c.unload(&dll).unwrap();
    assert!(c.control(&dll, &mut Control::new(api::STATUS)).is_err());
}

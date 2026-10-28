#![cfg(all(windows, target_pointer_width = "32"))]
use client_dll::{
    api::{self, Control},
    controller::Controller,
};
use multiplayer_server::protocol::{self, ServerUpdate};
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn control(controller: &Controller, dll: &std::path::Path, command: u32) -> Control {
    let mut block = Control::new(command);
    controller.control(dll, &mut block).unwrap();
    block
}
#[test]
fn loads_configures_captures_recovers_stops_restarts_and_unloads_real_dll() {
    let dll = std::env::var_os("AA_MP_TEST_DLL")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        })
        .canonicalize()
        .expect("build release DLL before running controller test");
    let mut fixture = Fixture(
        Command::new(
            PathBuf::from(env!("CARGO_BIN_EXE_memory_tool"))
                .canonicalize()
                .unwrap(),
        )
        .arg("fixture")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap(),
    );
    let mut stdout = BufReader::new(fixture.0.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        info["pid"].as_u64().unwrap(),
        u64::from(fixture.0.id()),
        "fixture PID differs from child handle"
    );
    let controller = Controller::open(fixture.0.id()).unwrap();
    controller.load(&dll).unwrap();
    controller.load(&dll).unwrap(); // no extra LoadLibrary reference
    assert_eq!(control(&controller, &dll, api::STATUS).status, 0); // DllMain starts nothing
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut start = Control::new(api::START);
    start.config.mode = api::MEMORY;
    api::put(
        &mut start.config.server,
        &receiver.local_addr().unwrap().to_string(),
    )
    .unwrap();
    for i in 0..3 {
        api::put(
            &mut start.config.addresses[i],
            info["addresses"][i].as_str().unwrap(),
        )
        .unwrap();
    }
    let mut invalid = start;
    invalid.version += 1;
    assert!(controller.control(&dll, &mut invalid).is_err());
    invalid = start;
    invalid.size -= 8;
    assert!(controller.control(&dll, &mut invalid).is_err());
    invalid = start;
    invalid.config.version += 1;
    assert!(controller.control(&dll, &mut invalid).is_err());
    assert_eq!(control(&controller, &dll, api::STATUS).status, 0);
    controller.control(&dll, &mut start).unwrap();
    let mut duplicate = start;
    assert!(controller.control(&dll, &mut duplicate).is_err());
    let mut bytes = [0; protocol::MAX_PACKET];
    let (len, addr) = receiver.recv_from(&mut bytes).unwrap();
    let packet = protocol::decode_client(&bytes[..len]).unwrap();
    assert_eq!(
        packet.position,
        protocol::Position {
            x: 1.25,
            y: -2.5,
            z: 3.75
        }
    );
    let remote = protocol::encode(&ServerUpdate {
        version: protocol::VERSION,
        player_id: 7,
        sequence: 10,
        position: packet.position,
        rotation: None,
    })
    .unwrap();
    receiver.send_to(&remote, addr).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while control(&controller, &dll, api::STATUS).received == 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    writeln!(fixture.0.stdin.as_mut().unwrap(), "set 7.25 -2.5 30").unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "updated");
    loop {
        let (len, _) = receiver.recv_from(&mut bytes).unwrap();
        if protocol::decode_client(&bytes[..len]).unwrap().position.x == 7.25 {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
    }
    writeln!(fixture.0.stdin.as_mut().unwrap(), "unavailable").unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "unavailable");
    std::thread::sleep(Duration::from_millis(80));
    let paused = control(&controller, &dll, api::STATUS);
    assert_eq!(paused.status, 1);
    assert_eq!(paused.sample_state, 2);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(control(&controller, &dll, api::STATUS).sent, paused.sent);
    writeln!(fixture.0.stdin.as_mut().unwrap(), "set 8.25 -2.5 40").unwrap();
    line.clear();
    stdout.read_line(&mut line).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    let resumed = control(&controller, &dll, api::STATUS);
    assert_eq!(resumed.sample_state, 1);
    assert!(resumed.sent > paused.sent);
    assert_eq!(control(&controller, &dll, api::STOP).status, 2);
    // A temporary invalid coordinate must keep the worker alive and emit no invented state.
    let mut unavailable = start;
    api::put(&mut unavailable.config.addresses[0], "1").unwrap();
    controller.control(&dll, &mut unavailable).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    let waiting = control(&controller, &dll, api::STATUS);
    assert_eq!(waiting.status, 1);
    assert_eq!(waiting.sample_state, 2);
    assert_eq!(waiting.sent, 0);
    assert!(waiting.read_failures > 0);
    control(&controller, &dll, api::STOP);
    // Failed sample sessions can be stopped and restarted with a valid configuration.
    controller.control(&dll, &mut start).unwrap();
    std::thread::sleep(Duration::from_millis(80));
    assert!(control(&controller, &dll, api::STATUS).sent > 0);
    controller.unload(&dll).unwrap();
    assert!(
        controller
            .control(&dll, &mut Control::new(api::STATUS))
            .is_err()
    );
    controller.load(&dll).unwrap();
    assert_eq!(control(&controller, &dll, api::STATUS).status, 0);
    controller.unload(&dll).unwrap();
}

//! Presentation discontinuities, not physical velocity or a game-state matrix.
use client_dll::interpolation::{Config, HISTORY, History, ReceiveHistory, Sample, Timebase};

fn sample(at: f64, x: f32) -> Sample {
    Sample {
        at,
        position: [x, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
    }
}
#[test]
fn stale_return_starts_at_received_pose_instead_of_replaying_the_gap() {
    let mut h = ReceiveHistory::default();
    h.receive(sample(1.0, 0.0), 1.0, Timebase::Arrival);
    h.receive(sample(1.1, 100.0), 1.1, Timebase::Arrival);
    h.receive(sample(3.0, 500.0), 3.0, Timebase::Arrival);
    assert_eq!(h.len(), 1);
    let pose = h.evaluate(3.0, Config::DEFAULT).unwrap();
    assert_eq!(pose.position[0], 500.0);
    assert_eq!(pose.prediction_ms, 0);
    h.receive(sample(3.1, 600.0), 3.1, Timebase::Arrival);
    assert!((h.evaluate(3.15, Config::DEFAULT).unwrap().position[0] - 550.0).abs() < 0.01);
}
#[test]
fn staleness_uses_arrival_not_a_future_server_timestamp_and_includes_boundary() {
    for (gap, expected) in [(0.999, 2), (1.0, 1), (1.001, 1)] {
        let mut h = ReceiveHistory::default();
        h.receive(sample(1.25, 0.0), 1.0, Timebase::Server);
        h.receive(sample(1.25 + gap, 100.0), 1.0 + gap, Timebase::Server);
        assert_eq!(h.len(), expected, "receive gap {gap}");
    }
}
#[test]
fn changing_timestamp_source_restarts_in_both_directions_even_during_a_burst() {
    for (from, to) in [
        (Timebase::Arrival, Timebase::Server),
        (Timebase::Server, Timebase::Arrival),
    ] {
        let mut h = ReceiveHistory::default();
        h.receive(sample(1.2, 0.0), 1.0, from);
        h.receive(sample(1.21, 10.0), 1.01, from);
        // The new time is behind the old mapping. Clamping it to 1.21 would
        // retain incompatible samples and mask the source switch.
        h.receive(sample(1.02, 20.0), 1.02, to);
        assert_eq!(h.len(), 1);
        assert_eq!(h.newest().unwrap().at, 1.02);
        assert_eq!(h.evaluate(1.02, Config::DEFAULT).unwrap().position[0], 20.0);
    }
}
#[test]
fn invalid_packet_cannot_change_timeline_pose_or_silence_start() {
    let mut h = ReceiveHistory::default();
    h.receive(sample(1.0, 10.0), 1.0, Timebase::Arrival);
    for (s, arrival) in [
        (sample(f64::NAN, 99.0), 1.5),
        (sample(-1.0, 99.0), 1.5),
        (sample(1.5, f32::NAN), 1.5),
        (sample(1.5, 99.0), f64::INFINITY),
        (sample(1.5, 99.0), f64::NAN),
        (sample(1.5, 99.0), -1.0),
        (sample(1.5, 99.0), 0.9),
    ] {
        assert!(!h.receive(s, arrival, Timebase::Server));
        assert_eq!(*h.newest().unwrap(), sample(1.0, 10.0));
    }
    h.receive(sample(2.0, 30.0), 2.0, Timebase::Arrival);
    assert_eq!(
        h.len(),
        1,
        "invalid packets must not extend the old segment"
    );
}
#[test]
fn invalid_monotonic_sample_cannot_be_repaired_into_a_valid_timestamp() {
    let mut h = History::default();
    h.push_monotonic(sample(1.0, 10.0));
    for at in [f64::NAN, f64::NEG_INFINITY, -1.0] {
        h.push_monotonic(sample(at, 99.0));
        assert_eq!(*h.newest().unwrap(), sample(1.0, 10.0));
    }
    h.push_monotonic(sample(0.9, 20.0));
    assert_eq!(*h.newest().unwrap(), sample(1.0, 20.0));
}
#[test]
fn regular_jitter_keeps_bounded_history_and_existing_teleport_guard() {
    let mut h = ReceiveHistory::default();
    for i in 0..100 {
        let at = 1.0 + f64::from(i) / 60.0;
        h.receive(sample(at, i as f32), at, Timebase::Arrival);
    }
    assert_eq!(h.len(), HISTORY);
    h.receive(sample(3.0, 1_000_000.0), 3.0, Timebase::Arrival);
    let pose = h.evaluate(3.05, Config::DEFAULT).unwrap();
    assert_eq!(pose.position[0], 1_000_000.0);
    assert_eq!(pose.mode, client_dll::interpolation::Mode::Snapped);
}

#[cfg(all(windows, target_arch = "x86"))]
mod dll {
    use client_dll::{
        api::{self, Control, EngineControl},
        controller::Controller,
        draw_observation::Remote,
    };
    use multiplayer_server::protocol::{self, Position, Rotation, ServerUpdate};
    use std::{
        io::{BufRead, BufReader, Write},
        net::UdpSocket,
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
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
    fn row(c: &Controller, dll: &Path) -> Remote {
        let mut b = EngineControl::new(api::REMOTE_LIST);
        c.engine_control(dll, &mut b).unwrap();
        Remote::decode(&b)
    }
    #[test]
    fn real_dll_udp_receive_restarts_after_silence_and_still_rejects_old_sequences() {
        // Own process, no engine hooks/assets and no game. Dynamic loading and
        // real worker/UDP are necessary to verify the production integration.
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
        let mut output = BufReader::new(host.0.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let c = Controller::open(host.0.id()).unwrap();
        c.load(&dll).unwrap();
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut start = Control::new(api::START);
        start.config.mode = api::SIMULATED;
        api::put(
            &mut start.config.server,
            &socket.local_addr().unwrap().to_string(),
        )
        .unwrap();
        c.control(&dll, &mut start).unwrap();
        let (_, address) = socket.recv_from(&mut [0; protocol::MAX_PACKET]).unwrap();
        let send = |sequence: u64, x: f32| {
            let bytes = protocol::encode(&ServerUpdate {
                version: protocol::VERSION,
                player_id: 7,
                sequence,
                position: Position { x, y: 0.0, z: 0.0 },
                rotation: Some(Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                }),
            })
            .unwrap();
            socket.send_to(&bytes, address).unwrap();
        };
        let wait = |sequence: u32| {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let current = row(&c, &dll);
                if current.count == 1 && current.sequence == sequence {
                    break current;
                }
                assert!(
                    Instant::now() < deadline,
                    "remote sequence {sequence}: {current:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        send(1, 10.0);
        assert_eq!(wait(1).samples, 1);
        send(2, 20.0);
        assert_eq!(wait(2).samples, 2);
        thread::sleep(Duration::from_millis(1200));
        send(3, 50.0);
        let returned = wait(3);
        assert_eq!(returned.samples, 1);
        assert_eq!(returned.position[0], 50.0);
        send(2, -999.0);
        // Ordered sentinel proves the worker processed past the old datagram.
        send(4, 60.0);
        let fresh = wait(4);
        assert_eq!(fresh.samples, 2);
        assert_eq!(fresh.position[0], 60.0);
        // v1 has no server stamp: requesting server mode still uses actual
        // arrival provenance and must not falsely reset on every packet.
        let mut tune = Control::new(api::PRESENTATION_SET);
        api::put(
            &mut tune.message,
            r#"{"delay_ms":100,"extrapolation_ms":200,"timebase":"server"}"#,
        )
        .unwrap();
        c.control(&dll, &mut tune).unwrap();
        send(5, 70.0);
        assert_eq!(wait(5).samples, 3);
        c.control(&dll, &mut Control::new(api::STOP)).unwrap();
        writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
        assert!(host.0.wait().unwrap().success());
    }
}

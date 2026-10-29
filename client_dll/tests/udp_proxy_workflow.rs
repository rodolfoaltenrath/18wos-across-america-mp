//! Real x86 DLL worker and UDP sockets in an own fixture. No game or engine
//! hooks: synthetic replies, duplicates, stale/invalid packets and an outage.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control, EngineControl},
    controller::Controller,
    draw_observation::Remote,
    network_conditions::Conditions,
    udp_proxy::{Controls, Options, Proxy, Report},
};
use multiplayer_server::protocol::{self, Position, Rotation, ServerUpdate};
use std::{
    io::{self, BufRead, BufReader},
    net::UdpSocket,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
struct RunningProxy {
    controls: Arc<Controls>,
    worker: Option<JoinHandle<io::Result<Report>>>,
}
impl RunningProxy {
    fn finish(&mut self) -> Report {
        self.controls.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap().unwrap()
    }
}
impl Drop for RunningProxy {
    fn drop(&mut self) {
        self.controls.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct Echo {
    stop: Arc<AtomicBool>,
    stale: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Drop for Echo {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn echo(socket: UdpSocket) -> Echo {
    socket
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let stale = Arc::new(AtomicBool::new(false));
    let inject_stale = stale.clone();
    let worker = thread::spawn(move || {
        let mut buffer = [0; protocol::MAX_PACKET + 1];
        while !thread_stop.load(Ordering::Acquire) {
            let (len, address) = match socket.recv_from(&mut buffer) {
                Ok(value) => value,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(error) => panic!("fixture server receive: {error}"),
            };
            let Some(update) = protocol::decode_client(&buffer[..len]) else {
                continue;
            };
            let valid = ServerUpdate {
                version: protocol::VERSION,
                player_id: 7,
                sequence: update.sequence,
                position: Position {
                    x: update.sequence as f32,
                    y: -2.0,
                    z: 3.0,
                },
                rotation: Some(Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                }),
            };
            // An old sequence carrying a recognizably wrong pose, and a future
            // sequence with the wrong protocol version: neither may poison state.
            let mut replies = vec![
                valid,
                ServerUpdate {
                    version: 255,
                    sequence: u64::MAX,
                    ..valid
                },
            ];
            // Only call sequence zero stale AFTER the controller has witnessed
            // a newer accepted pose. Initially there is no ordering baseline.
            if inject_stale.load(Ordering::Acquire) {
                replies.push(ServerUpdate {
                    sequence: 0,
                    position: Position {
                        x: -9999.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    ..valid
                });
            }
            for reply in replies {
                socket
                    .send_to(&protocol::encode(&reply).unwrap(), address)
                    .unwrap();
            }
            socket.send_to(&[0; 2000], address).unwrap();
        }
    });
    Echo {
        stop,
        stale,
        worker: Some(worker),
    }
}
fn status(c: &Controller, dll: &Path) -> Control {
    let mut b = Control::new(api::STATUS);
    c.control(dll, &mut b).unwrap();
    b
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
    done: impl Fn(&Control, &Remote) -> bool,
) -> (Control, Remote) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let (state, row) = (status(c, dll), remote(c, dll));
        assert_eq!(
            state.status,
            1,
            "{what}: {}",
            api::text(&state.message).unwrap()
        );
        if done(&state, &row) {
            return (state, row);
        }
        assert!(
            Instant::now() < deadline,
            "{what}: sent={} received={} row={row:?}",
            state.sent,
            state.received
        );
        thread::sleep(Duration::from_millis(20));
    }
}
fn valid(row: &Remote) {
    assert_eq!((row.count, row.player_id), (1, 7));
    assert_eq!(row.position, [row.sequence as f32, -2.0, 3.0]);
    assert!(row.has_rotation);
    assert_eq!(row.rotation, [0.0, 0.0, 0.0, 1.0]);
    assert!((1..=16).contains(&row.samples));
}

#[test]
fn actual_worker_rejects_bad_or_stale_udp_and_recovers_after_controlled_blackout() {
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
    let mut first = String::new();
    output.read_line(&mut first).unwrap();
    let info: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(info["pid"], u64::from(fixture.0.id()));
    let controller = Controller::open(fixture.0.id()).unwrap();
    controller.load(&dll).unwrap();
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let proxy = Proxy::bind(Options {
        listen: "127.0.0.1:0".parse().unwrap(),
        server: server.local_addr().unwrap(),
        duration_ms: 30000,
        blackout_after_ms: 0,
        blackout_for_ms: 0,
        conditions: Conditions {
            seed: 42,
            loss_permille: 100,
            duplicate_permille: 1000,
            delay_ms: 20,
            jitter_ms: 50,
            queue_capacity: 64,
        },
    })
    .unwrap();
    let destination = proxy.local_addr().unwrap();
    let controls = Arc::new(Controls::default());
    let worker_controls = controls.clone();
    let mut run = RunningProxy {
        controls,
        worker: Some(thread::spawn(move || proxy.run(&worker_controls))),
    };
    let echo = echo(server);
    let mut start = Control::new(api::START);
    start.config.mode = api::SIMULATED;
    api::put(&mut start.config.server, &destination.to_string()).unwrap();
    controller.control(&dll, &mut start).unwrap();
    let (initial, row) = until(&controller, &dll, "initial receive", |state, row| {
        state.received >= 8 && row.sequence >= 8
    });
    valid(&row);
    echo.stale.store(true, Ordering::Release);
    let mut sequence = row.sequence;
    for _ in 0..12 {
        let row = remote(&controller, &dll);
        valid(&row);
        assert!(
            row.sequence >= sequence,
            "old datagram replaced newer state"
        );
        sequence = row.sequence;
        thread::sleep(Duration::from_millis(20));
    }
    run.controls.blackout.store(true, Ordering::Release);
    let (paused, frozen) = until(&controller, &dll, "receive outage", |_, row| {
        row.count == 1 && row.age_ms >= 1200
    });
    valid(&frozen);
    assert!(
        paused.sent > initial.sent,
        "worker stopped sending during a receive outage"
    );
    assert_eq!(paused.sample_state, 1);
    // The remote remains listed (5 s retention); no game draw is claimed here.
    run.controls.blackout.store(false, Ordering::Release);
    let (_, recovered) = until(&controller, &dll, "receive recovery", |state, row| {
        state.received > paused.received && row.sequence > frozen.sequence
    });
    valid(&recovered);
    let mut stop = Control::new(api::STOP);
    controller.control(&dll, &mut stop).unwrap();
    assert_eq!(stop.status, 2);
    let report = run.finish();
    assert!(report.external_blackout_used);
    assert!(report.uplink.blackout_dropped > 0);
    assert!(report.downlink.oversized > 0);
    assert!(report.uplink.network.duplicate_copies > 0);
    assert!(report.downlink.network.duplicate_copies > 0);
    assert!(report.uplink.network.peak_queue <= 64 && report.downlink.network.peak_queue <= 64);
    controller.unload(&dll).unwrap();
    assert!(
        controller
            .control(&dll, &mut Control::new(api::STATUS))
            .is_err()
    );
}

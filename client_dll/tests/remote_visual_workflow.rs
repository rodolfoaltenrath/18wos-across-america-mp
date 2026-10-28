#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, Control, EngineControl},
    controller::Controller,
    draw_observation::Visual,
};
use multiplayer_server::protocol::{self, Position, Rotation, ServerUpdate};
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    path::{Path, PathBuf},
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
fn status(c: &Controller, dll: &Path) -> Visual {
    let mut b = EngineControl::new(api::VISUAL_STATUS);
    c.engine_control(dll, &mut b).unwrap();
    Visual::decode(&b)
}
fn send(host: &mut Host, output: &mut impl BufRead, command: &str) -> String {
    writeln!(host.0.stdin.as_mut().unwrap(), "{command}").unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    line.trim().to_owned()
}
#[test]
fn drawn_pose_is_sent_and_a_relayed_player_pose_drives_the_test_visual() {
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
    let controller = Controller::open(host.0.id()).unwrap();
    controller.load(&dll).unwrap();
    let mut attach = EngineControl::new(api::UI_ATTACH);
    attach.window = info["window"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut attach).unwrap();
    let mut pump = EngineControl::new(api::UI_PUMP_FIXTURE);
    pump.entity = info["peek_slot"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut pump).unwrap();
    let mut render = EngineControl::new(api::UI_RENDER_FIXTURE);
    render.fixture = std::array::from_fn(|i| words("render_descriptors", i));
    controller.engine_control(&dll, &mut render).unwrap();
    let mut observer = EngineControl::new(api::UI_DRAW_FIXTURE);
    observer.fixture = std::array::from_fn(|i| words("draw_descriptors", i));
    controller.engine_control(&dll, &mut observer).unwrap();
    assert_eq!(send(&mut host, &mut output, "draw-shallow"), "armed");

    // Drawn source: the local pose observed by the draw probe goes out with rotation.
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut start = Control::new(api::START);
    start.config.mode = api::DRAWN;
    api::put(
        &mut start.config.server,
        &server.local_addr().unwrap().to_string(),
    )
    .unwrap();
    controller.control(&dll, &mut start).unwrap();
    let mut bytes = [0; protocol::MAX_PACKET];
    let (len, client) = server.recv_from(&mut bytes).unwrap();
    let local = protocol::decode_client(&bytes[..len]).unwrap();
    assert_eq!((local.position.y, local.position.z), (2.0, 3.0));
    assert!(local.position.x >= 1.0);
    assert_eq!(
        local.rotation,
        Some(Rotation {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0
        })
    );

    // A relayed player drives the visual; offset is added to the remote pose.
    let turned = Rotation {
        x: 0.0,
        y: std::f32::consts::FRAC_1_SQRT_2,
        z: 0.0,
        w: std::f32::consts::FRAC_1_SQRT_2,
    };
    let relay_from = |player_id, sequence, x| {
        let packet = protocol::encode(&ServerUpdate {
            version: protocol::VERSION,
            player_id,
            sequence,
            position: Position {
                x,
                y: 10.0,
                z: -20.0,
            },
            rotation: Some(turned),
        })
        .unwrap();
        server.send_to(&packet, client).unwrap();
    };
    let relay = |sequence, x| relay_from(5, sequence, x);
    relay(1, 5000.0);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut b = Control::new(api::STATUS);
    while {
        controller.control(&dll, &mut b).unwrap();
        b.received == 0
    } {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut show = EngineControl::new(api::VISUAL_SHOW_FIXTURE);
    show.fixture = std::array::from_fn(|i| words("instance_descriptors", i));
    for i in 0..6 {
        show.frames[i] = words("visual_descriptors", i);
    }
    api::put(&mut show.name, "models/fixture/truck.psm").unwrap();
    [show.count_before, show.count_created, show.count_after] = [0f32, 1.0, 0.0].map(f32::to_bits);
    show.entity = 5;
    let mut shown = show;
    controller.engine_control(&dll, &mut shown).unwrap();
    assert_eq!(Visual::decode(&shown).target, 5);
    std::thread::sleep(Duration::from_millis(80));
    assert!(status(&controller, &dll).drawn > 0);
    let last: serde_json::Value =
        serde_json::from_str(&send(&mut host, &mut output, "visual-last")).unwrap();
    assert_eq!(last["position"], serde_json::json!([5000.0, 11.0, -20.0]));
    let q = last["quaternion"].as_array().unwrap();
    assert_eq!(q[0], 0.0);
    assert!((q[1].as_f64().unwrap() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    // Newer samples move the visual (held, so interpolation settles on them);
    // an older sequence is ignored.
    for sequence in 3..6 {
        relay(sequence, 5100.0);
        std::thread::sleep(Duration::from_millis(20));
    }
    relay(2, 9999.0);
    std::thread::sleep(Duration::from_millis(250));
    let last: serde_json::Value =
        serde_json::from_str(&send(&mut host, &mut output, "visual-last")).unwrap();
    assert_eq!(last["position"][0], 5100.0);
    // Without fresh samples the visual stops drawing instead of freezing in place.
    std::thread::sleep(Duration::from_millis(1200));
    let stale = status(&controller, &dll);
    std::thread::sleep(Duration::from_millis(80));
    let later = status(&controller, &dll);
    assert_eq!(later.drawn, stale.drawn);
    assert!(later.skipped > stale.skipped);
    // An unknown player draws nothing.
    relay(6, 5200.0);
    let mut unknown = show;
    unknown.entity = 9;
    controller.engine_control(&dll, &mut unknown).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let before = status(&controller, &dll).drawn;
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(status(&controller, &dll).drawn, before);
    // "all": every fresh player is drawn with the same instance, once per frame each.
    let mut all = show;
    all.entity = u32::MAX;
    controller.engine_control(&dll, &mut all).unwrap();
    let mut sequence = 10;
    let mut feed = |rounds| {
        for _ in 0..rounds {
            relay_from(5, sequence, 5200.0);
            relay_from(6, sequence, 7000.0);
            sequence += 1;
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    feed(10);
    let draws = |c: &Controller| {
        let mut b = EngineControl::new(api::DRAW_STATUS);
        c.engine_control(&dll, &mut b).unwrap();
        b.count_before
    };
    let (visual_before, car_before) = (status(&controller, &dll).drawn, draws(&controller));
    feed(10);
    let (visual_after, car_after) = (status(&controller, &dll).drawn, draws(&controller));
    let (visual, car) = (visual_after - visual_before, car_after - car_before);
    assert!(
        car > 0 && visual + 6 >= 2 * car && visual <= 2 * car + 6,
        "{visual}/{car}"
    );
    // Listing through the CLI (it takes the per-PID control mutex).
    drop(controller);
    let listed = Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
        .args(["--pid", &host.0.id().to_string(), "--dll"])
        .arg(&dll)
        .args(["--fixture", "engine", "remotes"])
        .output()
        .unwrap();
    assert!(listed.status.success(), "{listed:?}");
    let rows: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(
        (rows[0]["player_id"].as_u64(), rows[1]["player_id"].as_u64()),
        (Some(5), Some(6))
    );
    assert_eq!(rows[1]["has_rotation"], true);
    assert_eq!(rows[1]["position"][0], 7000.0);
    let controller = Controller::open(host.0.id()).unwrap();

    let mut hide = EngineControl::new(api::VISUAL_HIDE);
    controller.engine_control(&dll, &mut hide).unwrap();
    let mut released = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(20));
        let mut b = EngineControl::new(api::VISUAL_RELEASE);
        match controller.engine_control(&dll, &mut b) {
            Ok(()) => {
                released = true;
                break;
            }
            Err(e) if e.to_string().contains("not yet flushed") => {}
            Err(e) => panic!("{e}"),
        }
    }
    assert!(released);
    let mut b = EngineControl::new(api::UI_PUMP_REMOVE);
    controller.engine_control(&dll, &mut b).unwrap();
    let mut stop = Control::new(api::STOP);
    controller.control(&dll, &mut stop).unwrap();
    assert_eq!(stop.status, 2);
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

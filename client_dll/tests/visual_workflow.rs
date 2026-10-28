#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
    draw_observation::Visual,
};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
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
fn error(c: &Controller, dll: &Path, mut b: EngineControl) -> String {
    c.engine_control(dll, &mut b).unwrap_err().to_string()
}
#[test]
fn test_visual_draws_only_in_validated_context_and_releases_after_flush() {
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
    let mut show = EngineControl::new(api::VISUAL_SHOW_FIXTURE);
    show.fixture = std::array::from_fn(|i| words("instance_descriptors", i));
    for i in 0..6 {
        show.frames[i] = words("visual_descriptors", i);
    }
    api::put(&mut show.name, "models/fixture/truck.psm").unwrap();
    api::put(&mut show.message, "models/fixture/truck.shd").unwrap();
    [show.count_before, show.count_created, show.count_after] =
        [400f32, 0.0, -50.0].map(f32::to_bits);
    assert!(error(&controller, &dll, show).contains("draw observer"));
    let mut render = EngineControl::new(api::UI_RENDER_FIXTURE);
    render.fixture = std::array::from_fn(|i| words("render_descriptors", i));
    controller.engine_control(&dll, &mut render).unwrap();
    let mut observer = EngineControl::new(api::UI_DRAW_FIXTURE);
    observer.fixture = std::array::from_fn(|i| words("draw_descriptors", i));
    controller.engine_control(&dll, &mut observer).unwrap();
    let mut game = show;
    game.command = api::VISUAL_SHOW;
    assert!(error(&controller, &dll, game).contains("mode does not match"));
    let mut bad_skin = show;
    api::put(&mut bad_skin.message, "models/fixture/truck.psm").unwrap();
    assert!(error(&controller, &dll, bad_skin).contains("shader requires"));
    assert_eq!(status(&controller, &dll).state, 0);
    let mut far = show;
    far.count_before = 20_000f32.to_bits();
    assert!(error(&controller, &dll, far).contains("within 100 m"));
    assert_eq!(status(&controller, &dll).state, 0);
    // Default fixture draw runs at world depth 2: shown but never drawn there.
    let mut b = show;
    controller.engine_control(&dll, &mut b).unwrap();
    assert_eq!(b.executed_thread, attach.window_thread);
    std::thread::sleep(Duration::from_millis(60));
    let deep = status(&controller, &dll);
    assert_eq!(
        (deep.state, deep.shown, deep.drawn, deep.fault),
        (1, 1, 0, 0)
    );
    assert!(deep.skipped > 0 && deep.item != 0);
    assert_eq!(deep.offset, [400.0, 0.0, -50.0]);
    assert_eq!(deep.skinned_parts, 3);
    assert_eq!(send(&mut host, &mut output, "draw-shallow"), "armed");
    std::thread::sleep(Duration::from_millis(60));
    let drawing = status(&controller, &dll);
    assert!(drawing.drawn > 0);
    let last: serde_json::Value =
        serde_json::from_str(&send(&mut host, &mut output, "visual-last")).unwrap();
    assert_eq!(last["item"], drawing.item);
    assert_eq!(last["depth"], 1);
    assert_eq!(last["shader_calls"], 3);
    let x = last["position"][0].as_f64().unwrap();
    assert!(x > 400.0 && x.fract() == 0.0);
    assert_eq!(last["position"][1], 2.0);
    assert_eq!(last["position"][2], -47.0);
    assert_eq!(last["quaternion"], serde_json::json!([0.0, 0.0, 0.0, 1.0]));
    // Interior state mirrors the game's own branch and is skipped.
    assert_eq!(send(&mut host, &mut output, "draw-interior"), "armed");
    std::thread::sleep(Duration::from_millis(30));
    let before = status(&controller, &dll).drawn;
    std::thread::sleep(Duration::from_millis(40));
    let interior = status(&controller, &dll);
    assert_eq!(interior.drawn, before);
    assert!(interior.skipped > drawing.skipped);
    assert_eq!(send(&mut host, &mut output, "draw-exterior"), "armed");
    // Guards: no observer/pump removal or release while in use.
    let release = EngineControl::new(api::VISUAL_RELEASE);
    assert!(error(&controller, &dll, release).contains("hide the visual"));
    assert!(error(&controller, &dll, EngineControl::new(api::UI_DRAW_REMOVE)).contains("hide"));
    assert!(error(&controller, &dll, EngineControl::new(api::UI_PUMP_REMOVE)).contains("release"));
    // A queued draw that the flush has not consumed keeps the instance alive.
    assert_eq!(send(&mut host, &mut output, "visual-retain"), "armed");
    std::thread::sleep(Duration::from_millis(30));
    let mut hide = EngineControl::new(api::VISUAL_HIDE);
    controller.engine_control(&dll, &mut hide).unwrap();
    assert_eq!(Visual::decode(&hide).shown, 0);
    std::thread::sleep(Duration::from_millis(120));
    let retained = status(&controller, &dll);
    assert!(retained.access > 0);
    assert!(retained.flushes - retained.last_draw_flush >= 8);
    assert!(error(&controller, &dll, release).contains("still referenced"));
    assert!(
        error(&controller, &dll, EngineControl::new(api::UI_RENDER_REMOVE)).contains("release")
    );
    assert_eq!(send(&mut host, &mut output, "visual-consume"), "armed");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(status(&controller, &dll).access, 0);
    let mut b = release;
    controller.engine_control(&dll, &mut b).unwrap();
    assert_eq!((Visual::decode(&b).state, Visual::decode(&b).item), (0, 0));
    let stats: serde_json::Value =
        serde_json::from_str(&send(&mut host, &mut output, "actor-stats")).unwrap();
    assert_eq!(stats["instance"]["created"], stats["instance"]["freed"]);
    // Recreate through the CLI path, then hide/release normally.
    let mut b = show;
    controller.engine_control(&dll, &mut b).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert!(status(&controller, &dll).drawn > 0);
    drop(controller);
    let launcher = Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
        .args(["--pid", &host.0.id().to_string(), "--dll"])
        .arg(&dll)
        .args(["--fixture", "engine", "visual-hide"])
        .output()
        .unwrap();
    assert!(launcher.status.success(), "{launcher:?}");
    let json: serde_json::Value = serde_json::from_slice(&launcher.stdout).unwrap();
    assert_eq!(
        (json["state"].as_u64(), json["shown"].as_u64()),
        (Some(1), Some(0))
    );
    let controller = Controller::open(host.0.id()).unwrap();
    let mut released = false;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(20));
        let mut b = release;
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
    assert_eq!(
        send(&mut host, &mut output, "visual-last")
            .parse::<serde_json::Value>()
            .unwrap()["pending"],
        0
    );
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

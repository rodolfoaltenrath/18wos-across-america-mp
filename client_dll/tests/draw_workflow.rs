#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
    draw_observation::{CAPACITY, Record, Summary},
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
fn call(c: &Controller, dll: &Path, command: u32) -> EngineControl {
    let mut b = EngineControl::new(command);
    c.engine_control(dll, &mut b).unwrap();
    b
}
fn capture(c: &Controller, dll: &Path) -> (Summary, Vec<Record>) {
    call(c, dll, api::DRAW_ARM);
    let mut summary = Summary::decode(&call(c, dll, api::DRAW_STATUS));
    for _ in 0..100 {
        if summary.flags & 3 == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        summary = Summary::decode(&call(c, dll, api::DRAW_STATUS));
    }
    assert_eq!(summary.flags, 0);
    assert_eq!(summary.records, CAPACITY as u32);
    let rows = (0..summary.records)
        .map(|i| {
            let mut b = EngineControl::new(api::DRAW_READ);
            b.cycles = summary.generation;
            b.reserved = i;
            c.engine_control(dll, &mut b).unwrap();
            Record::decode(&b)
        })
        .collect();
    (summary, rows)
}
fn send(host: &mut Host, output: &mut impl BufRead, command: &str) -> String {
    writeln!(host.0.stdin.as_mut().unwrap(), "{command}").unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    line.trim().to_owned()
}
#[test]
fn passive_car_draw_observes_pose_stack_and_flush_order_then_restores_slot() {
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
    let descriptors: [u32; 6] =
        std::array::from_fn(|i| info["draw_descriptors"][i].as_u64().unwrap() as u32);
    let car = info["draw_car"].as_u64().unwrap() as u32;
    let controller = Controller::open(host.0.id()).unwrap();
    controller.load(&dll).unwrap();
    let mut attach = EngineControl::new(api::UI_ATTACH);
    attach.window = info["window"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut attach).unwrap();
    let mut observer = EngineControl::new(api::UI_DRAW_FIXTURE);
    observer.fixture = descriptors;
    let mut b = observer;
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("pump before")
    );
    let mut pump = EngineControl::new(api::UI_PUMP_FIXTURE);
    pump.entity = info["peek_slot"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut pump).unwrap();
    // A wrong original must be refused without touching the slot.
    b = observer;
    b.fixture[1] = descriptors[1] + 1;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    assert_eq!(
        Summary::decode(&call(&controller, &dll, api::DRAW_STATUS)).state,
        0
    );
    let mut render = EngineControl::new(api::UI_RENDER_FIXTURE);
    render.fixture =
        std::array::from_fn(|i| info["render_descriptors"][i].as_u64().unwrap() as u32);
    controller.engine_control(&dll, &mut render).unwrap();
    b = observer;
    controller.engine_control(&dll, &mut b).unwrap();
    b = observer;
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("already used")
    );
    let (first, rows) = capture(&controller, &dll);
    assert_eq!(first.owner_thread, attach.window_thread);
    assert!(first.draw_calls >= 16 && first.flush_calls > 0);
    for (i, r) in rows.iter().enumerate() {
        assert_eq!(r.kind, 1 + (i as u32 % 2));
        assert_eq!((r.thread, r.this, r.state), (attach.window_thread, car, 7));
        assert_eq!(r.vtable, descriptors[0] - 0x6c);
        assert_ne!(r.caller, 0);
        assert_eq!((r.path, r.path_info, r.viewport), (0x2222, 0x1111, 0x3333));
        let json = r.json();
        assert_eq!(json["world_depth"], 2);
        assert_eq!(json["quaternion"], serde_json::json!([0.0, 0.0, 0.0, 1.0]));
        let x = json["position"][0].as_f64().unwrap();
        assert_eq!(json["position"][1], 2.0);
        assert_eq!(json["world_matrix"][12].as_f64().unwrap(), x);
        assert_eq!(json["camera_matrix"][14].as_f64().unwrap(), -x);
    }
    for pair in rows.chunks(2) {
        // Same frame: no flush between entering and leaving the draw.
        assert_eq!(pair[0].flushes, pair[1].flushes);
        assert_eq!(pair[0].caller, pair[1].caller);
    }
    for next in rows.chunks(2).collect::<Vec<_>>().windows(2) {
        // The fixture flushes once after each draw, as the game's frame must.
        assert_eq!(next[1][0].flushes, next[0][0].flushes + 1);
        assert!(
            next[1][0].json()["position"][0].as_f64() > next[0][0].json()["position"][0].as_f64()
        );
    }
    assert_eq!(send(&mut host, &mut output, "draw-foreign"), "armed");
    assert!(Summary::decode(&call(&controller, &dll, api::DRAW_STATUS)).foreign_calls > 0);
    assert_eq!(send(&mut host, &mut output, "draw-invalid"), "armed");
    std::thread::sleep(Duration::from_millis(20));
    let (second, invalid) = capture(&controller, &dll);
    assert!(second.generation > first.generation);
    assert!(
        invalid
            .iter()
            .all(|r| r.json()["position"].is_null() && r.body == 0)
    );
    assert!(invalid.iter().all(|r| !r.json()["world_matrix"].is_null()));
    b = EngineControl::new(api::DRAW_READ);
    b.cycles = first.generation;
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("generation changed")
    );
    b.cycles = second.generation;
    b.reserved = CAPACITY as u32;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    assert_eq!(send(&mut host, &mut output, "draw-valid"), "armed");
    // The CLI takes the per-PID control mutex; release ours while it runs.
    drop(controller);
    let launcher = |task: &str| {
        Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
            .args(["--pid", &host.0.id().to_string(), "--dll"])
            .arg(&dll)
            .args(["--fixture", "engine", task])
            .output()
            .unwrap()
    };
    assert!(launcher("draw-arm").status.success());
    std::thread::sleep(Duration::from_millis(100));
    let report = launcher("draw-report");
    assert!(report.status.success(), "{report:?}");
    let json: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(json["records"].as_array().unwrap().len(), CAPACITY);
    assert_eq!(json["records"][0]["kind"], "draw_enter");
    assert_eq!(json["records"][0]["world_depth"], 2);
    let status = launcher("draw-status");
    assert!(status.status.success(), "{status:?}");
    let controller = Controller::open(host.0.id()).unwrap();
    call(&controller, &dll, api::UI_DRAW_REMOVE);
    let stopped = Summary::decode(&call(&controller, &dll, api::DRAW_STATUS));
    assert_eq!(stopped.state, 2);
    let before: u32 = send(&mut host, &mut output, "draw-stats").parse().unwrap();
    std::thread::sleep(Duration::from_millis(40));
    let after: u32 = send(&mut host, &mut output, "draw-stats").parse().unwrap();
    assert!(after > before);
    let calls = Summary::decode(&call(&controller, &dll, api::DRAW_STATUS)).draw_calls;
    std::thread::sleep(Duration::from_millis(40));
    assert_eq!(
        Summary::decode(&call(&controller, &dll, api::DRAW_STATUS)).draw_calls,
        calls
    );
    // Removing the pump also removes the render probe; the DLL stays pinned.
    call(&controller, &dll, api::UI_PUMP_REMOVE);
    assert!(
        controller
            .unload(&dll)
            .unwrap_err()
            .to_string()
            .contains("pins DLL")
    );
    // Fixture exit checks the original virtual and PAGE_READONLY restoration.
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

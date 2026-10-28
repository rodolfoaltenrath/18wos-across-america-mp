#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
    render_observation::{Record, Summary},
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
fn arm(c: &Controller, dll: &Path) {
    for _ in 0..20 {
        let mut b = EngineControl::new(api::RENDER_ARM);
        match c.engine_control(dll, &mut b) {
            Ok(()) => return,
            Err(e) if e.to_string().contains("flush in progress") => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => panic!("{e}"),
        }
    }
    panic!("no gap between own fixture flushes");
}
fn capture(c: &Controller, dll: &Path) -> (Summary, Vec<Record>) {
    arm(c, dll);
    std::thread::sleep(Duration::from_millis(80));
    let summary = Summary::decode(&call(c, dll, api::RENDER_STATUS));
    assert_eq!(summary.flags, 0);
    assert_eq!(summary.records, 5);
    let rows = (0..summary.records)
        .map(|i| {
            let mut b = EngineControl::new(api::RENDER_READ);
            b.cycles = summary.generation;
            b.reserved = i;
            c.engine_control(dll, &mut b).unwrap();
            Record::decode(&b)
        })
        .collect();
    (summary, rows)
}
#[test]
fn passive_render_preserves_abi_error_state_observes_drain_and_restores_slots_on_stop() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .args(["--ansi", "--peek", "--render"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut output = BufReader::new(host.0.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let info: serde_json::Value = serde_json::from_str(&line).unwrap();
    let controller = Controller::open(host.0.id()).unwrap();
    controller.load(&dll).unwrap();
    let mut attach = EngineControl::new(api::UI_ATTACH);
    attach.window = info["window"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut attach).unwrap();
    let mut observer = EngineControl::new(api::UI_RENDER_FIXTURE);
    observer.fixture =
        std::array::from_fn(|i| info["render_descriptors"][i].as_u64().unwrap() as u32);
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
    b = observer;
    b.fixture[3] = 1;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    assert_eq!(
        Summary::decode(&call(&controller, &dll, api::RENDER_STATUS)).state,
        0
    );
    b = observer;
    b.command = api::UI_RENDER_ATTACH;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    controller.engine_control(&dll, &mut observer).unwrap();
    // A background-like pause must remain diagnosable without resetting capture.
    writeln!(host.0.stdin.as_mut().unwrap(), "render-pause").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    arm(&controller, &dll);
    let pending = Summary::decode(&call(&controller, &dll, api::RENDER_STATUS));
    assert_eq!((pending.flags, pending.records), (1, 0));
    // The CLI must take the per-PID control mutex; the module stays loaded.
    drop(controller);
    let status = Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
        .args(["--pid", &host.0.id().to_string(), "--dll"])
        .arg(&dll)
        .args(["--fixture", "engine", "render-status"])
        .output()
        .unwrap();
    assert!(status.status.success(), "{:?}", status);
    let json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(json["flags"], 1);
    assert_eq!(json["generation"], pending.generation);
    assert_eq!(json["records"], 0);
    let controller = Controller::open(host.0.id()).unwrap();
    let unchanged = Summary::decode(&call(&controller, &dll, api::RENDER_STATUS));
    assert_eq!(unchanged.generation, pending.generation);
    assert_eq!(unchanged.flags, 1);
    writeln!(host.0.stdin.as_mut().unwrap(), "render-resume").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    let (first, rows) = capture(&controller, &dll);
    assert_eq!(first.owner_thread, attach.window_thread);
    assert_eq!(
        rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(
        rows.iter()
            .all(|r| r.thread == attach.window_thread && r.caller != 0)
    );
    assert_eq!(
        (rows[1].prototypes, rows[2].prototypes, rows[3].prototypes),
        (2, 2, 0)
    );
    assert_eq!(rows[1].first_refs, 1);
    assert_eq!(rows[3].json()["empty_observed"], true);
    assert_eq!(rows[1].pool, rows[2].pool);
    assert_eq!(rows[2].pool, rows[3].pool);
    assert_ne!(rows[1].device, 0);
    assert!(!rows[1].json()["world_matrix"].is_null());
    assert!(!rows[1].json()["camera_matrix"].is_null());
    writeln!(host.0.stdin.as_mut().unwrap(), "render-invalid").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    std::thread::sleep(Duration::from_millis(20));
    let (second, invalid) = capture(&controller, &dll);
    assert!(second.generation > first.generation);
    assert!(invalid[1].json()["world_matrix"].is_null());
    assert!(!invalid[1].json()["camera_matrix"].is_null());
    b = EngineControl::new(api::RENDER_READ);
    b.cycles = first.generation;
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("generation changed")
    );
    b.cycles = second.generation;
    b.reserved = 128;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    writeln!(host.0.stdin.as_mut().unwrap(), "render-foreign").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    assert!(Summary::decode(&call(&controller, &dll, api::RENDER_STATUS)).foreign_calls > 0);
    writeln!(host.0.stdin.as_mut().unwrap(), "render-many").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    arm(&controller, &dll);
    std::thread::sleep(Duration::from_millis(80));
    let capped = Summary::decode(&call(&controller, &dll, api::RENDER_STATUS));
    assert_eq!((capped.flags, capped.records), (4, 128));
    writeln!(host.0.stdin.as_mut().unwrap(), "render-one").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    capture(&controller, &dll);
    // An intervening slot owner must not be overwritten by cleanup or unload.
    writeln!(host.0.stdin.as_mut().unwrap(), "render-steal").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    b = EngineControl::new(api::UI_RENDER_REMOVE);
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("slot owner changed")
    );
    assert!(controller.unload(&dll).is_err());
    assert_eq!(call(&controller, &dll, api::UI_STATUS).active, 1);
    writeln!(host.0.stdin.as_mut().unwrap(), "render-return").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    assert!(
        controller
            .unload(&dll)
            .unwrap_err()
            .to_string()
            .contains("pins DLL")
    );
    let stopped = Summary::decode(&call(&controller, &dll, api::RENDER_STATUS));
    assert_eq!(stopped.state, 2);
    assert_eq!(stopped.records, 5);
    writeln!(host.0.stdin.as_mut().unwrap(), "render-stats").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let before: u32 = line.trim().parse().unwrap();
    std::thread::sleep(Duration::from_millis(40));
    writeln!(host.0.stdin.as_mut().unwrap(), "render-stats").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert!(line.trim().parse::<u32>().unwrap() > before);
    // Fixture exit checks original pointer values and PAGE_READONLY restoration.
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

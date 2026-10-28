#![cfg(all(windows, target_pointer_width = "32"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
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
fn call(controller: &Controller, dll: &Path, command: u32) -> EngineControl {
    let mut b = EngineControl::new(command);
    controller.engine_control(dll, &mut b).unwrap();
    b
}
#[test]
fn dispatcher_uses_window_thread_cancels_timeout_and_restores_before_unload() {
    exercise(false, false);
    exercise(true, false);
    exercise(true, true);
}
fn exercise(ansi: bool, peek: bool) {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_engine_fixture"))
        .canonicalize()
        .unwrap();
    let mut command = Command::new(fixture);
    if ansi {
        command.arg("--ansi");
    }
    if peek {
        command.arg("--peek");
    }
    let mut host = Host(
        command
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
    let mut incompatible = EngineControl::new(api::UI_STATUS);
    incompatible.version = 1;
    assert!(controller.engine_control(&dll, &mut incompatible).is_err());
    incompatible.version = 2;
    incompatible.size = 604;
    assert!(controller.engine_control(&dll, &mut incompatible).is_err());
    let mut attach = EngineControl::new(api::UI_ATTACH);
    attach.window = info["window"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut attach).unwrap();
    if peek {
        let mut observer = EngineControl::new(api::UI_PUMP_FIXTURE);
        observer.entity = info["peek_slot"].as_u64().unwrap() as u32;
        controller.engine_control(&dll, &mut observer).unwrap();
        assert_eq!(observer.phase, 1);
        std::thread::sleep(Duration::from_millis(50));
        let state = call(&controller, &dll, api::UI_PUMP_STATUS);
        assert!(state.phase_hits > 0);
        assert_eq!(state.phase_thread, attach.window_thread);
        assert_ne!(state.phase_site, 0);
        let phase = call(&controller, &dll, api::PHASE_PROBE);
        assert_eq!(phase.executed_thread, attach.window_thread);
        assert_eq!(phase.phase_completed, 1);
        let mut lifecycle = EngineControl::new(api::PRIVATE_FIXTURE);
        lifecycle.fixture = std::array::from_fn(|i| info["factory"][i].as_u64().unwrap() as u32);
        lifecycle.cycles = 4;
        controller.engine_control(&dll, &mut lifecycle).unwrap();
        assert_eq!(lifecycle.cycles, 4);
        assert_eq!(
            (
                lifecycle.count_before,
                lifecycle.count_created,
                lifecycle.count_after
            ),
            (7, 9, 7)
        );
        assert_eq!(lifecycle.executed_thread, attach.window_thread);
        // A failed post-create header check must still destroy the private object.
        lifecycle.fixture[5] = 1;
        lifecycle.cycles = 1;
        assert!(controller.engine_control(&dll, &mut lifecycle).is_err());
        let mut wrong_mode = EngineControl::new(api::PRIVATE_CYCLE);
        wrong_mode.cycles = 1;
        assert!(controller.engine_control(&dll, &mut wrong_mode).is_err());
        // Nested PeekMessage DOES run, but window callbacks are never an engine phase.
        let before = call(&controller, &dll, api::UI_PUMP_STATUS).phase_completed;
        writeln!(host.0.stdin.as_mut().unwrap(), "nested-pump").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "nested");
        let mut pending = EngineControl::new(api::PRIVATE_FIXTURE);
        pending.fixture = std::array::from_fn(|i| info["factory"][i].as_u64().unwrap() as u32);
        pending.cycles = 1;
        assert!(
            controller
                .engine_control(&dll, &mut pending)
                .unwrap_err()
                .to_string()
                .contains("canceled before execution")
        );
        std::thread::sleep(Duration::from_millis(2400));
        assert_eq!(
            call(&controller, &dll, api::UI_PUMP_STATUS).phase_completed,
            before
        );
        writeln!(host.0.stdin.as_mut().unwrap(), "actor-stats").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        let stats: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(stats["created"], 5);
        assert_eq!(stats["freed"], 5);
        assert_eq!(stats["post"], 5);
        assert_eq!(stats["pre"], 5);
        assert_eq!(stats["root"], 7);
        assert_eq!(stats["game"], 0);
    }
    assert_eq!(attach.active, 1);
    assert_eq!(
        attach.window_thread,
        info["thread"].as_u64().unwrap() as u32
    );
    for _ in 0..3 {
        let probe = call(&controller, &dll, api::UI_PROBE);
        assert_eq!(probe.executed_thread, attach.window_thread);
    }
    let trace = call(&controller, &dll, api::UI_TRACE);
    assert!(trace.frame_count > 0 && trace.frame_count <= 32);
    assert_ne!(trace.executable_base, 0);
    writeln!(host.0.stdin.as_mut().unwrap(), "ping").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "pong");
    writeln!(host.0.stdin.as_mut().unwrap(), "title").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "title=Caminhao MP");
    let mut unavailable = EngineControl::new(api::UI_REGISTRY);
    assert!(controller.engine_control(&dll, &mut unavailable).is_err()); // no commercial module
    let previous = call(&controller, &dll, api::UI_STATUS).completed;
    writeln!(host.0.stdin.as_mut().unwrap(), "block").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "blocked");
    let mut timeout = EngineControl::new(api::UI_PROBE);
    assert!(controller.engine_control(&dll, &mut timeout).is_err());
    std::thread::sleep(Duration::from_millis(2400));
    let probe = call(&controller, &dll, api::UI_PROBE);
    assert_eq!(probe.completed, previous + 1); // timed-out task was not executed later
    if peek {
        call(&controller, &dll, api::UI_PUMP_REMOVE);
        let previous = call(&controller, &dll, api::UI_PUMP_STATUS).phase_hits;
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            call(&controller, &dll, api::UI_PUMP_STATUS).phase_hits,
            previous
        );
        let retained = controller.unload(&dll).unwrap_err().to_string();
        assert!(retained.contains("pins DLL until process exit"));
        let state = call(&controller, &dll, api::UI_STATUS);
        assert_eq!(state.active, 0);
        assert_eq!(state.phase, 2);
    } else {
        controller.unload(&dll).unwrap(); // stop automatically detaches and releases the module lease
    }
    writeln!(host.0.stdin.as_mut().unwrap(), "ping").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "pong");
    if !peek {
        controller.load(&dll).unwrap();
        assert_eq!(call(&controller, &dll, api::UI_STATUS).active, 0);
        controller.unload(&dll).unwrap();
    }
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

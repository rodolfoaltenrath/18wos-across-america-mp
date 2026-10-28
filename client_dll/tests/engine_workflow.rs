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
    exercise(false);
    exercise(true);
}
fn exercise(ansi: bool) {
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
    let mut attach = EngineControl::new(api::UI_ATTACH);
    attach.window = info["window"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut attach).unwrap();
    assert_eq!(attach.active, 1);
    assert_eq!(
        attach.window_thread,
        info["thread"].as_u64().unwrap() as u32
    );
    for _ in 0..3 {
        let probe = call(&controller, &dll, api::UI_PROBE);
        assert_eq!(probe.executed_thread, attach.window_thread);
    }
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
    controller.unload(&dll).unwrap(); // stop automatically detaches and releases the module lease
    writeln!(host.0.stdin.as_mut().unwrap(), "ping").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "pong");
    controller.load(&dll).unwrap();
    assert_eq!(call(&controller, &dll, api::UI_STATUS).active, 0);
    controller.unload(&dll).unwrap();
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

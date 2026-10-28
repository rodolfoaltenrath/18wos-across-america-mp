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
fn private_info_balances_provider_references_and_detects_repeated_retention() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .args(["--ansi", "--peek"])
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
    let mut pump = EngineControl::new(api::UI_PUMP_FIXTURE);
    pump.entity = info["peek_slot"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut pump).unwrap();
    let mut request = EngineControl::new(api::PRIVATE_INFO_FIXTURE);
    for i in 0..3 {
        request.fixture[i] = info["info_descriptors"][i].as_u64().unwrap() as u32;
    }
    api::put(&mut request.name, "models/fixture/truck.psm").unwrap();
    request.cycles = 4;
    let mut result = request;
    controller.engine_control(&dll, &mut result).unwrap();
    assert_eq!(result.cycles, 4);
    assert_eq!(result.object_size, 16);
    assert_eq!(result.executed_thread, attach.window_thread);
    assert_eq!(
        (
            result.count_before,
            result.count_created,
            result.count_after
        ),
        (7, 8, 7)
    );
    writeln!(host.0.stdin.as_mut().unwrap(), "info-cold").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    result = request;
    result.cycles = 16;
    controller.engine_control(&dll, &mut result).unwrap();
    assert_eq!(result.cycles, 16);
    assert_eq!(
        (
            result.count_before,
            result.count_created,
            result.count_after
        ),
        (10, 11, 10)
    );
    assert!(
        api::text(&result.message)
            .unwrap()
            .contains("retained module delta=3")
    );
    writeln!(host.0.stdin.as_mut().unwrap(), "info-null").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    result = request;
    result.cycles = 1;
    assert!(
        controller
            .engine_control(&dll, &mut result)
            .unwrap_err()
            .to_string()
            .contains("allocation returned null")
    );
    result = request;
    api::put(&mut result.name, "models/../truck.psm").unwrap();
    assert!(controller.engine_control(&dll, &mut result).is_err());
    result = request;
    result.command = api::PRIVATE_INFO;
    assert!(controller.engine_control(&dll, &mut result).is_err());
    result = request;
    result.fixture[0] = 1;
    assert!(controller.engine_control(&dll, &mut result).is_err());
    writeln!(host.0.stdin.as_mut().unwrap(), "info-repeat-growth").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    result = request;
    result.cycles = 2;
    assert!(
        controller
            .engine_control(&dll, &mut result)
            .unwrap_err()
            .to_string()
            .contains("module/cache delta requires analysis")
    );
    writeln!(host.0.stdin.as_mut().unwrap(), "info-normal").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    result = request;
    result.cycles = 1;
    controller.engine_control(&dll, &mut result).unwrap();
    assert_eq!(
        (
            result.count_before,
            result.count_created,
            result.count_after
        ),
        (12, 13, 12)
    );
    // A timed-out task cannot allocate later from inside a nested window pump.
    writeln!(host.0.stdin.as_mut().unwrap(), "nested-pump").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "nested");
    result = request;
    result.cycles = 1;
    assert!(
        controller
            .engine_control(&dll, &mut result)
            .unwrap_err()
            .to_string()
            .contains("canceled before execution")
    );
    std::thread::sleep(Duration::from_millis(2400));
    writeln!(host.0.stdin.as_mut().unwrap(), "actor-stats").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let stats: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(stats["info"]["created"], 23);
    assert_eq!(stats["info"]["freed"], 23);
    assert_eq!(stats["info"]["cache"], 5);
    assert_eq!(stats["info"]["context"], 12);
    assert_eq!(stats["info"]["model_refs"], 3);
    assert_eq!(stats["created"], 0);
    call(&controller, &dll, api::UI_PUMP_REMOVE);
    assert!(
        controller
            .unload(&dll)
            .unwrap_err()
            .to_string()
            .contains("pins DLL")
    );
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}
#[test]
fn dispatcher_uses_window_thread_cancels_timeout_and_restores_before_unload() {
    exercise(false, false);
    exercise(true, false);
    exercise(true, true);
}
#[test]
fn private_model_setter_uses_cdecl_bool_bounds_metadata_and_balanced_cleanup() {
    let dll = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release/client_dll.dll")
        .canonicalize()
        .unwrap();
    let mut host = Host(
        Command::new(env!("CARGO_BIN_EXE_engine_fixture"))
            .args(["--ansi", "--peek"])
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
    let mut pump = EngineControl::new(api::UI_PUMP_FIXTURE);
    pump.entity = info["peek_slot"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut pump).unwrap();
    let mut request = EngineControl::new(api::PRIVATE_MODEL_FIXTURE);
    request.fixture[0] = info["item_descriptors"][0].as_u64().unwrap() as u32;
    request.fixture[1] = info["model_definitions"][0].as_u64().unwrap() as u32;
    request.fixture[2] = info["model_setter"].as_u64().unwrap() as u32;
    request.cycles = 16;
    let longest = format!("models/{}.psm", "a".repeat(52));
    api::put(&mut request.name, &longest).unwrap();
    let mut result = request;
    controller.engine_control(&dll, &mut result).unwrap();
    assert_eq!(result.cycles, 16);
    assert_eq!(
        (
            result.count_before,
            result.count_created,
            result.count_after
        ),
        (7, 9, 7)
    );
    assert_eq!(result.object_size, 0xa8c);
    assert_eq!(result.executed_thread, attach.window_thread);
    assert!(
        api::text(&result.message)
            .unwrap()
            .contains("model_name verified")
    );
    request.cycles = 2;
    api::put(&mut request.name, "models/fixture/truck.psm").unwrap();
    result = request;
    controller.engine_control(&dll, &mut result).unwrap();
    assert_eq!(result.cycles, 2);
    // Each malformed input is rejected before the container is allocated.
    for model in [
        "models/../truck.psm".to_string(),
        format!("models/{}.psm", "a".repeat(53)),
    ] {
        result = request;
        api::put(&mut result.name, &model).unwrap();
        assert!(
            controller
                .engine_control(&dll, &mut result)
                .unwrap_err()
                .to_string()
                .contains("relative models/")
        );
    }
    for row in info["model_definitions"].as_array().unwrap().iter().skip(1) {
        result = request;
        result.fixture[1] = row.as_u64().unwrap() as u32;
        assert!(
            controller
                .engine_control(&dll, &mut result)
                .unwrap_err()
                .to_string()
                .contains("metadata differs")
        );
    }
    result = request;
    result.command = api::PRIVATE_MODEL; // game command cannot borrow fixture callbacks
    assert!(controller.engine_control(&dll, &mut result).is_err());
    for (failure, diagnostic) in [
        ("model-fail-set", "setter returned false"),
        ("model-no-write", "readback differs"),
        ("model-side-effect", "outside model_name"),
    ] {
        writeln!(host.0.stdin.as_mut().unwrap(), "{failure}").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "armed");
        result = request;
        result.cycles = 1;
        assert!(
            controller
                .engine_control(&dll, &mut result)
                .unwrap_err()
                .to_string()
                .contains(diagnostic)
        );
    }
    // A canceled model task must not allocate, mutate, or call the setter later.
    writeln!(host.0.stdin.as_mut().unwrap(), "nested-pump").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "nested");
    result = request;
    result.cycles = 1;
    assert!(
        controller
            .engine_control(&dll, &mut result)
            .unwrap_err()
            .to_string()
            .contains("canceled before execution")
    );
    std::thread::sleep(Duration::from_millis(2400));
    writeln!(host.0.stdin.as_mut().unwrap(), "actor-stats").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let stats: serde_json::Value = serde_json::from_str(&line).unwrap();
    for key in ["created", "freed", "post", "pre", "model_calls"] {
        assert_eq!(stats[key], 21, "{key}");
    }
    for key in ["allocated", "released"] {
        assert_eq!(stats["items"][key], 21);
    }
    for key in ["bytes_allocated", "bytes_freed"] {
        assert_eq!(stats["items"][key], 21 * 0xa8c);
    }
    assert_eq!(stats["root"], 7);
    assert_eq!(stats["game"], 0);
    call(&controller, &dll, api::UI_PUMP_REMOVE);
    assert!(
        controller
            .unload(&dll)
            .unwrap_err()
            .to_string()
            .contains("pins DLL")
    );
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
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
        let mut item = EngineControl::new(api::PRIVATE_ITEM_FIXTURE);
        item.fixture[0] = info["item_descriptors"][0].as_u64().unwrap() as u32;
        item.cycles = 4;
        controller.engine_control(&dll, &mut item).unwrap();
        assert_eq!(item.cycles, 4);
        assert_eq!(
            (item.count_before, item.count_created, item.count_after),
            (7, 9, 7)
        );
        assert_eq!(item.executed_thread, attach.window_thread);
        // The container still owns and frees the placed actor on a Rust error.
        item.fixture[0] = info["item_descriptors"][1].as_u64().unwrap() as u32;
        item.cycles = 1;
        assert!(
            controller
                .engine_control(&dll, &mut item)
                .unwrap_err()
                .to_string()
                .contains("header differs")
        );
        item.fixture[0] = info["item_descriptors"][0].as_u64().unwrap() as u32;
        for (failure, diagnostic) in [
            ("item-fail-fix", "storage allocation returned null"),
            ("item-fail-create", "constructor returned null"),
        ] {
            writeln!(host.0.stdin.as_mut().unwrap(), "{failure}").unwrap();
            line.clear();
            output.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "armed");
            item.cycles = 1;
            assert!(
                controller
                    .engine_control(&dll, &mut item)
                    .unwrap_err()
                    .to_string()
                    .contains(diagnostic)
            );
        }
        // Nested PeekMessage DOES run, but window callbacks are never an engine phase.
        let before = call(&controller, &dll, api::UI_PUMP_STATUS).phase_completed;
        writeln!(host.0.stdin.as_mut().unwrap(), "nested-pump").unwrap();
        line.clear();
        output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "nested");
        let mut pending = EngineControl::new(api::PRIVATE_ITEM_FIXTURE);
        pending.fixture[0] = info["item_descriptors"][0].as_u64().unwrap() as u32;
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
        assert_eq!(stats["created"], 10);
        assert_eq!(stats["freed"], 10);
        assert_eq!(stats["post"], 10);
        assert_eq!(stats["pre"], 10);
        assert_eq!(stats["items"]["allocated"], 7);
        assert_eq!(stats["items"]["released"], 7);
        assert_eq!(stats["items"]["bytes_allocated"], 6 * 0xa8c);
        assert_eq!(stats["items"]["bytes_freed"], 6 * 0xa8c);
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

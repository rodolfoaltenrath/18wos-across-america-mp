#![cfg(all(windows, target_pointer_width = "32"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
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
fn reply(
    host: &mut Host,
    output: &mut BufReader<std::process::ChildStdout>,
    command: &str,
) -> String {
    writeln!(host.0.stdin.as_mut().unwrap(), "{command}").unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    line.trim().to_owned()
}
#[test]
fn private_instances_release_duplicate_shaders_packets_info_and_storage_on_failures() {
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
    let mut request = EngineControl::new(api::PRIVATE_INSTANCE_FIXTURE);
    request.fixture =
        std::array::from_fn(|i| info["instance_descriptors"][i].as_u64().unwrap() as u32);
    request.cycles = 4;
    api::put(&mut request.name, "models/fixture/truck.psm").unwrap();
    let mut result = request;
    controller.engine_control(&dll, &mut result).unwrap();
    assert_eq!(result.cycles, 4);
    assert_eq!(result.object_size, 256);
    assert_eq!(result.executed_thread, attach.window_thread);
    assert_eq!(
        (
            result.count_before,
            result.count_created,
            result.count_after
        ),
        (8, 9, 8)
    );
    assert_eq!(reply(&mut host, &mut output, "instance-cold"), "armed");
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
            .contains("info=0 instance=2")
    );
    for cycles in [0, 17] {
        result = request;
        result.cycles = cycles;
        assert!(controller.engine_control(&dll, &mut result).is_err());
    }
    result = request;
    api::put(&mut result.name, "models/../bad.psm").unwrap();
    assert!(controller.engine_control(&dll, &mut result).is_err());
    result = request;
    result.command = api::PRIVATE_INSTANCE;
    assert!(controller.engine_control(&dll, &mut result).is_err());
    result = request;
    result.fixture[3] = 1;
    assert!(controller.engine_control(&dll, &mut result).is_err());
    for (fault, diagnostic, cycles) in [
        ("instance-null", "allocation returned null", 1),
        ("instance-bad-header", "header differs", 1),
        (
            "instance-repeat-growth",
            "module/cache delta requires analysis",
            2,
        ),
    ] {
        assert_eq!(reply(&mut host, &mut output, fault), "armed");
        result = request;
        result.cycles = cycles;
        assert!(
            controller
                .engine_control(&dll, &mut result)
                .unwrap_err()
                .to_string()
                .contains(diagnostic)
        );
    }
    assert_eq!(reply(&mut host, &mut output, "instance-normal"), "armed");
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
    assert_eq!(reply(&mut host, &mut output, "nested-pump"), "nested");
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
    let stats: serde_json::Value =
        serde_json::from_str(&reply(&mut host, &mut output, "actor-stats")).unwrap();
    for key in ["created", "freed"] {
        assert_eq!(stats["instance"][key], 24);
        assert_eq!(stats["info"][key], 6);
    }
    for key in ["bytes_allocated", "bytes_freed"] {
        assert_eq!(stats["instance"][key], 24 * 256);
    }
    assert_eq!(stats["instance"]["pool_refs"], 0);
    assert_eq!(stats["instance"]["shader_refs"], serde_json::json!([3, 5]));
    assert_eq!(stats["info"]["context"], 11);
    assert_eq!(stats["info"]["cache"], 4);
    assert_eq!(stats["info"]["model_refs"], 3);
    assert_eq!(stats["created"], 0);
    let mut remove = EngineControl::new(api::UI_PUMP_REMOVE);
    controller.engine_control(&dll, &mut remove).unwrap();
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

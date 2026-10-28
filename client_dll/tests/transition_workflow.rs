#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
    transition_observation::{CAPACITY, Record, Summary},
};
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
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
fn send(host: &mut Host, output: &mut impl BufRead, command: &str) -> String {
    writeln!(host.0.stdin.as_mut().unwrap(), "{command}").unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    line.trim().to_owned()
}
#[test]
fn phase_virtuals_forward_arguments_result_and_error_then_restore_on_detach() {
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
    let controller = Controller::open(host.0.id()).unwrap();
    controller.load(&dll).unwrap();
    let call = |command| {
        let mut b = EngineControl::new(command);
        controller.engine_control(&dll, &mut b).unwrap();
        b
    };
    let mut attach = EngineControl::new(api::UI_ATTACH);
    attach.window = info["window"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut attach).unwrap();
    let mut observer = EngineControl::new(api::UI_TRANSITION_FIXTURE);
    observer.fixture =
        std::array::from_fn(|i| info["transition_descriptors"][i].as_u64().unwrap() as u32);
    let mut b = observer;
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("pump before")
    );
    b = EngineControl::new(api::UI_PUMP_FIXTURE);
    b.entity = info["peek_slot"].as_u64().unwrap() as u32;
    controller.engine_control(&dll, &mut b).unwrap();
    // Invalid SECOND original must not hook the first virtual either.
    b = observer;
    b.fixture[3] += 1;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    assert_eq!(Summary::decode(&call(api::TRANSITION_STATUS)).state, 0);
    for (command, key) in [
        (api::UI_RENDER_FIXTURE, "render_descriptors"),
        (api::UI_DRAW_FIXTURE, "draw_descriptors"),
    ] {
        b = EngineControl::new(command);
        b.fixture = std::array::from_fn(|i| info[key][i].as_u64().unwrap() as u32);
        controller.engine_control(&dll, &mut b).unwrap();
    }
    b = observer;
    controller.engine_control(&dll, &mut b).unwrap();
    b = EngineControl::new(api::VISUAL_SHOW_FIXTURE);
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("requires no retained visual")
    );
    let wait = |count| {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            let summary = Summary::decode(&call(api::TRANSITION_STATUS));
            if summary.records >= count {
                return summary;
            }
            assert!(Instant::now() < until, "capture timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let rows = |summary: &Summary| {
        (0..summary.records)
            .map(|i| {
                let mut b = EngineControl::new(api::TRANSITION_READ);
                b.cycles = summary.generation;
                b.reserved = i;
                controller.engine_control(&dll, &mut b).unwrap();
                Record::decode(&b)
            })
            .collect::<Vec<_>>()
    };
    call(api::TRANSITION_ARM);
    let baseline = wait(1);
    assert_eq!(baseline.flags, 1);
    assert_eq!(rows(&baseline)[0].kind, 5);
    assert_eq!(send(&mut host, &mut output, "transition-cycle"), "armed");
    // Poll after the fixture's synchronous enter/return burst to avoid deliberately
    // contending with its try_lock trace (contention is reported as LOST).
    std::thread::sleep(Duration::from_millis(80));
    let summary = wait(10);
    assert_eq!(summary.flags, 1);
    assert_eq!((summary.init_calls, summary.done_calls), (2, 2));
    let captured = rows(&summary);
    assert_eq!(
        captured.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [5, 3, 4, 3, 4, 1, 2, 1, 2, 5]
    );
    assert!(
        captured
            .iter()
            .all(|r| r.thread == attach.window_thread && r.readable == 15)
    );
    assert!(
        captured
            .iter()
            .all(|r| u64::from(r.this) == info["draw_car"].as_u64().unwrap())
    );
    assert!(captured[1..9].iter().all(|r| r.caller != 0 && r.depth == 1));
    assert_eq!(
        (captured[3].argument, captured[3].values[2]),
        (0, 0x11110000)
    );
    assert_eq!(captured[4].values[2], 0);
    assert_eq!(captured[5].values[2], 0);
    assert_eq!(captured[6].values[2], 0x11110000);
    assert_eq!(send(&mut host, &mut output, "transition-foreign"), "armed");
    let foreign = Summary::decode(&call(api::TRANSITION_STATUS));
    assert_eq!((foreign.records, foreign.foreign_calls), (10, 1));
    call(api::TRANSITION_ARM);
    b = EngineControl::new(api::TRANSITION_READ);
    b.cycles = summary.generation;
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("generation changed")
    );
    wait(1);
    assert_eq!(send(&mut host, &mut output, "transition-nested"), "armed");
    std::thread::sleep(Duration::from_millis(80));
    let nested = rows(&wait(6));
    assert_eq!(
        nested.iter().map(|r| r.depth).collect::<Vec<_>>(),
        [0, 1, 2, 2, 1, 0]
    );
    assert_eq!(nested[2].argument, 8);
    call(api::TRANSITION_ARM);
    wait(1);
    assert_eq!(send(&mut host, &mut output, "transition-overflow"), "armed");
    std::thread::sleep(Duration::from_millis(80));
    let overflow = wait(CAPACITY as u32);
    assert_eq!((overflow.records, overflow.flags), (CAPACITY as u32, 4));
    b = EngineControl::new(api::TRANSITION_READ);
    b.cycles = overflow.generation;
    b.reserved = CAPACITY as u32;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    // Refuse to overwrite a slot another owner changed; retry after it restores.
    assert_eq!(send(&mut host, &mut output, "transition-conflict"), "armed");
    b = EngineControl::new(api::UI_TRANSITION_REMOVE);
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("owner changed")
    );
    assert_eq!(
        send(&mut host, &mut output, "transition-unconflict"),
        "armed"
    );
    // Stop/detach must restore the transition hooks without a separate remove.
    call(api::UI_DETACH);
    assert_eq!(Summary::decode(&call(api::TRANSITION_STATUS)).state, 2);
    assert!(
        controller
            .unload(&dll)
            .unwrap_err()
            .to_string()
            .contains("pins DLL")
    );
    let originals_before: u32 = send(&mut host, &mut output, "transition-stats")
        .parse()
        .unwrap();
    let observed_before = Summary::decode(&call(api::TRANSITION_STATUS)).done_calls;
    send(&mut host, &mut output, "transition-cycle");
    std::thread::sleep(Duration::from_millis(80));
    let originals_after: u32 = send(&mut host, &mut output, "transition-stats")
        .parse()
        .unwrap();
    assert_eq!(originals_after, originals_before + 4);
    assert_eq!(
        Summary::decode(&call(api::TRANSITION_STATUS)).done_calls,
        observed_before
    );
    drop(controller);
    let report = Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
        .args(["--pid", &host.0.id().to_string(), "--dll"])
        .arg(&dll)
        .args(["--fixture", "engine", "transition-report"])
        .output()
        .unwrap();
    assert!(report.status.success(), "{report:?}");
    let report: serde_json::Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report["records"].as_array().unwrap().len(), CAPACITY);
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

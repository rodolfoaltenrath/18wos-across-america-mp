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
        controller
            .engine_control(&dll, &mut b)
            .unwrap_or_else(|error| panic!("engine command {command}: {error:?}"));
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
    let mut blocks = EngineControl::new(api::UI_TRANSITION_BLOCKS_FIXTURE);
    blocks.fixture = std::array::from_fn(|i| info["block_descriptors"][i].as_u64().unwrap() as u32);
    b = blocks;
    b.fixture[3] += 1;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    assert!(!Summary::decode(&call(api::TRANSITION_STATUS)).blocks_installed);
    b = blocks;
    controller.engine_control(&dll, &mut b).unwrap();
    assert!(Summary::decode(&call(api::TRANSITION_STATUS)).blocks_installed);
    let mut commands = EngineControl::new(api::UI_TRANSITION_COMMANDS_FIXTURE);
    commands.fixture =
        std::array::from_fn(|i| info["command_descriptors"][i].as_u64().unwrap() as u32);
    b = commands;
    b.fixture[1] += 1;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    assert!(!Summary::decode(&call(api::TRANSITION_STATUS)).commands_installed);
    b = commands;
    b.fixture[0] = blocks.fixture[0];
    assert!(controller.engine_control(&dll, &mut b).is_err());
    b = commands;
    b.fixture[2] = 0;
    assert!(controller.engine_control(&dll, &mut b).is_err());
    b = commands;
    controller.engine_control(&dll, &mut b).unwrap();
    assert!(Summary::decode(&call(api::TRANSITION_STATUS)).commands_installed);
    b = commands;
    assert!(controller.engine_control(&dll, &mut b).is_err());
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
            .all(|r| r.thread == attach.window_thread && r.readable & 15 == 15)
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
    assert_eq!(captured[3].models[..3], [0x11110000, 0, 0]);
    assert_eq!(captured[4].models[..3], [0, 0, 0]);
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
    // Reproduce the real ordering and signed phases, keeping the two entities
    // distinct. No destruction of the new entity is inferred from old callbacks.
    call(api::TRANSITION_ARM);
    let initial = rows(&wait(1))[0];
    assert_eq!(send(&mut host, &mut output, "transition-reload"), "armed");
    std::thread::sleep(Duration::from_millis(80));
    let real_order = rows(&wait(16));
    assert_eq!(real_order.len(), 16);
    assert_eq!(
        real_order.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [5, 1, 2, 1, 2, 1, 2, 1, 2, 3, 4, 3, 4, 3, 4, 5]
    );
    assert_eq!(
        real_order[1..9]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| p[0].argument as i32)
            .collect::<Vec<_>>(),
        [-10, -1, 0, 10]
    );
    assert_eq!(
        real_order[9..15]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| p[0].argument as i32)
            .collect::<Vec<_>>(),
        [-10, 0, 10]
    );
    let new_this = real_order[15].this;
    assert_ne!(new_this, initial.this);
    assert!(real_order[1..9].iter().all(|r| r.this == new_this));
    assert!(real_order[9..15].iter().all(|r| r.this == initial.this));
    assert_eq!(real_order[11].values[2], 0x11110000);
    assert_eq!(real_order[12].values[2], 0);
    call(api::TRANSITION_ARM);
    let old_this = rows(&wait(1))[0].this;
    send(&mut host, &mut output, "transition-block-reload");
    std::thread::sleep(Duration::from_millis(80));
    let block_summary = wait(20);
    assert_eq!(
        (
            block_summary.block_init_calls,
            block_summary.destroy_requests,
            block_summary.flags
        ),
        (1, 1, 1)
    );
    let block_rows = rows(&block_summary);
    assert_eq!(
        block_rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [5, 8, 9, 6, 1, 2, 1, 2, 1, 2, 1, 2, 7, 3, 4, 3, 4, 3, 4, 5]
    );
    assert_eq!(block_rows[2].returned, 1); // bool AL despite original's dirty EAX.
    assert_eq!(block_rows[2].context[0], old_this);
    assert_eq!(block_rows[2].context[7], 1);
    assert_ne!(block_rows[2].context[6], 0);
    assert_eq!(block_rows[3].context[0], block_rows[4].this);
    assert_eq!(block_rows[3].context[5], old_this);
    assert!(block_rows[4..12].iter().all(|r| r.depth == 2));
    assert!(block_rows[13..19].iter().all(|r| r.this == old_this));
    assert_ne!(block_rows[19].this, old_this);
    assert_eq!(block_rows[12].json()["returned"], true);
    assert!(block_rows[3].json()["phase"].is_null());
    assert_eq!(block_rows[3].json()["block_strong"], 1);
    call(api::TRANSITION_ARM);
    let old_this = rows(&wait(1))[0].this;
    send(&mut host, &mut output, "transition-block-rollback");
    std::thread::sleep(Duration::from_millis(80));
    let rollback = wait(14);
    let rollback_rows = rows(&rollback);
    assert_eq!(
        (
            rollback.block_init_calls,
            rollback.destroy_requests,
            rollback.flags
        ),
        (2, 1, 1)
    );
    assert_eq!(rollback_rows[12].kind, 7);
    assert_eq!(rollback_rows[12].json()["returned"], false);
    assert_eq!((rollback_rows[10].kind, rollback_rows[10].depth), (3, 2));
    assert_eq!(rollback_rows[10].this, rollback_rows[1].context[0]);
    assert_ne!(rollback_rows[10].this, old_this);
    assert_eq!(rollback_rows[13].this, old_this);
    send(&mut host, &mut output, "transition-block-foreign");
    assert_eq!(
        Summary::decode(&call(api::TRANSITION_STATUS)).foreign_calls,
        2
    );
    assert_eq!(Summary::decode(&call(api::TRANSITION_STATUS)).records, 14);
    // The profiled C call returns exactly once, keeping ECX's entry identity,
    // EAX and LastError, but only its owner-thread/callsite enters the trace.
    call(api::TRANSITION_ARM);
    wait(1);
    let argv_before: u32 = send(&mut host, &mut output, "transition-command-stats")
        .parse()
        .unwrap();
    send(&mut host, &mut output, "transition-command-unrelated");
    send(&mut host, &mut output, "transition-command-foreign");
    let filtered = Summary::decode(&call(api::TRANSITION_STATUS));
    assert_eq!(
        (
            filtered.records,
            filtered.command_calls,
            filtered.foreign_calls
        ),
        (1, 0, 3)
    );
    let old = rows(&filtered)[0];
    send(&mut host, &mut output, "transition-command-reload");
    std::thread::sleep(Duration::from_millis(80));
    let ordered = rows(&wait(21));
    assert_eq!(ordered.len(), 21);
    assert_eq!(ordered[1].kind, 10);
    assert_eq!(ordered[1].this, commands.fixture[3]);
    assert_eq!(ordered[1].context[0], old.values[0]);
    assert_eq!(ordered[1].context[5], old.this);
    assert_eq!(ordered[1].json()["command"], "load");
    assert_eq!(ordered[1].json()["command_status"], "complete");
    assert!(ordered[1].json()["block_data"].is_null());
    assert_eq!(ordered[2].kind, 8);
    assert_eq!(ordered[4].kind, 6);
    assert_ne!(ordered.last().unwrap().this, old.this);
    for (action, status) in [
        ("transition-command-invalid", "unreadable"),
        ("transition-command-long", "unterminated"),
        ("transition-command-null", "unreadable"),
    ] {
        call(api::TRANSITION_ARM);
        wait(1);
        send(&mut host, &mut output, action);
        std::thread::sleep(Duration::from_millis(80));
        let records = rows(&wait(3));
        assert_eq!(
            records.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [5, 10, 5]
        );
        assert_eq!(records[1].command, [0; 32]);
        assert_eq!(records[1].json()["command_status"], status);
        assert!(records[1].json()["command"].is_null());
    }
    let argv_after: u32 = send(&mut host, &mut output, "transition-command-stats")
        .parse()
        .unwrap();
    assert_eq!(argv_after, argv_before + 6);
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
    send(&mut host, &mut output, "transition-command-conflict");
    b = EngineControl::new(api::UI_TRANSITION_REMOVE);
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("owner changed")
    );
    send(&mut host, &mut output, "transition-command-unconflict");
    send(&mut host, &mut output, "transition-block-conflict");
    b = EngineControl::new(api::UI_TRANSITION_REMOVE);
    assert!(
        controller
            .engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("owner changed")
    );
    send(&mut host, &mut output, "transition-block-unconflict");
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
    let observed_blocks = Summary::decode(&call(api::TRANSITION_STATUS)).block_init_calls;
    let observed_commands = Summary::decode(&call(api::TRANSITION_STATUS)).command_calls;
    call(api::UI_DETACH);
    assert_eq!(Summary::decode(&call(api::TRANSITION_STATUS)).state, 2);
    assert!(!Summary::decode(&call(api::TRANSITION_STATUS)).commands_installed);
    send(&mut host, &mut output, "transition-command-foreign");
    assert_eq!(
        Summary::decode(&call(api::TRANSITION_STATUS)).command_calls,
        observed_commands
    );
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
    let block_calls: u32 = send(&mut host, &mut output, "transition-block-stats")
        .parse()
        .unwrap();
    send(&mut host, &mut output, "transition-block-rollback");
    std::thread::sleep(Duration::from_millis(80));
    send(&mut host, &mut output, "transition-block-foreign");
    assert_eq!(
        send(&mut host, &mut output, "transition-block-stats")
            .parse::<u32>()
            .unwrap(),
        block_calls + 2
    );
    assert_eq!(
        Summary::decode(&call(api::TRANSITION_STATUS)).block_init_calls,
        observed_blocks
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

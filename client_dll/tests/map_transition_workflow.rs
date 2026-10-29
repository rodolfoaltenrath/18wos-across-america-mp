//! Separate own process/DLL boundary. Never loads a commercial game image.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
    transition_observation::{Record, Summary},
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
    line.trim().into()
}
#[test]
fn map_descriptor_observes_nested_reload_and_rollback_then_restores_without_touching_argv() {
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
    let c = Controller::open(host.0.id()).unwrap();
    c.load(&dll).unwrap();
    let call = |command| {
        let mut b = EngineControl::new(command);
        c.engine_control(&dll, &mut b).unwrap();
        b
    };
    let mut b = EngineControl::new(api::UI_ATTACH);
    b.window = info["window"].as_u64().unwrap() as u32;
    c.engine_control(&dll, &mut b).unwrap();
    b = EngineControl::new(api::UI_PUMP_FIXTURE);
    b.entity = info["peek_slot"].as_u64().unwrap() as u32;
    c.engine_control(&dll, &mut b).unwrap();
    for (command, key) in [
        (api::UI_RENDER_FIXTURE, "render_descriptors"),
        (api::UI_DRAW_FIXTURE, "draw_descriptors"),
        (api::UI_TRANSITION_FIXTURE, "transition_descriptors"),
        (api::UI_TRANSITION_BLOCKS_FIXTURE, "block_descriptors"),
        (api::UI_TRANSITION_COMMANDS_FIXTURE, "command_descriptors"),
    ] {
        b = EngineControl::new(command);
        b.fixture = std::array::from_fn(|i| info[key][i].as_u64().unwrap() as u32);
        c.engine_control(&dll, &mut b).unwrap();
    }
    let mut map = EngineControl::new(api::UI_TRANSITION_MAP_FIXTURE);
    map.fixture = std::array::from_fn(|i| info["map_descriptors"][i].as_u64().unwrap() as u32);
    // Preflight must fail without touching the descriptor or removing old hooks.
    for index in [1, 2, 3, 4] {
        b = map;
        b.fixture[index] = 1;
        assert!(c.engine_control(&dll, &mut b).is_err());
        let s = Summary::decode(&call(api::TRANSITION_STATUS));
        assert!(!s.map_installed && s.commands_installed && s.state == 1);
    }
    b = map;
    b.fixture[3] = b.fixture[2];
    assert!(c.engine_control(&dll, &mut b).is_err());
    b = map;
    c.engine_control(&dll, &mut b).unwrap();
    assert!(Summary::decode(&call(api::TRANSITION_STATUS)).map_installed);
    b = map;
    assert!(c.engine_control(&dll, &mut b).is_err());
    let capture = |minimum| {
        let until = Instant::now() + Duration::from_secs(4);
        loop {
            let s = Summary::decode(&call(api::TRANSITION_STATUS));
            if s.records >= minimum {
                let rows = (0..s.records)
                    .map(|i| {
                        let mut b = EngineControl::new(api::TRANSITION_READ);
                        b.cycles = s.generation;
                        b.reserved = i;
                        c.engine_control(&dll, &mut b).unwrap();
                        Record::decode(&b)
                    })
                    .collect::<Vec<_>>();
                return (s, rows);
            }
            assert!(Instant::now() < until, "map capture timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let analyze = |s: &Summary, r: &[Record]| {
        client_dll::transition_analysis::analyze(&serde_json::to_vec(&serde_json::json!({"summary":s,"records":r.iter().map(Record::json).collect::<Vec<_>>()})).unwrap()).unwrap()
    };
    call(api::TRANSITION_ARM);
    capture(1);
    send(&mut host, &mut output, "transition-map-reload");
    std::thread::sleep(Duration::from_millis(100));
    let (s, r) = capture(21);
    assert_eq!((s.map_calls, s.foreign_calls, s.flags), (1, 0, 1));
    assert_eq!(
        r.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [
            5, 11, 10, 6, 1, 2, 1, 2, 1, 2, 1, 2, 7, 3, 4, 3, 4, 3, 4, 12, 5
        ]
    );
    assert_eq!(r[1].argument, map.fixture[0]);
    assert_eq!(r[1].context[1], 2);
    assert_eq!(r[1].command, r[19].command);
    assert_eq!(r[19].returned, 0xfedcba98);
    assert_eq!(r[2].depth, 2);
    assert_eq!(r[4].depth, 3);
    assert_eq!(r[1].this, r[2].this);
    let a = analyze(&s, &r);
    assert!(a.structurally_complete && !a.automation_validated);
    assert_eq!(a.map_calls.len(), 1);
    assert_eq!(a.map_calls[0].first_handler_dispatch_inside, Some(2));
    assert_eq!(a.map_calls[0].first_different_actor_init_inside, Some(4));
    assert_eq!(a.map_calls[0].first_old_actor_done_inside, Some(13));
    assert_eq!(
        a.map_calls[0].first_different_actor_draw_after_return,
        Some(20)
    );
    assert_eq!(a.map_calls[0].argv_unchanged, Some(true));
    call(api::TRANSITION_ARM);
    capture(1);
    send(&mut host, &mut output, "transition-map-nested");
    std::thread::sleep(Duration::from_millis(100));
    let (s, r) = capture(24);
    let a = analyze(&s, &r);
    assert!(a.structurally_complete);
    assert_eq!(a.map_calls.len(), 2);
    assert_eq!(a.map_calls[0].first_handler_dispatch_inside, Some(5));
    assert_eq!(a.map_calls[1].first_handler_dispatch_inside, Some(3));
    assert_eq!(
        (r[1].depth, r[2].depth, r[3].depth, r[4].depth),
        (1, 2, 3, 2)
    );
    call(api::TRANSITION_ARM);
    capture(1);
    send(&mut host, &mut output, "transition-map-rollback");
    std::thread::sleep(Duration::from_millis(100));
    let (s, r) = capture(17);
    let a = analyze(&s, &r);
    assert!(a.structurally_complete);
    assert_eq!(a.failed_block_initializations.len(), 1);
    assert!(a.map_calls[0].first_old_actor_done_inside.is_none());
    assert!(
        a.map_calls[0]
            .first_different_actor_draw_after_return
            .is_none()
    );
    call(api::TRANSITION_ARM);
    capture(1);
    send(&mut host, &mut output, "transition-map-arguments-change");
    std::thread::sleep(Duration::from_millis(100));
    let (s, r) = capture(5);
    let a = analyze(&s, &r);
    assert!(!a.structurally_complete && !a.automation_validated);
    assert_eq!(a.map_calls[0].argv_unchanged, Some(false));
    assert_eq!(
        (a.map_calls[0].argc_before, a.map_calls[0].argc_after),
        (Some(2), Some(0))
    );
    let before = Summary::decode(&call(api::TRANSITION_STATUS));
    send(&mut host, &mut output, "transition-map-foreign");
    let after = Summary::decode(&call(api::TRANSITION_STATUS));
    assert_eq!(
        (after.records, after.map_calls),
        (before.records, before.map_calls)
    );
    assert_eq!(after.foreign_calls, before.foreign_calls + 1);
    assert_eq!(send(&mut host, &mut output, "transition-map-stats"), "6");
    // A foreign callback stays outside the trace, but removal/arming must wait
    // until its original has actually returned.
    send(&mut host, &mut output, "transition-map-hold");
    for command in [api::UI_TRANSITION_REMOVE, api::TRANSITION_ARM] {
        b = EngineControl::new(command);
        assert!(
            c.engine_control(&dll, &mut b)
                .unwrap_err()
                .to_string()
                .contains("callback active")
        );
    }
    send(&mut host, &mut output, "transition-map-release");
    send(&mut host, &mut output, "transition-map-conflict");
    b = EngineControl::new(api::UI_TRANSITION_REMOVE);
    assert!(
        c.engine_control(&dll, &mut b)
            .unwrap_err()
            .to_string()
            .contains("owner changed")
    );
    send(&mut host, &mut output, "transition-map-unconflict");
    call(api::UI_DETACH);
    assert!(!Summary::decode(&call(api::TRANSITION_STATUS)).map_installed);
    c.control(&dll, &mut api::Control::new(api::STOP)).unwrap();
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

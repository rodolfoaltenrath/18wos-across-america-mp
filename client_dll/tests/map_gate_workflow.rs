//! Active map gate end to end on the own fixture: session-start installs it,
//! a load request with a retained visual is deferred, the visual is released
//! before the fixture's load runs, the original is replayed once with copied
//! arguments and the visual is recreated in the new world.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    api::{self, EngineControl},
    controller::Controller,
};
use serde_json::Value;
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    path::PathBuf,
    process::{Child, ChildStdout, Command, Output, Stdio},
    time::{Duration, Instant},
};
struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Bench {
    host: Host,
    output: BufReader<ChildStdout>,
    pid: String,
    dll: PathBuf,
}
impl Bench {
    fn send(&mut self, command: &str) -> String {
        writeln!(self.host.0.stdin.as_mut().unwrap(), "{command}").unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        line.trim().into()
    }
    fn stats(&mut self) -> Value {
        serde_json::from_str(&self.send("gate-stats")).unwrap()
    }
    fn launcher(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
            .args(["--pid", &self.pid, "--dll"])
            .arg(&self.dll)
            .arg("--fixture")
            .args(args)
            .output()
            .unwrap()
    }
    fn status(&self) -> Value {
        let out = self.launcher(&["session-status"]);
        assert!(out.status.success(), "{out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    }
    /// Poll session-status until `done` holds, with a bounded wait.
    fn until(&self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let s = self.status();
            if done(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "{what}: {s:#}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    fn stats_until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let s = self.stats();
            if done(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "{what}: {s:#}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn idle(s: &Value) -> bool {
    s["load_gate"]["phase"] == "idle" && s["load_gate"]["in_flight"] == false
}
fn shown(s: &Value) -> bool {
    s["visual"]["state"] == 1 && s["visual"]["shown"] == 1
}
#[test]
fn deferred_map_releases_before_load_replays_once_and_recreates() {
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
    let info: Value = serde_json::from_str(&line).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let info_path = directory.path().join("fixture.json");
    std::fs::write(&info_path, &line).unwrap();
    let pid = host.0.id().to_string();
    let mut b = Bench {
        host,
        output,
        pid,
        dll: dll.clone(),
    };
    assert_eq!(b.send("draw-shallow"), "armed");
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap().to_string();
    let started = b.launcher(&[
        "session-start",
        "--server",
        &address,
        "--player",
        "0",
        "--load-timeout",
        "2",
        "--fixture-info",
        info_path.to_str().unwrap(),
    ]);
    assert!(started.status.success(), "{started:?}");
    let report: Value = serde_json::from_slice(&started.stdout).unwrap();
    assert!(
        report["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["step"] == "load-gate-attach" && s["ok"] == true)
    );
    let s = b.until("first draw", |s| {
        shown(s) && s["visual"]["drawn"].as_u64().unwrap() > 0
    });
    assert_eq!(s["load_gate"]["installed"], true);
    assert_eq!(s["load_gate"]["timeout_ms"], 2000);
    assert_eq!(s["load_gate"]["automatic_load_supported"], false);
    // A second gate on the same descriptor is refused.
    // One controller per call: it holds the per-process lock the launcher needs.
    let id = b.host.0.id();
    let engine =
        |block: &mut EngineControl| Controller::open(id).unwrap().engine_control(&dll, block);
    let mut again = EngineControl::new(api::UI_LOAD_GATE_FIXTURE);
    again.fixture = std::array::from_fn(|i| info["map_descriptors"][i].as_u64().unwrap() as u32);
    assert!(engine(&mut again).is_err());
    // The passive transition map observer cannot share the descriptor either.
    let mut passive = EngineControl::new(api::UI_TRANSITION_MAP_FIXTURE);
    passive.fixture = again.fixture;
    assert!(engine(&mut passive).is_err());

    // 1. Retained visual: swallowed, released, replayed once, recreated.
    assert_eq!(b.send("gate-map"), "armed");
    let st = b.stats_until("deferred load", |s| s["loads"] == 1);
    assert_eq!(st["synchronous_loads"], 0, "{st}");
    assert_eq!(st["unsafe_loads"], 0, "{st}");
    assert_eq!(
        (st["bad_calls"].clone(), st["abi_errors"].clone()),
        (0.into(), 0.into())
    );
    assert_eq!(st["swallowed"], 1);
    let s = b.until("recreated", |s| {
        idle(s) && s["load_gate"]["recreated"] == 1 && shown(s)
    });
    let g = &s["load_gate"];
    assert_eq!(
        [
            &g["deferred"],
            &g["replays"],
            &g["arguments_restored"],
            &g["dropped"]
        ],
        [1, 1, 1, 0].map(Value::from).each_ref()
    );
    assert_eq!(g["last_return"], 0xfedc_ba98u32);
    let first_actor = g["actor"].clone();
    // The recreated instance is drawn again in the new world.
    let before = s["visual"]["drawn"].as_u64().unwrap();
    b.until("drawn after recreation", |s| {
        s["visual"]["drawn"].as_u64().unwrap() > before
    });

    // 2. Two requests in one frame: the second is a duplicate and is dropped.
    assert_eq!(b.send("gate-map-twice"), "armed");
    let st = b.stats_until("second load", |s| s["loads"] == 2);
    assert_eq!(
        (st["swallowed"].clone(), st["unsafe_loads"].clone()),
        (3.into(), 0.into())
    );
    let s = b.until("recreated twice", |s| {
        idle(s) && s["load_gate"]["recreated"] == 2 && shown(s)
    });
    assert_eq!(s["load_gate"]["swallowed_busy"], 1);
    assert_ne!(s["load_gate"]["actor"], first_actor);

    // 3. A command nested inside the replayed load is forwarded to it.
    assert_eq!(b.send("gate-map-nested"), "armed");
    let st = b.stats_until("nested load", |s| s["loads"] == 3);
    assert_eq!(
        (st["nested"].clone(), st["unsafe_loads"].clone()),
        (1.into(), 0.into())
    );
    let s = b.until("recreated thrice", |s| {
        idle(s) && s["load_gate"]["recreated"] == 3 && shown(s)
    });
    assert_eq!(s["load_gate"]["replay_nested"], 1);

    // 4. Hidden by the user: released and loaded, but not recreated.
    let hidden = b.launcher(&["engine", "visual-hide"]);
    assert!(hidden.status.success(), "{hidden:?}");
    assert_eq!(b.send("gate-map"), "armed");
    b.stats_until("hidden load", |s| s["loads"] == 4);
    let s = b.until("released without recreation", |s| {
        idle(s) && s["visual"]["state"] == 0
    });
    assert_eq!(s["load_gate"]["recreated"], 3);
    assert_eq!(b.stats()["unsafe_loads"], 0);

    // 5. Nothing retained: the original load runs synchronously, unchanged.
    assert_eq!(b.send("gate-map"), "armed");
    let st = b.stats_until("forwarded load", |s| s["loads"] == 5);
    assert_eq!(st["synchronous_loads"], 1);
    // 6. argc 1 is a no-op in the handler and is forwarded too.
    assert_eq!(b.send("gate-map-short"), "armed");
    b.stats_until("short", |s| s["no_arguments"] == 1);
    let s = b.status();
    assert_eq!(s["load_gate"]["forwarded"], 2);
    assert_eq!(s["load_gate"]["last_note"], "no_arguments");

    let show = |b: &Bench| {
        let mut v = EngineControl::new(api::VISUAL_SHOW_FIXTURE);
        v.fixture =
            std::array::from_fn(|i| info["instance_descriptors"][i].as_u64().unwrap() as u32);
        for i in 0..8 {
            v.frames[i] = info["visual_descriptors"][i].as_u64().unwrap() as u32;
        }
        api::put(&mut v.name, "models/fixture/truck.psm").unwrap();
        api::put(&mut v.message, "models/fixture/truck.shd").unwrap();
        engine(&mut v).unwrap();
        b.until("shown again", |s| {
            shown(s) && s["visual"]["drawn"].as_u64().unwrap() > 0
        });
    };
    // 7. Unreadable arguments: request dropped, visual released, no load.
    show(&b);
    let loads = b.stats()["loads"].clone();
    assert_eq!(b.send("gate-map-bad"), "armed");
    let s = b.until("fallback release", |s| {
        s["load_gate"]["fallback_releases"] == 1 && s["visual"]["state"] == 0
    });
    assert_eq!(s["load_gate"]["dropped"], 1);
    assert_eq!(s["load_gate"]["last_note"], "capture_failed");
    assert_eq!(b.stats()["loads"], loads);

    // 8. Draws never flushed: deadline expires BEFORE any load or free.
    show(&b);
    assert_eq!(b.send("visual-retain"), "armed");
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(b.send("gate-map"), "armed");
    let s = b.until("deadline", |s| s["load_gate"]["phase"] == "recoverable");
    assert_eq!(s["load_gate"]["reason"], "deadline_expired");
    assert_eq!(s["load_gate"]["execute_started"], false);
    assert_eq!(
        (s["visual"]["state"].clone(), s["visual"]["shown"].clone()),
        (1.into(), 0.into())
    );
    assert_eq!(b.stats()["loads"], loads);
    // While the visual is still retained a manual show is refused only while busy;
    // a terminal job does not block a NEW request, which completes normally.
    assert_eq!(b.send("visual-consume"), "armed");
    assert_eq!(b.send("gate-map"), "armed");
    let st = b.stats_until("load after recovery", |s| {
        s["loads"] == loads.as_u64().unwrap() + 1
    });
    assert_eq!(st["unsafe_loads"], 0);
    let s = b.until("recreated after recovery", |s| {
        idle(s) && s["load_gate"]["recreated"] == 4 && shown(s)
    });
    assert_eq!(s["load_gate"]["deferred"], 6);

    // 9. Session stop waits for the gate, releases, removes it and restores.
    let stopped = b.launcher(&["session-stop", "--timeout", "10"]);
    assert!(stopped.status.success(), "{stopped:?}");
    let stopped: Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(stopped["released"], true);
    assert_eq!(stopped["load_gate"]["phase"], "idle");
    let mut status = EngineControl::new(api::LOAD_GATE_STATUS);
    engine(&mut status).unwrap();
    let gate = client_dll::load_gate::Status::decode(&status).unwrap();
    assert!(!gate.installed);
    let st = b.stats();
    assert_eq!(
        (st["live_instances"].clone(), st["abi_errors"].clone()),
        (0.into(), 0.into())
    );
    assert_eq!(st["bad_calls"], 0);
    // After removal the descriptor forwards directly again.
    assert_eq!(b.send("gate-map"), "armed");
    b.stats_until("direct load after removal", |s| s["synchronous_loads"] == 2);
    writeln!(b.host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(b.host.0.wait().unwrap().success());
}

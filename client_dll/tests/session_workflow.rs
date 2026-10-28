#![cfg(all(windows, target_arch = "x86"))]
use multiplayer_server::protocol::{self, Position, Rotation, ServerUpdate};
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    time::Duration,
};
struct Host(Child);
impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn session_commands_compose_start_status_and_safe_stop() {
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
    let directory = tempfile::tempdir().unwrap();
    let info = directory.path().join("fixture.json");
    std::fs::write(&info, &line).unwrap();
    writeln!(host.0.stdin.as_mut().unwrap(), "draw-shallow").unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "armed");
    let server = UdpSocket::bind("127.0.0.1:0").unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let pid = host.0.id().to_string();
    let launcher = |args: &[&str]| -> Output {
        Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
            .args(["--pid", &pid, "--dll"])
            .arg(&dll)
            .arg("--fixture")
            .args(args)
            .output()
            .unwrap()
    };
    let address = server.local_addr().unwrap().to_string();
    let info_path = info.to_str().unwrap();
    let saves = directory.path().join("saves");
    std::fs::create_dir(&saves).unwrap();
    std::fs::write(saves.join("a.sav"), b"first").unwrap();
    std::fs::write(saves.join("b.sav"), b"second").unwrap();
    let backup = directory.path().join("backup");
    let (saves_path, backup_path) = (saves.to_str().unwrap(), backup.to_str().unwrap());
    // The fixture flag and the descriptor file must go together.
    let missing = Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
        .args(["--pid", &pid, "--dll"])
        .arg(&dll)
        .args(["--fixture", "session-start", "--server", &address])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let started = launcher(&[
        "session-start",
        "--server",
        &address,
        "--offset",
        "0",
        "1",
        "0",
        "--fixture-info",
        info_path,
        "--backup",
        backup_path,
        "--saves",
        saves_path,
    ]);
    assert!(started.status.success(), "{started:?}");
    let report: serde_json::Value = serde_json::from_slice(&started.stdout).unwrap();
    assert_eq!(report["started"], true);
    assert_eq!(report["backup_files"], 2);
    assert_eq!(
        std::fs::read(backup.join("saves/b.sav")).unwrap(),
        b"second"
    );
    assert_eq!(report["steps"].as_array().unwrap().len(), 7);
    // A second start stops at the first refused step and reports it.
    let again = launcher(&[
        "session-start",
        "--server",
        &address,
        "--fixture-info",
        info_path,
    ]);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("session-start failed at"));
    let mut bytes = [0; protocol::MAX_PACKET];
    let (len, client) = server.recv_from(&mut bytes).unwrap();
    assert!(
        protocol::decode_client(&bytes[..len])
            .unwrap()
            .rotation
            .is_some()
    );
    for sequence in 1..20 {
        let packet = protocol::encode(&ServerUpdate {
            version: protocol::VERSION,
            player_id: 3,
            sequence,
            position: Position {
                x: 100.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: Some(Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            }),
        })
        .unwrap();
        server.send_to(&packet, client).unwrap();
        std::thread::sleep(Duration::from_millis(15));
    }
    let status = launcher(&["session-status"]);
    assert!(status.status.success(), "{status:?}");
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["client"]["status"], 1);
    assert!(status["client"]["sent"].as_u64().unwrap() > 0);
    assert_eq!(status["remotes"][0]["player_id"], 3);
    assert_eq!(status["visual"]["target"], u32::MAX);
    assert!(status["visual"]["drawn"].as_u64().unwrap() > 0);
    let stopped = launcher(&["session-stop", "--timeout", "10", "--verify", backup_path]);
    assert!(stopped.status.success(), "{stopped:?}");
    let stopped: serde_json::Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(stopped["released"], true);
    assert_eq!(stopped["client_status"], 2);
    assert_eq!(stopped["files"]["unchanged"], 2);
    // A changed save is reported; the backup copy is kept for restoration.
    std::fs::write(saves.join("a.sav"), b"changed").unwrap();
    let changed = launcher(&["session-verify", "--backup", backup_path]);
    assert!(!changed.status.success());
    assert!(String::from_utf8_lossy(&changed.stderr).contains("a.sav"));
    assert_eq!(std::fs::read(backup.join("saves/a.sav")).unwrap(), b"first");
    writeln!(host.0.stdin.as_mut().unwrap(), "quit").unwrap();
    assert!(host.0.wait().unwrap().success());
}

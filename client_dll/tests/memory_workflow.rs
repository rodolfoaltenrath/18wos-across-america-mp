#![cfg(windows)]

use client_dll::{
    addresses::hex,
    inspection::{process::Process, snapshot::Snapshot},
};
use multiplayer_server::protocol;
use std::{
    io::{BufRead, BufReader, Write},
    net::UdpSocket,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::Duration,
};

fn tool() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_memory_tool"))
        .canonicalize()
        .unwrap()
}

struct Fixture {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    info: serde_json::Value,
}

impl Fixture {
    fn new() -> Self {
        let mut process = Command::new(tool())
            .arg("fixture")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = process.stdin.take().unwrap();
        let mut output = BufReader::new(process.stdout.take().unwrap());
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let info = serde_json::from_str(&line).unwrap();
        Self {
            process,
            input,
            output,
            info,
        }
    }

    fn set(&mut self) {
        writeln!(self.input, "set 7.25 -2.5 30").unwrap();
        self.input.flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "updated");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn run(args: &[&str]) -> String {
    let output = Command::new(tool()).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn discovers_remote_changes_resolves_pointers_and_streams_actual_readings() {
    let mut fixture = Fixture::new();
    let pid = fixture.process.id().to_string();
    let addresses: Vec<String> = fixture.info["addresses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_string())
        .collect();
    let start = addresses[0].clone();
    let end = format!("0x{:x}", hex(&addresses[2]).unwrap() + 4);
    let temp = tempfile::tempdir().unwrap();
    let baseline = temp.path().join("baseline.aamem");
    let changed = temp.path().join("changed.aamem");
    let stable = temp.path().join("stable.aamem");
    run(&[
        "scan",
        "--pid",
        &pid,
        "--start",
        &start,
        "--end",
        &end,
        "--output",
        baseline.to_str().unwrap(),
        "--min-abs",
        "0",
        "--memory",
        "private",
    ]);
    assert_eq!(Snapshot::load(&baseline).unwrap().candidates.len(), 3);
    let excluded = temp.path().join("excluded-image.aamem");
    run(&[
        "scan",
        "--pid",
        &pid,
        "--start",
        &start,
        "--end",
        &end,
        "--memory",
        "image",
        "--output",
        excluded.to_str().unwrap(),
    ]);
    assert!(Snapshot::load(&excluded).unwrap().candidates.is_empty());
    assert!(run(&["regions", "--pid", &pid, "--limit", "1"]).contains("PRIVATE:"));
    fixture.set();
    run(&[
        "filter",
        baseline.to_str().unwrap(),
        "--output",
        changed.to_str().unwrap(),
        "--mode",
        "changed",
    ]);
    let changed = Snapshot::load(&changed).unwrap();
    assert_eq!(
        changed
            .candidates
            .iter()
            .map(|entry| entry.value)
            .collect::<Vec<_>>(),
        vec![7.25, 30.0]
    );
    run(&[
        "filter",
        baseline.to_str().unwrap(),
        "--output",
        stable.to_str().unwrap(),
        "--mode",
        "unchanged",
    ]);
    let stable = Snapshot::load(&stable).unwrap();
    assert_eq!(stable.candidates.len(), 1);
    assert_eq!(stable.candidates[0].value, -2.5);
    let dump = run(&["dump", "--pid", &pid, "--address", &start, "--length", "12"]);
    assert!(dump.contains("u32=0x40e80000") && dump.contains("f32=7.250000000"));
    assert!(dump.contains("f32=-2.500000000") && dump.contains("f32=30.000000000"));
    let pointer = format!("{}->0x4", fixture.info["pointer"].as_str().unwrap());
    let watch = run(&[
        "watch",
        "--pid",
        &pid,
        "--address",
        &pointer,
        "--seconds",
        "1",
        "--hz",
        "1",
    ]);
    assert!(watch.contains("=-2.500000"));
    let double_address = fixture.info["double_addresses"][0].as_str().unwrap();
    let watch = run(&[
        "watch",
        "--pid",
        &pid,
        "--address",
        double_address,
        "--kind",
        "f64",
        "--seconds",
        "1",
        "--hz",
        "1",
    ]);
    assert!(watch.contains("=7.250000"));
    let process = Process::open(fixture.process.id()).unwrap();
    let mut stale = process.identity.clone();
    stale.created += 1;
    assert!(process.same_session(&stale).is_err());
    assert!(process.read(1, &mut [0; 4]).is_err());
    assert!(
        process
            .modules()
            .unwrap()
            .iter()
            .any(|module| module.name.eq_ignore_ascii_case("memory_tool.exe"))
    );

    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let stream = Command::new(tool())
        .args([
            "stream",
            "--pid",
            &pid,
            "--x",
            &addresses[0],
            "--y",
            &addresses[1],
            "--z",
            &addresses[2],
            "--server",
            &receiver.local_addr().unwrap().to_string(),
            "--seconds",
            "1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut bytes = [0; protocol::MAX_PACKET];
    let len = receiver.recv(&mut bytes).unwrap();
    let update = protocol::decode_client(&bytes[..len]).unwrap();
    assert_eq!(update.position.x, 7.25);
    assert_eq!(update.position.y, -2.5);
    assert_eq!(update.position.z, 30.0);
    let output = stream.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Reusing an existing output must fail without changing its previous data.
    let bytes = std::fs::read(&baseline).unwrap();
    let failure = Command::new(tool())
        .args([
            "scan",
            "--pid",
            &pid,
            "--output",
            baseline.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert_eq!(std::fs::read(&baseline).unwrap(), bytes);
    fixture.process.kill().unwrap();
    fixture.process.wait().unwrap();
    assert!(process.ensure_running().is_err());
}

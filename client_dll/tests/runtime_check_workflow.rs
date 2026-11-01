//! Real Windows loader and ABI in our own short-lived launcher, no game/assets.
#![cfg(all(windows, target_arch = "x86"))]
use client_dll::{
    runtime_check::{self, LoaderFailure, LoaderStage},
    runtime_info::Source,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
fn build() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/i686-pc-windows-gnu/release")
        .canonicalize()
        .unwrap()
}
fn invoke(dll: &Path, pid: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
        .args(["--pid", pid, "--dll"])
        .arg(dll)
        .arg("runtime-check")
        .output()
        .unwrap()
}
#[test]
fn actual_pair_loads_queries_and_unloads_without_worker_game_or_network() {
    let report = runtime_check::run(&build()).unwrap();
    report.validate().unwrap();
    assert_eq!(report.runtime.source, Source::None);
    assert!(report.module_released);
    assert!(!report.worker_started && !report.game_tested && !report.lan_tested);
    // A second NEW helper succeeds too; no process-pinned DLL or shared worker.
    assert_eq!(
        runtime_check::run(&build()).unwrap().dll_sha256,
        report.dll_sha256
    );
    let denied = invoke(Path::new("nonexistent-client.dll"), "1");
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("requires --pid 0"));
}
#[test]
fn portable_assistant_checks_own_kit_without_game_or_rust_installation() {
    let directory = tempfile::tempdir().unwrap();
    let kit = directory.path().join("own-kit");
    client_dll::lan_kit::create(
        &build(),
        &kit,
        "own-runtime-fixture",
        &client_dll::lan_kit::Settings {
            server: "127.0.0.1:39000".parse().unwrap(),
            room: "lan".into(),
        },
        None,
    )
    .unwrap();
    let output = Command::new(kit.join("mp_lan.exe"))
        .arg("--check-runtime")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: runtime_check::Report = serde_json::from_slice(&output.stdout).unwrap();
    report.validate().unwrap();
    assert!(!kit.join("prism3d.exe").exists());
    assert!(!kit.join("pending.json").exists());
}
#[test]
fn broken_own_dependency_reports_loader_code_and_malformed_dll_is_refused() {
    let directory = tempfile::Builder::new()
        .prefix("runtime check próprios ")
        .tempdir()
        .unwrap();
    let dll = directory.path().join("client_dll.dll");
    let mut bytes = fs::read(build().join("client_dll.dll")).unwrap();
    // Mutate only our own PE import fixture: preserve length, exports and x86
    // headers while requesting a nonexistent DLL instead of KERNEL32.
    let original = b"KERNEL32.dll\0";
    let matches: Vec<_> = bytes
        .windows(original.len())
        .enumerate()
        .filter_map(|(at, b)| b.eq_ignore_ascii_case(original).then_some(at))
        .collect();
    assert!(!matches.is_empty());
    for at in matches {
        bytes[at..at + original.len()].copy_from_slice(b"missing0.dll\0");
    }
    fs::write(&dll, bytes).unwrap();
    let output = invoke(&dll, "0");
    assert!(!output.status.success());
    let failure: LoaderFailure = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure.schema, 1);
    assert!(matches!(failure.stage, LoaderStage::Load));
    assert_ne!(failure.windows_code, 0);
    fs::write(&dll, b"own malformed DLL fixture").unwrap();
    let refused = invoke(&dll, "0");
    assert!(!refused.status.success() && refused.stdout.is_empty());
}

//! Own reports through actual CLI export/check; no game, kit or private saves.
use std::{fs, process::Command};
#[test]
fn report_cli_exports_only_filtered_evidence_and_doctor_detects_changes() {
    let t = tempfile::tempdir().unwrap();
    let input = t.path().join("private-name-status.json");
    let raw = serde_json::to_vec(&serde_json::json!({"client":{"status":1,"sample_state":1,"sent":1,"received":0,"diagnostic":"PRIVATE-SENTINEL"},"draw":{"draw_calls":0,"flush_calls":0,"foreign_calls":0,"flags":0},"visual":{"item":0,"shown":0,"drawn":0,"fault":0},"presentation":client_dll::presentation::status(),"remotes":[],"token":"PRIVATE-SENTINEL","identity":"PRIVATE-SENTINEL","saves":"PRIVATE-SENTINEL"})).unwrap();
    fs::write(&input, &raw).unwrap();
    let out = t.path().join("share");
    let export = Command::new(env!("CARGO_BIN_EXE_session_report"))
        .arg(&input)
        .arg("--bundle")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stderr)
    );
    let check = Command::new(env!("CARGO_BIN_EXE_mp_doctor"))
        .arg("--diagnostics")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&check.stdout).unwrap()["validation_claimed"],
        false
    );
    for file in fs::read_dir(&out).unwrap() {
        let bytes = fs::read(file.unwrap().path()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("PRIVATE-SENTINEL") && !text.contains("private-name-status"));
    }
    assert_eq!(fs::read(&input).unwrap(), raw);
    fs::write(out.join("analysis.json"), b"{}").unwrap();
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_mp_doctor"))
            .arg("--diagnostics")
            .arg(&out)
            .output()
            .unwrap()
            .status
            .success()
    );
    let bad = t.path().join("private-data.json");
    fs::write(&bad, b"{\"save\":\"PRIVATE-SENTINEL\"}").unwrap();
    let absent = t.path().join("refused");
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_session_report"))
            .arg(bad)
            .arg("--bundle")
            .arg(&absent)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!absent.exists());
}

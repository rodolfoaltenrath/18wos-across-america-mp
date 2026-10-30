//! Exercise the actual launcher CLI while deliberately providing no DLL/PID.
//! Every career refusal must precede process access and backup creation.
#![cfg(windows)]
use multiplayer_server::{
    career::{Compatibility, Goal},
    career_entry::{self, Receipt, Registry},
    career_store as store, identity, session_protocol,
};

#[test]
fn launcher_checks_career_before_dll_process_and_backup_without_starting_a_game() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let save = root.join("own.sav");
    let map = root.join("own-map.scs");
    store::write_new(&save, b"own save fixture, not a commercial save").unwrap();
    store::write_new(&map, b"own map fixture, not a commercial archive").unwrap();
    let compatibility = Compatibility {
        profile: session_protocol::PROFILE.into(),
        map_sha256: career_entry::fingerprint(&map).unwrap(),
        mods_sha256: vec![],
    };
    let import = root.join("import");
    let manifest = store::create(&import, &save, Goal::Unlimited, compatibility).unwrap();
    let receipt = Receipt {
        schema: 1,
        career: manifest.career.clone(),
        manifest_sha256: career_entry::manifest_hash(&manifest).unwrap(),
        save_sha256: manifest.bootstrap.sha256.clone(),
        bytes: manifest.bootstrap.bytes,
        bootstrap_verified: true,
        engine_loaded: false,
        world_synchronized: false,
    };
    store::write_new(
        &import.join("receipt.json"),
        &store::json(&receipt).unwrap(),
    )
    .unwrap();
    let identity_path = root.join("identity.json");
    let identity = identity::load_or_create(&identity_path).unwrap();
    let original_identity = std::fs::read(&identity_path).unwrap();
    let relay = "127.0.0.1:39000".parse().unwrap();
    let registry = Registry::new(manifest, "career".into(), relay).unwrap();
    let launch = registry.issue(&identity, &receipt).unwrap();
    let launch_path = root.join("private-launch.json");
    store::write_new(&launch_path, &store::json(&launch).unwrap()).unwrap();
    let backup = root.join("must-not-be-created");
    let run = || {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mp_launcher"))
            .args(["--pid", "0", "--dll"])
            .arg(root.join("client_dll.dll")) // Intentionally absent, never injected.
            .args([
                "session-start",
                "--server",
                "127.0.0.1:39000",
                "--room",
                "career",
                "--identity-file",
            ])
            .arg(&identity_path)
            .arg("--career-launch")
            .arg(&launch_path)
            .arg("--career-import")
            .arg(&import)
            .arg("--career-map")
            .arg(&map)
            .arg("--backup")
            .arg(&backup)
            .arg("--saves")
            .arg(root)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!backup.exists());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains(&launch.proof.ticket));
        error
    };
    // Valid career inputs reach the deliberately missing DLL check, not a PID.
    assert!(run().contains("DLL must exist"));
    std::fs::write(
        &identity_path,
        store::json(&serde_json::json!({"schema":1,"identity":"f".repeat(32)})).unwrap(),
    )
    .unwrap();
    assert!(run().contains("differs from launch capability"));
    std::fs::write(&identity_path, original_identity).unwrap();
    std::fs::write(import.join("bootstrap.sav"), b"changed fixture").unwrap();
    let error = run();
    assert!(error.contains("does not match host manifest"));
    assert!(!error.contains("DLL must exist"));
    assert_eq!(
        std::fs::read(&save).unwrap(),
        b"own save fixture, not a commercial save"
    );
}

//! OUR renderer/fonts/resource cleanup only. This never starts/opens the game.
#![cfg(windows)]
#[test]
fn offscreen_ui_supports_portuguese_and_releases_its_gdi_resources() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mp_ui"))
        .arg("--self-test")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["screens"], 9);
    assert_eq!(r["pt_br_glyphs"], true);
    assert_eq!(r["game_tested"], false);
    assert_eq!(r["network_started"], false);
    assert!(r["gdi_after"].as_u64() <= r["gdi_before"].as_u64());
}
#[test]
fn layout_export_is_new_only_and_cannot_overwrite_previous_results() {
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("export");
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_mp_ui"))
            .arg("--export")
            .arg(&out)
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    let a = std::fs::read(out.join("pt-BR.json")).unwrap();
    assert!(std::str::from_utf8(&a).unwrap().contains("Configurações"));
    assert!(!run().status.success());
    assert_eq!(std::fs::read(out.join("pt-BR.json")).unwrap(), a);
    assert!(
        !std::fs::read_to_string(out.join("host.esc"))
            .unwrap()
            .contains("ui.handler")
    );
}

use super::*;
const CFG: &[u8] = b"# own configuration\nmount -o ro,p2 \"aa/base.scs\" /\nmount -o ro,p3 \"aa/patch_1_10.scs\" /\nmount -o ro,p1 \"data.scs\" /\n";
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let game = t.path().join("game");
    fs::create_dir_all(game.join("aa/lib")).unwrap();
    fs::write(game.join("aa/init.cfg"), CFG).unwrap();
    for name in &INPUTS[1..] {
        fs::write(game.join(name), format!("own fixture {name}")).unwrap();
    }
    (t, game)
}
fn prep(game: &Path, work: &Path) -> Report {
    prepare_with(game, &[], Component::Menu, None, work, generate).unwrap()
}
#[test]
fn embedded_catalogues_preserve_authored_translation_coverage() {
    let table = l10n::catalog::load(TABLE).unwrap();
    let literals = l10n::literals::load(LITERALS).unwrap();
    assert_eq!(table.entries.len(), 484);
    assert_eq!(
        literals
            .files
            .values()
            .map(|f| f.entries.len())
            .sum::<usize>(),
        264
    );
    assert_eq!(literals.files.len(), 263);
}
#[test]
fn preparation_uses_local_fingerprints_and_never_modifies_the_game() {
    let (t, game) = fixture();
    let before = inputs(&game).unwrap();
    let work = t.path().join("new");
    let report = prep(&game, &work);
    assert_eq!(report.files, 10);
    assert!(!report.installed && !report.engine_validated && report.locally_rebuilt);
    assert_eq!(check_prepared(&game, &work).unwrap(), Component::Menu);
    assert!(status(&game, &work).unwrap().is_none());
    assert!(prepare_with(&game, &[], Component::Menu, None, &work, generate).is_err());
    assert!(inputs(&game).unwrap() == before);
    assert!(!work.join("backup").exists());
    assert!(!game.join("aa/rust_mp_ui.scs").exists());
}
#[test]
fn failed_or_changed_input_preparation_stays_incomplete_and_cannot_install() {
    let (t, game) = fixture();
    let output = t.path().join("changed");
    assert!(
        prepare_with(&game, &[], Component::Menu, None, &output, |s, c, f, p| {
            let result = generate(s, c, f, p)?;
            fs::write(game.join("data.scs"), b"own changed fixture")?;
            Ok(result)
        })
        .is_err()
    );
    assert!(output.join(".INCOMPLETE").exists());
    assert!(check_prepared(&game, &output).is_err());
    assert!(!output.join("prepared.json").exists());
    let failed = t.path().join("failed");
    assert!(
        prepare_with(
            &game,
            &[],
            Component::Menu,
            None,
            &failed,
            |_, _, _, _| anyhow::bail!("owned generation failure")
        )
        .is_err()
    );
    assert!(failed.join(".INCOMPLETE").exists());
    assert!(!game.join("aa/rust_mp_ui.scs").exists());
}
#[test]
fn wrong_context_unknown_fields_modified_package_and_changed_sources_are_refused() {
    let (t, game) = fixture();
    let work = t.path().join("new");
    prep(&game, &work);
    let path = work.join("prepared.json");
    let before = fs::read(&path).unwrap();
    let mut plan: serde_json::Value = serde_json::from_slice(&before).unwrap();
    plan["engine_validated"] = serde_json::json!(true);
    fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(check_prepared(&game, &work).is_err());
    fs::write(&path, &before).unwrap();
    let mut plan: serde_json::Value = serde_json::from_slice(&before).unwrap();
    plan["inputs"][0]["name"] = serde_json::json!("../other");
    fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    assert!(check_prepared(&game, &work).is_err());
    fs::write(&path, &before).unwrap();
    let other = t.path().join("other");
    fs::create_dir(&other).unwrap();
    assert!(check_prepared(&other, &work).is_err());
    fs::write(game.join("data.scs"), b"own source changed").unwrap();
    assert!(check_prepared(&game, &work).is_err());
    assert!(!work.join("backup").exists());
    let payload = work.join("package/overlay.scs");
    fs::write(&payload, b"own damaged payload").unwrap();
    assert!(check_prepared(&game, &work).is_err());
}
#[test]
fn setup_sources_reject_unknown_mounts_duplicates_and_menu_after_translation() {
    let (_t, game) = fixture();
    let expected = ui::hash(&fs::read(game.join("aa/lib/game.dll")).unwrap());
    let check = |kind| mount::setup_sources_with_hash(&game, kind, &expected);
    assert_eq!(check(mount::Overlay::PtBrText).unwrap().1.len(), 3);
    assert!(mount::setup_sources(&game, mount::Overlay::PtBrText).is_err()); // actual profile still mandatory
    let menu = b"mount -o ro,p4 \"aa/rust_mp_ui.scs\" /\n";
    let mut config = CFG.to_vec();
    config.extend(menu);
    fs::write(game.join("aa/init.cfg"), &config).unwrap();
    assert!(check(mount::Overlay::PtBrText).is_ok());
    assert!(check(mount::Overlay::UiPreview).is_err());
    config.extend(menu);
    fs::write(game.join("aa/init.cfg"), &config).unwrap();
    assert!(check(mount::Overlay::PtBrText).is_err());
    let mut config = CFG.to_vec();
    config.extend(b"mount -o ro,p5 \"aa/rust_mp_ptbr.scs\" /\n");
    fs::write(game.join("aa/init.cfg"), &config).unwrap();
    assert!(check(mount::Overlay::UiPreview).is_err());
    let mut config = CFG.to_vec();
    config.extend(b"mount -o ro,p4 \"unknown.scs\" /\n");
    fs::write(game.join("aa/init.cfg"), &config).unwrap();
    assert!(check(mount::Overlay::PtBrText).is_err());
}
#[cfg(unix)]
#[test]
fn setup_never_follows_source_or_work_directory_aliases() {
    let (t, game) = fixture();
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let link = t.path().join("linked");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert!(
        prepare_with(
            &game,
            &[],
            Component::Menu,
            None,
            &link.join("new"),
            generate
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_file(game.join("data.scs")).unwrap();
    std::os::unix::fs::symlink(t.path().join("absent"), game.join("data.scs")).unwrap();
    assert!(
        prepare_with(
            &game,
            &[],
            Component::Menu,
            None,
            &t.path().join("new"),
            generate
        )
        .is_err()
    );
    assert!(!t.path().join("new").exists());
}

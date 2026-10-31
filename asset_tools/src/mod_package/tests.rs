use super::*;
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    project: PathBuf,
    package: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("project");
        example(&source).unwrap();
        Self {
            project: source.join("project.json"),
            package: temp.path().join("package"),
            temp,
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    fn edit(&self, change: impl FnOnce(&mut Project)) {
        let mut project: Project =
            serde_json::from_slice(&fs::read(&self.project).unwrap()).unwrap();
        change(&mut project);
        fs::write(&self.project, serde_json::to_vec_pretty(&project).unwrap()).unwrap();
    }
    fn build(&self) {
        build(&self.project, &self.package, true).unwrap();
    }
    fn source(&self, name: &str) -> PathBuf {
        self.project
            .parent()
            .unwrap()
            .join("inputs/owned")
            .join(name)
    }
    fn edit_manifest(&self, change: impl FnOnce(&mut Manifest)) {
        let path = self.package.join(MANIFEST);
        let mut manifest: Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut manifest);
        fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }
}

#[test]
fn deterministic_across_roots_order_and_reuse_preserves_output() {
    let a = Fixture::new();
    let b = Fixture::new();
    b.edit(|p| p.files.reverse());
    OpenOptions::new()
        .write(true)
        .open(b.source("checker.png"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH))
        .unwrap();
    assert!(!build(&a.project, &a.package, true).unwrap().reused);
    b.build();
    assert_eq!(contents(&a.package).unwrap(), contents(&b.package).unwrap());
    let modified = fs::metadata(a.package.join(PAYLOAD))
        .unwrap()
        .modified()
        .unwrap();
    assert!(build(&a.project, &a.package, true).unwrap().reused);
    assert_eq!(
        modified,
        fs::metadata(a.package.join(PAYLOAD))
            .unwrap()
            .modified()
            .unwrap()
    );
    let manifest = verify(&a.package, true).unwrap();
    assert!(manifest.distributable);
    assert_eq!(manifest.files.len(), 2);
    assert!(
        !String::from_utf8(fs::read(a.package.join(MANIFEST)).unwrap())
            .unwrap()
            .contains(&a.temp.path().to_string_lossy().to_string())
    );
}

#[test]
fn cross_host_envelope_is_fixed_and_legacy_windows_packages_remain_strictly_verifiable() {
    let f = Fixture::new();
    f.build();
    let (json, canonical) = contents(&f.package).unwrap();
    let mut m: Manifest = serde_json::from_slice(&json).unwrap();
    let files: BTreeMap<_, _> = m
        .files
        .iter()
        .map(|e| {
            (
                e.input.path.clone(),
                fs::read(f.project.parent().unwrap().join(&e.input.source)).unwrap(),
            )
        })
        .collect();
    let central = canonical
        .windows(4)
        .position(|p| p == b"PK\x01\x02")
        .unwrap();
    assert_eq!(
        canonical[central + 5],
        3,
        "ZIP creator must be Unix on every host"
    );
    let legacy = archive_for_system(&files, zip::System::Dos).unwrap();
    assert_ne!(canonical, legacy);
    m.payload_sha256 = hash(&legacy);
    let legacy_json = serde_json::to_vec_pretty(&m).unwrap();
    validate(&legacy_json, &legacy, true).unwrap();
    fs::write(f.package.join(MANIFEST), &legacy_json).unwrap();
    fs::write(f.package.join(PAYLOAD), &legacy).unwrap();
    let staged = f.path("legacy-staged");
    stage(&f.package, &staged, true).unwrap();
    assert_eq!(
        contents(&staged).unwrap(),
        (legacy_json.clone(), legacy.clone())
    );
    unstage(&staged, &f.path("legacy-backup")).unwrap();
    assert_eq!(
        contents(&f.path("legacy-backup")).unwrap(),
        (legacy_json.clone(), legacy.clone())
    );
    assert!(build(&f.project, &f.package, true).is_err());
    assert_eq!(contents(&f.package).unwrap(), (legacy_json, legacy.clone()));
    // A valid manifest hash must not make appended/hidden ZIP bytes canonical.
    let mut appended = legacy;
    appended.extend_from_slice(b"own hidden trailer");
    m.payload_sha256 = hash(&appended);
    assert!(validate(&serde_json::to_vec_pretty(&m).unwrap(), &appended, true).is_err());
}

#[test]
fn source_bytes_and_png_alpha_are_preserved_without_conversion() {
    let f = Fixture::new();
    f.build();
    let payload = fs::read(f.package.join(PAYLOAD)).unwrap();
    let mut zip = ZipArchive::new(Cursor::new(payload)).unwrap();
    let mut bytes = Vec::new();
    zip.by_name("textures/mp/checker.png")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, fs::read(f.source("checker.png")).unwrap());
    let image = image::load_from_memory(&bytes).unwrap().into_rgba8();
    assert_eq!(image.dimensions(), (2, 2));
    assert_eq!(image.get_pixel(0, 0).0, [255, 128, 0, 255]);
    assert_eq!(image.get_pixel(1, 0).0, [0, 128, 255, 64]);
}

#[test]
fn changed_input_requires_new_output_and_dependency_hash_changes() {
    let f = Fixture::new();
    f.build();
    let original = contents(&f.package).unwrap();
    fs::write(f.source("notes.txt"), b"new own text\n").unwrap();
    assert!(build(&f.project, &f.package, true).is_err());
    assert_eq!(contents(&f.package).unwrap(), original);
    let newer = build(&f.project, &f.path("new-output"), true).unwrap();
    assert!(!newer.reused);
    let old = verify(&f.package, true).unwrap();
    assert_ne!(old.payload_sha256, newer.manifest.payload_sha256);
    assert_ne!(old.files[0].sha256, newer.manifest.files[0].sha256);
    assert_eq!(old.files[1].sha256, newer.manifest.files[1].sha256);
}

#[test]
fn local_derivation_is_not_redistributable_and_origin_is_mandatory() {
    let f = Fixture::new();
    let local = f.project.parent().unwrap().join("inputs/local");
    fs::create_dir(&local).unwrap();
    fs::write(
        local.join("notes.txt"),
        b"own fixture standing in for a local derivation",
    )
    .unwrap();
    f.edit(|p| {
        p.files[1].origin = Origin::DerivedLocal;
        p.files[1].source = "inputs/local/notes.txt".into();
    });
    assert!(build(&f.project, &f.package, true).is_err());
    assert!(!f.package.exists());
    assert!(
        !build(&f.project, &f.package, false)
            .unwrap()
            .manifest
            .distributable
    );
    assert!(verify(&f.package, true).is_err());
    assert!(stage(&f.package, &f.path("distributed"), true).is_err());
    assert!(!f.path("distributed").exists());
    stage(&f.package, &f.path("local-stage"), false).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&f.project).unwrap()).unwrap();
    value["files"][0].as_object_mut().unwrap().remove("origin");
    fs::write(&f.project, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(build(&f.project, &f.path("missing-origin"), false).is_err());
}

#[test]
fn source_origin_directories_are_enforced() {
    let f = Fixture::new();
    f.edit(|p| p.files[0].origin = Origin::DerivedLocal);
    assert!(build(&f.project, &f.package, false).is_err());
    assert!(!f.package.exists());
}

#[test]
fn dependency_graph_rejects_missing_cycles_duplicates_and_case_mismatch() {
    for dep in [
        "textures/mp/missing.png",
        "locale/mp/notes.txt",
        "Textures/mp/checker.png",
    ] {
        let f = Fixture::new();
        f.edit(|p| p.files[1].depends_on = vec![dep.into()]);
        assert!(
            build(&f.project, &f.package, true).is_err(),
            "accepted {dep}"
        );
        assert!(!f.package.exists());
    }
    let f = Fixture::new();
    f.edit(|p| p.files[0].depends_on = vec!["locale/mp/notes.txt".into()]);
    assert!(build(&f.project, &f.package, true).is_err());
    let f = Fixture::new();
    f.edit(|p| p.files[1].depends_on.push("textures/mp/checker.png".into()));
    assert!(build(&f.project, &f.package, true).is_err());
}

#[test]
fn portable_path_collisions_and_unsafe_paths_fail_before_output() {
    for path in [
        "../escape.txt",
        "AUX.txt",
        "C:/escape.txt",
        "textures\\escape.png",
        "textures/x .png/../x.png",
        "saves/a.txt",
        "textures/á.png",
        "textures/x.png/",
        "prism3d.exe",
        "base.scs",
        "locale/a.txt:stream",
    ] {
        let f = Fixture::new();
        f.edit(|p| {
            p.files[0].path = path.into();
            p.files[1].depends_on.clear();
        });
        assert!(
            build(&f.project, &f.package, true).is_err(),
            "accepted {path}"
        );
        assert!(!f.package.exists());
    }
    for paths in [
        ["a.txt", "A.txt"],
        ["a.txt", "a.txt/child.txt"],
        ["a.txt", "A.TXT/child.txt"],
    ] {
        let f = Fixture::new();
        f.edit(|p| {
            p.files[0].path = paths[0].into();
            p.files[1].path = paths[1].into();
            p.files[1].depends_on.clear();
        });
        assert!(build(&f.project, &f.package, true).is_err());
    }
}

#[test]
fn disguised_executables_archives_and_binary_text_are_rejected() {
    for bytes in [
        b"MZfixture".as_slice(),
        b"\x7fELFfixture",
        b"PK\x03\x04fixture",
        b"bad\0text",
    ] {
        let f = Fixture::new();
        fs::write(f.source("notes.txt"), bytes).unwrap();
        assert!(build(&f.project, &f.package, true).is_err());
        assert!(!f.package.exists());
    }
}

#[test]
fn single_byte_tables_and_fonts_allow_accents_but_not_controls() {
    assert!(data_only("scripts/local.def", b"k:\"A\xe7\xe3o\"\r\n").is_ok());
    assert!(data_only("fonts/own.font", b"$227, 1, 2, 3, 4, 0, 1, 5 # \xe3\n").is_ok());
    assert!(data_only("scripts/local.def", b"k:\"\x07\"").is_err());
    assert!(data_only("fonts/own.font", b"\x7f").is_err());
    // Script/shader text stays UTF-8.
    assert!(data_only("ui/own.esc", b"\xe3").is_err());
    assert!(asset_path("fonts/own.font").is_ok());
}

#[test]
fn size_count_and_schema_limits_are_enforced() {
    let f = Fixture::new();
    OpenOptions::new()
        .write(true)
        .open(f.source("checker.png"))
        .unwrap()
        .set_len(MAX_FILE as u64 + 1)
        .unwrap();
    assert!(build(&f.project, &f.package, true).is_err());
    let f = Fixture::new();
    f.edit(|p| p.files = vec![p.files[0].clone(); MAX_FILES + 1]);
    assert!(build(&f.project, &f.package, true).is_err());
    let f = Fixture::new();
    f.edit(|p| p.schema = 2);
    assert!(build(&f.project, &f.package, true).is_err());
    let f = Fixture::new();
    fs::write(&f.project, vec![b' '; MAX_JSON + 1]).unwrap();
    assert!(build(&f.project, &f.package, true).is_err());
}

#[test]
fn tampering_cannot_be_staged_or_removed() {
    let f = Fixture::new();
    f.build();
    let mut payload = fs::read(f.package.join(PAYLOAD)).unwrap();
    payload[40] ^= 1;
    fs::write(f.package.join(PAYLOAD), payload).unwrap();
    assert!(verify(&f.package, false).is_err());
    assert!(stage(&f.package, &f.path("stage"), false).is_err());
    assert!(unstage(&f.package, &f.path("backup")).is_err());
    assert!(!f.path("stage").exists());
    assert!(!f.path("backup").exists());
    assert!(f.package.exists());
}

#[test]
fn recomputing_outer_hash_does_not_allow_extra_data_or_forged_inner_hashes() {
    let f = Fixture::new();
    f.build();
    let mut payload = fs::read(f.package.join(PAYLOAD)).unwrap();
    payload.extend_from_slice(b"unlisted data");
    fs::write(f.package.join(PAYLOAD), &payload).unwrap();
    f.edit_manifest(|m| m.payload_sha256 = hash(&payload));
    assert!(verify(&f.package, true).is_err());
    let f = Fixture::new();
    f.build();
    f.edit_manifest(|m| m.files[0].sha256 = "0".repeat(64));
    assert!(verify(&f.package, true).is_err());
    let f = Fixture::new();
    f.build();
    f.edit_manifest(|m| m.distributable = false);
    assert!(verify(&f.package, false).is_err());
}

#[test]
fn unlisted_archive_entries_and_nonregular_attributes_are_rejected() {
    let f = Fixture::new();
    f.build();
    let mut files = BTreeMap::new();
    files.insert(
        "locale/mp/notes.txt".to_owned(),
        fs::read(f.source("notes.txt")).unwrap(),
    );
    files.insert(
        "textures/mp/checker.png".to_owned(),
        fs::read(f.source("checker.png")).unwrap(),
    );
    files.insert("unlisted.txt".to_owned(), b"hidden".to_vec());
    let payload = archive(&files).unwrap();
    fs::write(f.package.join(PAYLOAD), &payload).unwrap();
    f.edit_manifest(|m| m.payload_sha256 = hash(&payload));
    assert!(verify(&f.package, true).is_err());
    let f = Fixture::new();
    f.build();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink(
        "locale/mp/notes.txt",
        "outside",
        SimpleFileOptions::default(),
    )
    .unwrap();
    zip.start_file("textures/mp/checker.png", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&fs::read(f.source("checker.png")).unwrap())
        .unwrap();
    let payload = zip.finish().unwrap().into_inner();
    fs::write(f.package.join(PAYLOAD), &payload).unwrap();
    f.edit_manifest(|m| m.payload_sha256 = hash(&payload));
    assert!(verify(&f.package, false).is_err());
}

#[test]
fn stage_backup_remove_and_restore_preserve_every_byte() {
    let f = Fixture::new();
    f.build();
    let original = contents(&f.package).unwrap();
    fs::write(f.path("unrelated-save.sav"), b"untouched fixture").unwrap();
    stage(&f.package, &f.path("stage"), true).unwrap();
    assert_eq!(contents(&f.path("stage")).unwrap(), original);
    unstage(&f.path("stage"), &f.path("backup")).unwrap();
    assert!(!f.path("stage").exists());
    assert_eq!(contents(&f.path("backup")).unwrap(), original);
    stage(&f.path("backup"), &f.path("stage"), true).unwrap();
    assert_eq!(contents(&f.path("stage")).unwrap(), original);
    assert_eq!(
        fs::read(f.path("unrelated-save.sav")).unwrap(),
        b"untouched fixture"
    );
    assert_eq!(contents(&f.package).unwrap(), original);
}

#[test]
fn existing_destinations_and_extra_user_files_are_never_overwritten() {
    let f = Fixture::new();
    f.build();
    let original = contents(&f.package).unwrap();
    fs::create_dir(f.path("existing")).unwrap();
    fs::write(f.path("existing/user.txt"), b"keep").unwrap();
    assert!(stage(&f.package, &f.path("existing"), true).is_err());
    assert!(unstage(&f.package, &f.path("existing")).is_err());
    assert_eq!(contents(&f.package).unwrap(), original);
    assert_eq!(fs::read(f.path("existing/user.txt")).unwrap(), b"keep");
    fs::write(f.package.join("user.txt"), b"extra").unwrap();
    assert!(unstage(&f.package, &f.path("backup")).is_err());
    assert!(!f.path("backup").exists());
    assert_eq!(fs::read(f.package.join("user.txt")).unwrap(), b"extra");
}

#[test]
fn partial_packages_and_missing_parents_are_not_reused_or_repaired_implicitly() {
    let f = Fixture::new();
    fs::create_dir(&f.package).unwrap();
    fs::write(f.package.join(PAYLOAD), b"partial").unwrap();
    assert!(build(&f.project, &f.package, true).is_err());
    assert_eq!(fs::read(f.package.join(PAYLOAD)).unwrap(), b"partial");
    assert!(build(&f.project, &f.path("missing/child"), true).is_err());
    assert!(!f.path("missing").exists());
    assert!(build(&f.project, &f.source("nested-output"), true).is_err());
}

#[cfg(windows)]
#[test]
fn failed_removal_keeps_a_verified_backup_for_recovery_to_a_new_directory() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.build();
    let manifest_path = f.package.join(MANIFEST);
    // Allow readers/writers but deny deletion until this handle is dropped.
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0x1 | 0x2)
        .open(&manifest_path)
        .unwrap();
    assert!(unstage(&f.package, &f.path("backup")).is_err());
    assert!(!f.package.join(PAYLOAD).exists());
    assert!(manifest_path.exists());
    verify(&f.path("backup"), true).unwrap();
    stage(&f.path("backup"), &f.path("recovered"), true).unwrap();
    assert_eq!(
        contents(&f.path("backup")).unwrap(),
        contents(&f.path("recovered")).unwrap()
    );
    drop(held);
}

#[cfg(unix)]
#[test]
fn symlink_input_and_ancestor_are_rejected() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let input = f.source("notes.txt");
    fs::rename(&input, f.path("outside.txt")).unwrap();
    symlink(f.path("outside.txt"), &input).unwrap();
    assert!(build(&f.project, &f.package, true).is_err());
    symlink(f.project.parent().unwrap(), f.path("linked-project")).unwrap();
    assert!(build(&f.path("linked-project/project.json"), &f.package, true).is_err());
}

#[cfg(windows)]
#[test]
fn reparse_point_input_is_rejected_when_symlink_creation_is_available() {
    let f = Fixture::new();
    let link = f.source("linked.txt");
    match std::os::windows::fs::symlink_file(f.source("notes.txt"), &link) {
        Ok(()) => {
            f.edit(|p| p.files[1].source = "inputs/owned/linked.txt".into());
            assert!(build(&f.project, &f.package, true).is_err());
        }
        Err(e)
            if e.kind() == std::io::ErrorKind::PermissionDenied
                || e.raw_os_error() == Some(1314) =>
        {
            eprintln!("symlink fixture unavailable without Windows developer mode/privilege");
        }
        Err(e) => panic!("unexpected symlink error: {e}"),
    }
}

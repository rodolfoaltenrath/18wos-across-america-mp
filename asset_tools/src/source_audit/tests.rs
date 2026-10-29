use super::*;

fn repository() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "--quiet"], 1024).unwrap();
    write(temp.path(), "Cargo.toml", b"[workspace]\n");
    write(temp.path(), "asset_tools/src/lib.rs", b"pub fn own() {}\n");
    git(temp.path(), &["add", "--all"], 1024).unwrap();
    git(
        temp.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=.no-fixture-hooks",
            "commit",
            "--quiet",
            "-m",
            "Own fixture",
        ],
        1024,
    )
    .unwrap();
    temp
}

fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn stage_and_head_audit_exact_git_blobs_not_dirty_worktree() {
    let temp = repository();
    write(
        temp.path(),
        "asset_tools/src/lib.rs",
        b"MZrenamed commercial executable",
    );
    assert!(audit(temp.path(), Scope::Worktree).is_err());
    let staged = audit(temp.path(), Scope::Index).unwrap();
    let head = audit(temp.path(), Scope::Head).unwrap();
    assert!(staged.dirty && head.dirty);
    assert_eq!(staged.files[1].sha256, head.files[1].sha256);
    git(temp.path(), &["add", "asset_tools/src/lib.rs"], 1024).unwrap();
    assert!(audit(temp.path(), Scope::Index).is_err());
    assert!(audit(temp.path(), Scope::Head).is_ok());
}

#[test]
fn forced_game_and_private_paths_fail_even_with_ignore_rules() {
    let temp = repository();
    write(temp.path(), ".gitignore", b"*.scs\n");
    write(temp.path(), "base.scs", b"own dummy, not a game file");
    assert!(audit(temp.path(), Scope::Worktree).is_ok());
    git(temp.path(), &["add", "-f", "base.scs"], 1024).unwrap();
    assert!(audit(temp.path(), Scope::Index).is_err());
    for path in [
        "../bad.rs",
        "client_dll/src/AUX.rs",
        "client_dll/src/.env",
        "client_dll/src/generated/x.rs",
        "client_dll/src/own.rs/",
        "docs/private.json",
        "base.scs",
        ".cargo/config.toml",
        "client_dll/src/../x.rs",
    ] {
        assert!(allowed(path).is_err(), "accepted {path}");
    }
}

#[test]
fn case_collisions_untracked_private_files_and_subroots_are_rejected() {
    let temp = repository();
    write(temp.path(), "private.json", b"{}");
    assert!(audit(temp.path(), Scope::Worktree).is_err());
    fs::remove_file(temp.path().join("private.json")).unwrap();
    let blob = git(
        temp.path(),
        &["rev-parse", "HEAD:asset_tools/src/lib.rs"],
        128,
    )
    .unwrap();
    let blob = std::str::from_utf8(&blob).unwrap().trim();
    git(
        temp.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},asset_tools/src/LIB.rs"),
        ],
        1024,
    )
    .unwrap();
    assert!(audit(temp.path(), Scope::Index).is_err());
    assert!(audit(&temp.path().join("asset_tools"), Scope::Head).is_err());
}

#[test]
fn git_symlinks_binary_content_and_large_sources_are_refused() {
    let temp = repository();
    let blob = git(
        temp.path(),
        &["rev-parse", "HEAD:asset_tools/src/lib.rs"],
        128,
    )
    .unwrap();
    let blob = std::str::from_utf8(&blob).unwrap().trim();
    git(
        temp.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},asset_tools/src/link.rs"),
        ],
        1024,
    )
    .unwrap();
    assert!(audit(temp.path(), Scope::Index).is_err());
    for bytes in [
        b"PK\x03\x04 disguised archive".as_slice(),
        b"\x89PNG",
        b"a\0b",
        &[255, 255],
    ] {
        assert!(text_only("asset_tools/src/own.rs", bytes).is_err());
    }
    assert!(text_only("asset_tools/src/own.rs", &vec![b'x'; MAX_FILE + 1]).is_err());
}

#[test]
fn head_bundle_is_deterministic_read_only_and_excludes_later_private_edits() {
    let temp = repository();
    let out = tempfile::tempdir().unwrap();
    let a = out.path().join("a.zip");
    let b = out.path().join("b.zip");
    let before = fs::read(temp.path().join("asset_tools/src/lib.rs")).unwrap();
    bundle(temp.path(), &a).unwrap();
    write(temp.path(), "private.json", b"credentials must stay out");
    write(temp.path(), "asset_tools/src/lib.rs", b"MZdirty worktree");
    bundle(temp.path(), &b).unwrap();
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let manifest = verify_bundle(&a).unwrap();
    assert_eq!(manifest.files.len(), 2);
    assert_eq!(manifest.files[1].sha256, hash(&before));
    assert!(bundle(temp.path(), &a).is_err());
    assert_eq!(
        fs::read(temp.path().join("private.json")).unwrap(),
        b"credentials must stay out"
    );
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 2);
}

fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn verify_rejects_traversal_unlisted_paths_changed_hashes_and_duplicate_manifests() {
    let temp = repository();
    let out = tempfile::tempdir().unwrap();
    let original = out.path().join("original.zip");
    let manifest = bundle(temp.path(), &original).unwrap();
    let metadata = serde_json::to_vec(&manifest).unwrap();
    let bad = out.path().join("bad.zip");
    for name in ["../escape.rs", "base.scs", "SOURCE_manifest.json"] {
        archive(&bad, &[(MANIFEST, &metadata), (name, b"bad")]);
        assert!(verify_bundle(&bad).is_err(), "accepted {name}");
    }
    archive(
        &bad,
        &[
            (MANIFEST, &metadata),
            ("Cargo.toml", b"tampered"),
            ("asset_tools/src/lib.rs", b"pub fn own() {}\n"),
        ],
    );
    assert!(verify_bundle(&bad).is_err());
    assert!(!out.path().join("escape.rs").exists());
    let mut duplicate = manifest;
    duplicate.files[1] = duplicate.files[0].clone();
    let metadata = serde_json::to_vec(&duplicate).unwrap();
    archive(
        &bad,
        &[
            (MANIFEST, &metadata),
            ("Cargo.toml", b"[workspace]\n"),
            ("asset_tools/src/lib.rs", b"pub fn own() {}\n"),
        ],
    );
    assert!(verify_bundle(&bad).is_err());
}

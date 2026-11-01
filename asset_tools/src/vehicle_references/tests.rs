use super::*;
use std::{fs::File, io::Write, path::Path};
use zip::{ZipWriter, write::SimpleFileOptions};
fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}
fn fixture(path: &Path) -> (Manifest, Vec<Input>) {
    archive(path,&[
        ("makes/truck/own/description.def",br#"def own { name:"Private prose sentinel" access0:"cabin" access1:"base" }"#),
        ("makes/truck/own/acc/cabin/!desc.def",br#"extdefs "/own_parent.def" def own {model:"own.psm" shader:"paint.shd" type:"cabin" offset:<1 2 3>}"#),
        ("makes/truck/own/acc/base/!desc.def",br#"def own { name:"unterminated }"#),
        ("makes/truck/own/acc/cabin/own.psm",b"owned fake model"),
        ("makes/truck/own/acc/cabin/paint.shd",b"owned local shader"),
        ("makes/truck/own/acc/shared/paint.shd",b"owned shared shader"),
        ("own_parent.def",br#"extdefs "/makes/truck/own/acc/cabin/!desc.def" def own {type:"own"}"#),
    ]);
    let inputs = vec![Input {
        role: multiplayer_server::world_content::Role::Map,
        path: path.into(),
    }];
    let content =
        crate::world_content::inventory(multiplayer_server::session_protocol::PROFILE, &inputs)
            .unwrap();
    (content, inputs)
}
#[test]
fn known_candidates_remain_ambiguous_and_malformed_definitions_stay_unparsed() {
    let t = tempfile::tempdir().unwrap();
    let (c, i) = fixture(&t.path().join("own.scs"));
    let a = audit(&c, &i, "own").unwrap();
    assert_eq!(a.definitions.len(), 4);
    assert_eq!(
        a.definitions
            .iter()
            .filter(|d| d.status == Status::LexingRefused)
            .count(),
        1
    );
    assert_eq!(
        a.definitions
            .iter()
            .flat_map(|d| &d.references)
            .filter(|r| r.resolution == Resolution::Ambiguous)
            .count(),
        1
    );
    assert_eq!(a.deduplicated_targets, 1);
    assert!(
        a.offset_not_interpreted
            && a.inheritance_not_applied
            && !a.engine_verified
            && !a.world_synchronized
    );
    let encoded = serde_json::to_string(&a).unwrap();
    for sentinel in [
        "Private prose sentinel",
        "paint.shd",
        "own.psm",
        t.path().to_str().unwrap(),
    ] {
        assert!(!encoded.contains(sentinel));
    }
}
#[test]
fn report_is_new_only_and_hash_changed_archive_never_publishes_output() {
    let t = tempfile::tempdir().unwrap();
    let archive_path = t.path().join("own.scs");
    let (c, i) = fixture(&archive_path);
    let output = t.path().join("report.json");
    let summary = create(&c, &i, "own", &output).unwrap();
    assert_eq!(summary.ambiguous, 1);
    assert!(create(&c, &i, "own", &output).is_err());
    archive(
        &archive_path,
        &[("makes/truck/own/description.def", b"changed")],
    );
    let output2 = t.path().join("changed.json");
    assert!(create(&c, &i, "own", &output2).is_err());
    assert!(!output2.exists());
}
#[test]
fn reference_lexer_ignores_prose_and_refuses_escaped_ambiguous_or_excessive_fields() {
    let fields = literals::reference_fields(
        br#"# model:"ignored"
        def own {name:"model: not a field" model:"own.psm" shader:"x.shd:y.shd"}"#,
    )
    .unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields["model"], "own.psm");
    for bytes in [
        br#"model:"a" model:"b""#.as_slice(),
        br#"model:"x\y""#,
        br#"name:"unterminated"#,
    ] {
        assert!(literals::reference_fields(bytes).is_err());
    }
    let mut rng = multiplayer_server::fuzz::Rng::new(417);
    let seed = br#"def own { model:"own.psm" shader:"own.shd" }"#;
    for _ in 0..6000 {
        let _ = literals::reference_fields(&multiplayer_server::fuzz::mutate(&mut rng, seed));
    }
}
#[test]
fn absent_or_unsafe_vehicle_is_not_invented_and_sparse_reports_are_explicit() {
    let t = tempfile::tempdir().unwrap();
    let (c, i) = fixture(&t.path().join("own.scs"));
    assert!(audit(&c, &i, "../other").is_err());
    let absent = audit(&c, &i, "absent").unwrap();
    assert_eq!(absent.definitions[0].status, Status::Missing);
}

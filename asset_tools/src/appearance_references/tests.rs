use super::*;
use multiplayer_server::vehicle_appearance::observation::{Slot, Source};
use std::{fs::File, io::Write};
use zip::{ZipWriter, write::SimpleFileOptions};
fn fixture(path: &Path) -> (Manifest, Vec<Input>, Observation) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in [
        (
            "makes/truck/own/description.def",
            br#"def own {name:"Own private prose sentinel" access0:"default_part"}"#.as_slice(),
        ),
        (
            "makes/truck/own/acc/default_part/!desc.def",
            br#"def own {model:"root.psm" type:"cabin"}"#,
        ),
        (
            "makes/truck/own/acc/default_part/root.psm",
            b"own fake geometry",
        ),
        (
            "makes/truck/own/acc/selected_part/!desc.def",
            br#"def own {type:"paint" shader:"alternate.shd"}"#,
        ),
        (
            "makes/truck/own/acc/selected_part/alternate.shd",
            b"own fake material",
        ),
        (
            "makes/truck/another/acc/cabin/!desc.def",
            br#"def own {type:"cabin"}"#,
        ),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    let inputs = vec![Input {
        role: multiplayer_server::world_content::Role::Map,
        path: path.into(),
    }];
    let content =
        crate::world_content::inventory(multiplayer_server::session_protocol::PROFILE, &inputs)
            .unwrap();
    let observation = Observation {
        schema: 1,
        profile: content.profile.clone(),
        source: Source::ProfiledActorNameCandidates,
        make_definition_key: resource_key("makes/truck/own/description.def").unwrap(),
        slots: vec![
            Slot {
                index: 0,
                definition_key: Some(
                    resource_key("makes/truck/own/acc/default_part/!desc.def").unwrap(),
                ),
            },
            Slot {
                index: 1,
                definition_key: None,
            },
            Slot {
                index: 2,
                definition_key: Some(
                    resource_key("makes/truck/own/acc/selected_part/!desc.def").unwrap(),
                ),
            },
            Slot {
                index: 3,
                definition_key: Some(
                    resource_key("makes/truck/own/acc/missing/!desc.def").unwrap(),
                ),
            },
        ],
        candidate_only: true,
        engine_verified: false,
        world_synchronized: false,
    };
    (content, inputs, observation)
}
#[test]
fn candidate_variant_is_audited_beside_defaults_without_merging_or_disclosing_names() {
    let t = tempfile::tempdir().unwrap();
    let (c, i, o) = fixture(&t.path().join("own.scs"));
    let default = vehicle_references::audit(&c, &i, "own").unwrap();
    assert_eq!(default.definitions.len(), 2);
    let report = audit(&c, &i, &o).unwrap();
    assert!(
        report.make_in_content
            && report.graph_includes_declared_defaults
            && report.candidate_only
            && !report.engine_verified
            && !report.world_synchronized
    );
    assert_eq!(report.slots.len(), 4);
    assert!(report.slots[0].available && report.slots[2].available);
    assert!(!report.slots[1].available && !report.slots[3].available);
    assert_eq!(report.references.as_ref().unwrap().definitions.len(), 3);
    assert_eq!(report.observation_sha256, o.fingerprint().unwrap());
    let json = serde_json::to_string(&report).unwrap();
    for raw in [
        "Own private prose sentinel",
        "selected_part",
        "alternate.shd",
        "root.psm",
        t.path().to_str().unwrap(),
    ] {
        assert!(!json.contains(raw));
    }
    let output = t.path().join("report.json");
    let summary = create(&c, &i, &o, &output).unwrap();
    assert_eq!(
        (
            summary.definitions,
            summary.unnamed_slots,
            summary.missing_slots
        ),
        (3, 1, 1)
    );
    assert!(create(&c, &i, &o, &output).is_err());
}
#[test]
fn missing_make_is_explicit_and_never_replaced_by_an_unrelated_default() {
    let t = tempfile::tempdir().unwrap();
    let (c, i, mut o) = fixture(&t.path().join("own.scs"));
    o.make_definition_key = resource_key("makes/truck/missing/description.def").unwrap();
    o.slots.truncate(2);
    o.slots[0].definition_key =
        Some(resource_key("makes/truck/missing/acc/unknown/!desc.def").unwrap());
    let report = audit(&c, &i, &o).unwrap();
    assert!(!report.make_in_content && report.references.is_none());
    assert!(!report.slots[0].available);
    let path = t.path().join("sparse.scs");
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    zip.start_file(
        "makes/truck/missing/acc/unknown/!desc.def",
        SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"def own {type:\"paint\"}").unwrap();
    zip.finish().unwrap();
    let inputs = vec![Input {
        role: multiplayer_server::world_content::Role::Map,
        path,
    }];
    let mut sparse =
        crate::world_content::inventory(multiplayer_server::session_protocol::PROFILE, &inputs)
            .unwrap();
    let report = audit(&sparse, &inputs, &o).unwrap();
    assert!(!report.make_in_content && report.slots[0].available && report.references.is_none());
    sparse.layers[0].resources[0].sha256 = multiplayer_server::career::hash(b"forged declaration");
    assert!(audit(&sparse, &inputs, &o).is_err());
}
#[test]
fn mismatched_make_false_claims_or_changed_archives_refuse_before_creating_output() {
    let t = tempfile::tempdir().unwrap();
    let archive = t.path().join("own.scs");
    let (c, i, mut o) = fixture(&archive);
    let output = t.path().join("absent.json");
    let original = o.clone();
    o.slots[0].definition_key =
        Some(resource_key("makes/truck/another/acc/cabin/!desc.def").unwrap());
    assert!(create(&c, &i, &o, &output).is_err());
    assert!(!output.exists());
    o = original.clone();
    o.engine_verified = true;
    assert!(create(&c, &i, &o, &output).is_err());
    assert!(!output.exists());
    std::fs::write(archive, b"changed own archive").unwrap();
    assert!(create(&c, &i, &original, &output).is_err());
    assert!(!output.exists());
}

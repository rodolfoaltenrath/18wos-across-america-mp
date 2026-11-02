use super::*;
use crate::world_content::{inventory, resource_key};
use multiplayer_server::{
    session_protocol::PROFILE,
    vehicle_appearance::{
        observation::{Observation, Source},
        resources::Instance as Observed,
    },
    world_content::Role,
};
use std::{
    fs::{self, File},
    io::Write,
};
use zip::{ZipWriter, write::SimpleFileOptions};
const VERIFIED_BYTES: u64 =
    b"Own fake geometry sentinel".len() as u64 + b"Own fake material sentinel".len() as u64;
fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}
fn fixture(path: &Path) -> (Manifest, Vec<Input>, ResourceObservation) {
    archive(
        path,
        &[
            ("makes/truck/own/cabin.psm", b"Own fake geometry sentinel"),
            ("makes/truck/own/red.shd", b"Own fake material sentinel"),
            ("wrong.def", b"Own unrelated sentinel"),
        ],
    );
    let i = vec![Input {
        role: Role::Map,
        path: path.into(),
    }];
    let c = inventory(PROFILE, &i).unwrap();
    let o = ResourceObservation {
        schema: 1,
        names: Observation {
            schema: 1,
            profile: PROFILE.into(),
            source: Source::ProfiledActorNameCandidates,
            make_definition_key: resource_key("makes/truck/own/description.def").unwrap(),
            slots: vec![],
            candidate_only: true,
            engine_verified: false,
            world_synchronized: false,
        },
        instances: vec![
            None,
            None,
            Some(Observed {
                model_key: resource_key("makes/truck/own/cabin.psm").unwrap(),
                material_keys: vec![
                    resource_key("makes/truck/own/red.shd").unwrap(),
                    resource_key("makes/truck/own/red.shd").unwrap(),
                    resource_key("missing.shd").unwrap(),
                ],
            }),
        ],
        candidate_only: true,
        complete_assembly: false,
        engine_verified: false,
        world_synchronized: false,
    };
    (c, i, o)
}
#[test]
fn verifies_candidate_bytes_preserves_sparse_order_and_duplicates_without_fallback_or_leaks() {
    let t = tempfile::tempdir().unwrap();
    let (c, i, o) = fixture(&t.path().join("own.scs"));
    let report = audit(&c, &i, &o).unwrap();
    assert!(report.instances[0].is_none() && report.instances[1].is_none());
    let main = report.instances[2].as_ref().unwrap();
    assert_eq!(main.materials.len(), 3);
    assert_eq!(main.materials[0].key, main.materials[1].key);
    assert!(main.materials[2].verified.is_none());
    assert_eq!(report.unique_resources_verified, 2);
    assert_eq!(report.verified_bytes, VERIFIED_BYTES);
    assert!(
        !report.complete_assembly
            && !report.resource_formats_verified
            && !report.mounted_content_verified
            && !report.engine_verified
            && !report.world_synchronized
    );
    assert_eq!(report.observation_sha256, o.fingerprint().unwrap());
    let json = serde_json::to_string(&report).unwrap();
    for raw in [
        "cabin.psm",
        "red.shd",
        "Own fake",
        t.path().to_str().unwrap(),
    ] {
        assert!(!json.contains(raw));
    }
    let output = t.path().join("report.json");
    let summary = create(&c, &i, &o, &output).unwrap();
    assert_eq!(
        (
            summary.instances,
            summary.absent_fields,
            summary.material_links,
            summary.missing_links
        ),
        (1, 2, 3, 1)
    );
    let before = fs::read(&output).unwrap();
    assert!(create(&c, &i, &o, &output).is_err());
    assert_eq!(fs::read(&output).unwrap(), before);
}
#[test]
fn changed_archives_forged_resource_hashes_wrong_types_and_false_claims_refuse_output() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("own.scs");
    let (c, i, o) = fixture(&path);
    let output = t.path().join("absent.json");
    let mut forged = c.clone();
    let key = &o.instances[2].as_ref().unwrap().model_key;
    forged.layers[0]
        .resources
        .iter_mut()
        .find(|r| &r.key == key)
        .unwrap()
        .sha256 = "a".repeat(64);
    assert!(create(&forged, &i, &o, &output).is_err() && !output.exists());
    let mut wrong = o.clone();
    wrong.instances[2].as_mut().unwrap().model_key = resource_key("wrong.def").unwrap();
    assert!(create(&c, &i, &wrong, &output).is_err() && !output.exists());
    let mut wrong = o.clone();
    wrong.instances[2].as_mut().unwrap().material_keys[0] = key.clone();
    assert!(create(&c, &i, &wrong, &output).is_err() && !output.exists());
    let mut wrong = o.clone();
    wrong.engine_verified = true;
    assert!(create(&c, &i, &wrong, &output).is_err() && !output.exists());
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(create(&c, &i, &o, &output).is_err() && !output.exists());
}
#[test]
fn file_and_total_budgets_refuse_before_a_report_is_published() {
    let t = tempfile::tempdir().unwrap();
    let (c, i, mut o) = fixture(&t.path().join("own.scs"));
    assert!(audit_inner(&c, &i, &o, VERIFIED_BYTES - 1).is_err());
    assert_eq!(
        audit_inner(&c, &i, &o, VERIFIED_BYTES)
            .unwrap()
            .unique_resources_verified,
        2
    );
    let path = t.path().join("large.scs");
    archive(&path, &[("large.shd", &vec![0; MAX_MATERIAL + 1])]);
    let i = vec![Input {
        role: Role::Map,
        path,
    }];
    let c = inventory(PROFILE, &i).unwrap();
    o.instances[2].as_mut().unwrap().model_key = resource_key("missing.psm").unwrap();
    o.instances[2].as_mut().unwrap().material_keys = vec![resource_key("large.shd").unwrap()];
    let output = t.path().join("absent.json");
    assert!(create(&c, &i, &o, &output).is_err() && !output.exists());
}
#[test]
fn declared_layer_winner_is_verified_and_reordered_bindings_do_not_change_it_silently() {
    let t = tempfile::tempdir().unwrap();
    let (_, mut i, o) = fixture(&t.path().join("own.scs"));
    let path = t.path().join("paint.scs");
    archive(&path, &[("makes/truck/own/red.shd", b"Own override")]);
    i.push(Input {
        role: Role::Mod,
        path,
    });
    let c = inventory(PROFILE, &i).unwrap();
    let report = audit(&c, &i, &o).unwrap();
    let main = report.instances[2].as_ref().unwrap();
    assert_eq!(main.model.verified.as_ref().unwrap().layer, 0);
    let material = main.materials[0].verified.as_ref().unwrap();
    assert_eq!(material.layer, 1);
    assert_eq!(
        material.sha256,
        multiplayer_server::career::hash(b"Own override")
    );
    i.reverse();
    assert!(audit(&c, &i, &o).is_err());
}

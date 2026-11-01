use super::*;
use crate::world_content::{Layer, Resource, Role};
pub fn fixture() -> (Manifest, Catalogue, Description) {
    let key = career::hash(b"own-model");
    let content = Manifest {
        schema: 1,
        profile: crate::session_protocol::PROFILE.into(),
        layers: vec![Layer {
            role: Role::Map,
            sha256: career::hash(b"own-map"),
            bytes: 200,
            resources: vec![Resource {
                key: key.clone(),
                sha256: career::hash(b"own-resource"),
                bytes: 12,
            }],
        }],
    };
    let paint = |id: &str| Paint {
        id: id.into(),
        materials: vec![Material {
            slot: 0,
            resource: key.clone(),
        }],
        dependencies: vec![],
    };
    let catalogue = Catalogue {
        schema: 1,
        content_sha256: content.fingerprint().unwrap(),
        models: vec![Model {
            id: "own-truck".into(),
            resource: key.clone(),
            dependencies: vec![],
            paints: vec![paint("blue"), paint("red")],
        }],
    };
    let d = Description {
        schema: 1,
        catalogue_sha256: catalogue.fingerprint(&content).unwrap(),
        world_epoch: 1,
        player: "a".repeat(32),
        vehicle: "b".repeat(32),
        generation: 1,
        revision: 1,
        model: "own-truck".into(),
        paint: "red".into(),
    };
    (content, catalogue, d)
}
#[test]
fn catalogue_is_pinned_to_content_and_only_known_resource_keys() {
    let (content, catalogue, d) = fixture();
    let check = check(&content, &catalogue, &d).unwrap();
    assert!(!check.engine_verified && !check.world_synchronized);
    let mut bad = catalogue.clone();
    bad.models[0].resource = career::hash(b"missing");
    assert!(bad.validate(&content).is_err());
    bad = catalogue.clone();
    bad.models[0].paints[0].materials[0].resource = "../paint".into();
    assert!(bad.validate(&content).is_err());
    bad = catalogue.clone();
    bad.content_sha256 = career::hash(b"other content");
    assert!(bad.validate(&content).is_err());
    bad = catalogue.clone();
    bad.models[0].paints.swap(0, 1);
    assert!(bad.validate(&content).is_err());
    bad = catalogue.clone();
    let repeated = bad.models[0].paints[0].materials[0].clone();
    bad.models[0].paints[0].materials.push(repeated);
    assert!(bad.validate(&content).is_err());
    let mut changed = content.clone();
    changed.layers[0].sha256 = career::hash(b"other archive");
    assert!(catalogue.validate(&changed).is_err());
}
#[test]
fn stale_and_conflicting_revisions_never_replace_a_newer_paint() {
    let (content, catalogue, mut d) = fixture();
    let mut b = Board::new(1, catalogue, &content).unwrap();
    assert_eq!(b.apply(d.clone()).unwrap(), Applied::Updated);
    assert_eq!(b.apply(d.clone()).unwrap(), Applied::Duplicate);
    let old = d.clone();
    d.revision = 2;
    d.paint = "blue".into();
    b.apply(d.clone()).unwrap();
    assert_eq!(b.apply(old).unwrap(), Applied::Stale);
    let mut conflict = d.clone();
    conflict.paint = "red".into();
    assert!(b.apply(conflict).is_err());
    assert_eq!(b.get(&d.player), Some(&d));
    d.generation = 2;
    d.revision = 1;
    b.apply(d.clone()).unwrap();
    let mut old = d.clone();
    old.generation = 1;
    old.revision = 99;
    assert_eq!(b.apply(old).unwrap(), Applied::Stale);
}
#[test]
fn wrong_world_unknown_model_or_invalid_update_keeps_previous_complete_record() {
    let (content, catalogue, d) = fixture();
    let mut b = Board::new(1, catalogue, &content).unwrap();
    b.apply(d.clone()).unwrap();
    for n in 0..6 {
        let mut bad = d.clone();
        bad.revision = 2;
        match n {
            0 => bad.world_epoch = 2,
            1 => bad.model = "unknown".into(),
            2 => bad.paint = "unknown".into(),
            3 => bad.vehicle = "not-an-id".into(),
            4 => bad.vehicle = "c".repeat(32),
            _ => bad.catalogue_sha256 = career::hash(b"another catalogue"),
        };
        assert!(b.apply(bad).is_err());
        assert_eq!(b.get(&d.player), Some(&d));
    }
}
#[test]
fn pose_waits_for_exact_appearance_and_old_leave_cannot_delete_new_generation() {
    let (content, catalogue, mut d) = fixture();
    let mut b = Board::new(1, catalogue, &content).unwrap();
    assert!(!b.pose_ready(1, &d.player, &d.vehicle, 1, 1));
    b.apply(d.clone()).unwrap();
    assert!(b.pose_ready(1, &d.player, &d.vehicle, 1, 1));
    assert!(!b.pose_ready(1, &d.player, &d.vehicle, 1, 2));
    assert!(!b.pose_ready(2, &d.player, &d.vehicle, 1, 1));
    d.generation = 2;
    d.vehicle = "d".repeat(32);
    b.apply(d.clone()).unwrap();
    assert!(!b.remove(&d.player, 1).unwrap());
    assert!(b.get(&d.player).is_some());
    assert!(b.remove(&d.player, 2).unwrap());
}
#[test]
fn two_players_cannot_claim_one_vehicle_and_capacity_is_bounded() {
    let (content, catalogue, d) = fixture();
    let mut b = Board::new(1, catalogue, &content).unwrap();
    b.apply(d.clone()).unwrap();
    let mut other = d.clone();
    other.player = "c".repeat(32);
    assert!(b.apply(other).is_err());
    for i in 1..MAX_VEHICLES {
        let mut n = d.clone();
        n.player = format!("{i:032x}");
        n.vehicle = format!("{:032x}", i + 1000);
        b.apply(n).unwrap();
    }
    let mut extra = d.clone();
    extra.player = "e".repeat(32);
    extra.vehicle = "f".repeat(32);
    assert!(b.apply(extra).is_err());
    let mut update = d.clone();
    update.revision = 2;
    assert_eq!(b.apply(update).unwrap(), Applied::Updated);
}
#[test]
fn bounded_decoders_reject_unknown_or_duplicate_fields_and_do_not_leak_identity() {
    let (content, catalogue, d) = fixture();
    let bytes = serde_json::to_vec(&d).unwrap();
    assert_eq!(Description::parse(&bytes, &catalogue).unwrap(), d);
    assert!(Description::parse(&vec![b' '; MAX_DESCRIPTION_JSON + 1], &catalogue).is_err());
    assert!(Catalogue::parse(&vec![b' '; MAX_CATALOGUE_JSON + 1], &content).is_err());
    let duplicate = String::from_utf8(bytes)
        .unwrap()
        .replacen("{", "{\"revision\":3,", 1);
    assert!(Description::parse(duplicate.as_bytes(), &catalogue).is_err());
    let mut json = serde_json::to_value(&catalogue).unwrap();
    json["engine_verified"] = true.into();
    assert!(Catalogue::parse(&serde_json::to_vec(&json).unwrap(), &content).is_err());
    let report = serde_json::to_string(&check(&content, &catalogue, &d).unwrap()).unwrap();
    assert!(!report.contains(&d.player) && !report.contains(&d.vehicle));
}
#[test]
fn removed_generation_stays_retired_across_snapshot_and_history_overflow_is_explicit() {
    let (content, catalogue, mut d) = fixture();
    let mut b = Board::new(1, catalogue.clone(), &content).unwrap();
    b.apply(d.clone()).unwrap();
    b.remove(&d.player, 1).unwrap();
    assert_eq!(b.apply(d.clone()).unwrap(), Applied::Stale);
    let (vehicles, retired) = b.snapshot();
    let mut restored =
        Board::from_records(1, catalogue.clone(), &content, &vehicles, &retired).unwrap();
    assert_eq!(restored.apply(d.clone()).unwrap(), Applied::Stale);
    assert!(Board::from_records(1, catalogue, &content, &[d.clone()], &retired).is_err());
    d.generation = 2;
    assert_eq!(restored.apply(d).unwrap(), Applied::Updated);
    for i in 1..MAX_RETIRED {
        b.remove(&format!("{i:032x}"), 1).unwrap();
    }
    assert!(b.remove(&"f".repeat(32), 1).is_err());
}

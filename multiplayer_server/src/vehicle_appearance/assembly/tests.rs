use super::*;
use crate::{
    career,
    vehicle_appearance::{Material, Paint},
    world_content::{Layer, Resource, Role},
};
fn fixture() -> (Manifest, Catalogue) {
    let key = career::hash(b"owned resource");
    let content = Manifest {
        schema: 1,
        profile: crate::session_protocol::PROFILE.into(),
        layers: vec![Layer {
            role: Role::Map,
            sha256: career::hash(b"owned archive"),
            bytes: 200,
            resources: vec![Resource {
                key: key.clone(),
                sha256: career::hash(b"owned content"),
                bytes: 13,
            }],
        }],
    };
    let part = |id: &str, parent: Option<&str>| Part {
        id: id.into(),
        parent: parent.map(String::from),
        resource: key.clone(),
        dependencies: vec![],
        placement: Placement::DefinitionOffset {
            value: [1.0, 2.0, 3.0],
        },
    };
    let catalogue = Catalogue {
        schema: 2,
        content_sha256: content.fingerprint().unwrap(),
        models: vec![Model {
            id: "own-model".into(),
            resource: key.clone(),
            dependencies: vec![],
            parts: vec![part("a-lamp", Some("z-cabin")), part("z-cabin", None)],
            paints: vec![Paint {
                id: "own-paint".into(),
                dependencies: vec![],
                materials: vec![
                    Material {
                        slot: 0,
                        resource: key.clone(),
                        part: Some("a-lamp".into()),
                    },
                    Material {
                        slot: 0,
                        resource: key.clone(),
                        part: None,
                    },
                    Material {
                        slot: 0,
                        resource: key,
                        part: Some("z-cabin".into()),
                    },
                ],
            }],
        }],
    };
    (content, catalogue)
}
#[test]
fn multipart_plan_orders_parents_before_children_and_separates_material_slots() {
    let (c, a) = fixture();
    let p = plan(&c, &a, "own-model", "own-paint").unwrap();
    assert_eq!(
        p.parts.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["root", "z-cabin", "a-lamp"]
    );
    assert_eq!(p.parts[2].parent.as_deref(), Some("z-cabin"));
    assert_eq!(p.assets.len(), 1);
    assert!(p.parts.iter().all(|p| p.materials.len() == 1));
    assert!(!p.all_placements_declared_local && !p.engine_verified && !p.world_synchronized);
    assert!(plan(&c, &a, "unknown", "own-paint").is_err());
    assert!(plan(&c, &a, "own-model", "unknown").is_err());
}
#[test]
fn cycles_unknown_parents_and_root_aliases_are_not_guessed() {
    let (c, a) = fixture();
    for variant in 0..4 {
        let mut bad = a.clone();
        match variant {
            0 => bad.models[0].parts[1].parent = Some("a-lamp".into()),
            1 => bad.models[0].parts[1].parent = Some("missing".into()),
            2 => bad.models[0].parts[0].id = "root".into(),
            _ => bad.models[0].parts[0].parent = Some("root".into()),
        };
        assert!(bad.validate(&c).is_err());
    }
}
#[test]
fn paint_targets_and_slot_collisions_require_known_parts() {
    let (c, a) = fixture();
    let mut bad = a.clone();
    bad.models[0].paints[0].materials[0].part = Some("unknown".into());
    assert!(bad.validate(&c).is_err());
    bad = a.clone();
    let duplicate = bad.models[0].paints[0].materials[0].clone();
    bad.models[0].paints[0].materials.insert(0, duplicate);
    assert!(bad.validate(&c).is_err());
    bad = a.clone();
    bad.schema = 1;
    assert!(bad.validate(&c).is_err());
    bad = a.clone();
    bad.models[0].parts[0].resource = career::hash(b"missing");
    assert!(bad.validate(&c).is_err());
}
#[test]
fn placements_must_be_finite_bounded_and_explicit_about_their_origin() {
    let (c, a) = fixture();
    let mut bad = a.clone();
    bad.models[0].parts[0].placement = Placement::DefinitionOffset {
        value: [f32::NAN, 0.0, 0.0],
    };
    assert!(bad.validate(&c).is_err());
    bad = a.clone();
    bad.models[0].parts[0].placement = Placement::LocalPose {
        position_cm: [1.0; 3],
        rotation: [0.0; 4],
    };
    assert!(bad.validate(&c).is_err());
    bad = a.clone();
    bad.models[0].parts[0].placement = Placement::Attachment {
        anchor: "../bad".into(),
    };
    assert!(bad.validate(&c).is_err());
    let mut local = a;
    for p in &mut local.models[0].parts {
        p.placement = Placement::LocalPose {
            position_cm: [1.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
    }
    let p = plan(&c, &local, "own-model", "own-paint").unwrap();
    assert!(p.all_placements_declared_local && !p.engine_verified);
}
#[test]
fn schema_one_serialization_is_preserved_and_new_fields_require_schema_two() {
    let (c, mut a) = fixture();
    a.schema = 1;
    a.models[0].parts.clear();
    a.models[0].paints[0].materials.retain(|m| m.part.is_none());
    let bytes = serde_json::to_vec(&a).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(!text.contains("\"parts\"") && !text.contains("\"part\""));
    assert_eq!(Catalogue::parse(&bytes, &c).unwrap(), a);
    let mut unknown = serde_json::to_value(&a).unwrap();
    unknown["engine_verified"] = true.into();
    assert!(Catalogue::parse(&serde_json::to_vec(&unknown).unwrap(), &c).is_err());
}
#[test]
fn owned_mutations_of_multibody_catalogue_are_bounded_and_never_panic() {
    let (c, a) = fixture();
    let seed = serde_json::to_vec(&a).unwrap();
    let mut rng = crate::fuzz::Rng::new(719);
    for _ in 0..6000 {
        if let Ok(decoded) = Catalogue::parse(&crate::fuzz::mutate(&mut rng, &seed), &c) {
            let model = &decoded.models[0];
            assert!(plan(&c, &decoded, &model.id, &model.paints[0].id).is_ok());
        }
    }
}

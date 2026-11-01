use super::*;
use crate::{
    career_world::{Clock, Company, CompanyView},
    vehicle_appearance::{Material, Model, Paint},
    world_content::{Layer, Resource, Role},
};
fn fixture() -> (
    career::Manifest,
    world_content::Manifest,
    Catalogue,
    Snapshot,
) {
    let resource = career::hash(b"own-resource");
    let content = world_content::Manifest {
        schema: 1,
        profile: crate::session_protocol::PROFILE.into(),
        layers: vec![Layer {
            role: Role::Map,
            sha256: career::hash(b"own archive"),
            bytes: 100,
            resources: vec![Resource {
                key: resource.clone(),
                sha256: resource.clone(),
                bytes: 12,
            }],
        }],
    };
    let catalogue = Catalogue {
        schema: 1,
        content_sha256: content.fingerprint().unwrap(),
        models: vec![Model {
            id: "own-model".into(),
            resource: resource.clone(),
            dependencies: vec![],
            paints: vec![Paint {
                id: "own-paint".into(),
                materials: vec![Material {
                    slot: 0,
                    resource,
                    part: None,
                }],
                dependencies: vec![],
            }],
            parts: vec![],
        }],
    };
    let manifest = career::Manifest::new(
        career::Goal::Unlimited,
        content.legacy().unwrap(),
        b"own save fixture",
    )
    .unwrap();
    let player = "a".repeat(32);
    let snapshot = Snapshot {
        schema: 1,
        scope: Scope::SidecarAppearanceOnly,
        world_epoch: 1,
        authority_generation: 1,
        career_manifest_sha256: career_entry::manifest_hash(&manifest).unwrap(),
        content_sha256: content.fingerprint().unwrap(),
        catalogue_sha256: catalogue.fingerprint(&content).unwrap(),
        appearance_cut: 10,
        sidecar: career_world::Snapshot {
            schema: 1,
            career: manifest.career.clone(),
            checkpoint_revision: 1,
            clock: Clock {
                world_ms: 100,
                paused: false,
                scale: 1,
            },
            clock_checkpoint_world_ms: 100,
            ranking: vec![CompanyView {
                player: player.clone(),
                company: Company {
                    name: "Own company".into(),
                    balance_minor: 0,
                    checkpoint: None,
                },
                ledger_goal_reached: false,
            }],
            contracts: Default::default(),
            engine_adapters: false,
            original_save_updated: false,
        },
        appearances: vec![Description {
            schema: 1,
            catalogue_sha256: catalogue.fingerprint(&content).unwrap(),
            world_epoch: 1,
            player,
            vehicle: "b".repeat(32),
            generation: 1,
            revision: 1,
            model: "own-model".into(),
            paint: "own-paint".into(),
        }],
        retired: vec![],
    };
    (manifest, content, catalogue, snapshot)
}
fn context<'a>(
    career: &'a career::Manifest,
    content: &'a world_content::Manifest,
    catalogue: &'a Catalogue,
) -> Context<'a> {
    Context {
        career,
        content,
        catalogue,
    }
}
fn delta(s: &Snapshot, sequence: u64) -> Delta {
    let mut d = s.appearances[0].clone();
    d.revision = sequence;
    Delta {
        schema: 1,
        world_epoch: 1,
        authority_generation: 1,
        sequence,
        change: Change::Appearance { description: d },
    }
}
#[test]
fn snapshot_cut_and_ordered_changes_prepare_once_without_claiming_engine_state() {
    let (m, c, a, s) = fixture();
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    assert!(r.prepare().is_err());
    r.begin(s.clone()).unwrap();
    assert_eq!(r.receive(delta(&s, 10)).unwrap(), Received::BeforeCut);
    assert_eq!(r.receive(delta(&s, 11)).unwrap(), Received::Queued);
    assert_eq!(r.receive(delta(&s, 11)).unwrap(), Received::Duplicate);
    let p = r.prepare().unwrap();
    assert_eq!(p.through_sequence, 11);
    assert_eq!(
        p.appearances
            .get(&s.appearances[0].player)
            .unwrap()
            .revision,
        11
    );
    assert!(!p.report().engine_verified && !p.report().world_synchronized);
    assert_eq!(p.report().staged_events, 1);
    assert!(r.prepare().is_err());
    assert!(r.begin(s.clone()).is_err());
    let mut next = s;
    next.appearance_cut = 11;
    next.appearances[0].revision = 11;
    assert!(r.begin(next).is_ok());
}
#[test]
fn wrong_binding_snapshot_preserves_previous_buffer_and_context() {
    let (m, c, a, s) = fixture();
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    r.begin(s.clone()).unwrap();
    r.receive(delta(&s, 11)).unwrap();
    for n in 0..8 {
        let mut bad = s.clone();
        match n {
            0 => bad.authority_generation = 2,
            1 => bad.world_epoch = 2,
            2 => bad.content_sha256 = career::hash(b"other"),
            3 => bad.career_manifest_sha256 = career::hash(b"other"),
            4 => bad.sidecar.engine_adapters = true,
            5 => bad.appearances[0].player = "c".repeat(32),
            6 => bad.appearances.push(bad.appearances[0].clone()),
            _ => bad.sidecar.clock.world_ms = 99,
        };
        assert!(r.begin(bad).is_err());
        assert_eq!(r.phase(), Phase::Buffering);
    }
    assert_eq!(r.prepare().unwrap().through_sequence, 11);
}
#[test]
fn gap_or_conflicting_duplicate_requires_new_snapshot_covering_observed_sequence() {
    let (m, c, a, s) = fixture();
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    r.begin(s.clone()).unwrap();
    assert!(r.receive(delta(&s, 12)).is_err());
    assert_eq!(r.phase(), Phase::NeedsSnapshot);
    assert!(r.prepare().is_err());
    assert!(r.begin(s.clone()).is_err());
    let mut next = s.clone();
    next.appearance_cut = 12;
    r.begin(next).unwrap();
    let d = delta(&s, 13);
    r.receive(d.clone()).unwrap();
    let mut conflict = d;
    if let Change::Appearance { description } = &mut conflict.change {
        description.generation = 2;
    }
    assert!(r.receive(conflict).is_err());
    assert_eq!(r.phase(), Phase::NeedsSnapshot);
}
#[test]
fn overflow_never_drops_changes_silently_and_wrong_context_does_not_renew_barrier() {
    let (m, c, a, s) = fixture();
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    r.begin(s.clone()).unwrap();
    let mut other = delta(&s, 11);
    other.authority_generation = 2;
    assert!(r.receive(other).is_err());
    for sequence in 11..11 + MAX_PENDING as u64 {
        r.receive(delta(&s, sequence)).unwrap();
    }
    assert!(r.receive(delta(&s, 11 + MAX_PENDING as u64)).is_err());
    assert_eq!(r.phase(), Phase::NeedsSnapshot);
    assert!(r.prepare().is_err());
    assert!(r.begin(s).is_err());
}
#[test]
fn invalid_staged_change_never_exposes_partial_board_or_clones_host_company() {
    let (m, c, a, s) = fixture();
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    r.begin(s.clone()).unwrap();
    r.receive(delta(&s, 11)).unwrap();
    let mut bad = delta(&s, 12);
    if let Change::Appearance { description } = &mut bad.change {
        description.player = "c".repeat(32);
        description.vehicle = "d".repeat(32);
    }
    r.receive(bad).unwrap();
    assert!(r.prepare().is_err());
    assert_eq!(r.phase(), Phase::NeedsSnapshot);
    let mut next = s;
    next.appearance_cut = 12;
    r.begin(next.clone()).unwrap();
    let p = r.prepare().unwrap();
    assert!(p.appearances.get(&"c".repeat(32)).is_none());
    assert_eq!(p.sidecar.ranking.len(), 1);
}
#[test]
fn stale_leave_cannot_remove_recreated_vehicle_and_decoders_are_bounded() {
    let (m, c, a, mut s) = fixture();
    s.appearances[0].generation = 2;
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    r.begin(s.clone()).unwrap();
    let remove = Delta {
        schema: 1,
        world_epoch: 1,
        authority_generation: 1,
        sequence: 11,
        change: Change::Remove {
            player: s.appearances[0].player.clone(),
            vehicle: s.appearances[0].vehicle.clone(),
            generation: 1,
        },
    };
    assert_eq!(
        Delta::parse(&serde_json::to_vec(&remove).unwrap()).unwrap(),
        remove
    );
    r.receive(remove).unwrap();
    assert!(
        r.prepare()
            .unwrap()
            .appearances
            .get(&s.appearances[0].player)
            .is_some()
    );
    assert!(Snapshot::parse(&vec![b' '; MAX_SNAPSHOT_JSON + 1]).is_err());
    assert!(Delta::parse(&vec![b' '; MAX_DELTA_JSON + 1]).is_err());
    let json = serde_json::to_string(&s)
        .unwrap()
        .replacen("{", "{\"schema\":2,", 1);
    assert!(Snapshot::parse(json.as_bytes()).is_err());
}
#[test]
fn mutated_owned_snapshot_and_delta_parsers_never_panic_and_accepted_data_revalidates() {
    let (m, c, a, s) = fixture();
    let mut rng = crate::fuzz::Rng::new(113);
    let snapshot = serde_json::to_vec(&s).unwrap();
    let event = serde_json::to_vec(&delta(&s, 11)).unwrap();
    for _ in 0..6000 {
        if let Ok(v) = Snapshot::parse(&crate::fuzz::mutate(&mut rng, &snapshot)) {
            let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
            let _ = r.begin(v);
        }
        if let Ok(v) = Delta::parse(&crate::fuzz::mutate(&mut rng, &event)) {
            let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
            r.begin(s.clone()).unwrap();
            let _ = r.receive(v);
        }
    }
}
#[test]
fn same_cut_or_checkpoint_cannot_change_already_observed_state() {
    let (m, c, a, s) = fixture();
    let mut r = Receiver::new(context(&m, &c, &a), 1, 1).unwrap();
    r.begin(s.clone()).unwrap();
    r.prepare().unwrap();
    let mut conflicting = s.clone();
    conflicting.appearances[0].revision += 1;
    assert!(r.begin(conflicting).is_err());
    conflicting = s.clone();
    conflicting.sidecar.ranking[0].company.name = "Other company".into();
    assert!(r.begin(conflicting).is_err());
    let mut advanced = s;
    advanced.sidecar.clock.world_ms += 100;
    assert!(r.begin(advanced).is_ok());
}
#[test]
fn nested_duplicate_contract_ids_are_not_silently_replaced() {
    let (_, _, _, s) = fixture();
    let contract = r#"{"origin":"own_a","destination":"own_b","reward_minor":1,"progress":{"kind":"offered"}}"#;
    let map = format!(
        "\"contracts\":{{\"{}\":{contract},\"{}\":{contract}}}",
        "c".repeat(32),
        "c".repeat(32)
    );
    let bytes = serde_json::to_string(&s)
        .unwrap()
        .replace("\"contracts\":{}", &map);
    assert!(Snapshot::parse(bytes.as_bytes()).is_err());
}

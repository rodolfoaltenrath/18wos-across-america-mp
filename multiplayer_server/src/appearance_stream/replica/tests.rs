use super::*;
use crate::{
    appearance_stream::{Authority, Disposition},
    world_join::{self as join, Change},
};

fn context<'a>(
    m: &'a career::Manifest,
    c: &'a crate::world_content::Manifest,
    a: &'a crate::vehicle_appearance::Catalogue,
) -> Context<'a> {
    Context {
        career: m,
        content: c,
        catalogue: a,
    }
}
fn snapshot_reply(pin: &Pin, s: Snapshot) -> Reply {
    Reply {
        schema: 1,
        pin: pin.clone(),
        payload: Payload::Snapshot {
            snapshot: Box::new(s),
        },
        engine_verified: false,
        world_synchronized: false,
    }
}
fn publish(host: &mut Authority, pin: &Pin, s: &Snapshot, revision: u64) {
    let mut d = s.appearances[0].clone();
    d.revision = revision;
    host.execute(
        &d.player.clone(),
        pin,
        &Operation::Publish {
            change: Change::Appearance { description: d },
        },
    )
    .unwrap();
}
#[test]
fn pages_resume_from_private_checkpoint_and_preserve_retirements() {
    let (m, c, a, s) = join::tests::fixture();
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    let who = &s.appearances[0].player;
    let mut replica = Replica::new(context(&m, &c, &a), &pin).unwrap();
    assert!(replica.checkpoint_bytes().is_err());
    replica
        .apply(&host.execute(who, &pin, &replica.next_pull()).unwrap())
        .unwrap();
    for revision in 2..=91 {
        publish(&mut host, &pin, &s, revision);
    }
    replica
        .apply(&host.execute(who, &pin, &replica.next_pull()).unwrap())
        .unwrap();
    assert_eq!(replica.report().through_sequence, Some(42));
    assert_eq!(replica.report().observed_head, Some(100));
    assert!(!replica.report().at_observed_head);
    let bytes = replica.checkpoint_bytes().unwrap();
    let mut resumed = Replica::from_bytes(context(&m, &c, &a), &pin, &bytes).unwrap();
    for expected in [74, 100] {
        resumed
            .apply(&host.execute(who, &pin, &resumed.next_pull()).unwrap())
            .unwrap();
        assert_eq!(resumed.report().through_sequence, Some(expected));
    }
    assert!(resumed.report().at_observed_head);
    let remove = Operation::Publish {
        change: Change::Remove {
            player: who.clone(),
            vehicle: s.appearances[0].vehicle.clone(),
            generation: 1,
        },
    };
    host.execute(who, &pin, &remove).unwrap();
    resumed
        .apply(&host.execute(who, &pin, &resumed.next_pull()).unwrap())
        .unwrap();
    let restored = Replica::from_bytes(
        context(&m, &c, &a),
        &pin,
        &resumed.checkpoint_bytes().unwrap(),
    )
    .unwrap();
    assert_eq!(restored.report().appearances, 0);
    assert_eq!(restored.report().retired, 1);
    let old = host
        .execute(
            who,
            &pin,
            &Operation::Publish {
                change: Change::Appearance {
                    description: s.appearances[0].clone(),
                },
            },
        )
        .unwrap();
    assert!(matches!(
        old.payload,
        Payload::Accepted {
            disposition: Disposition::Stale,
            ..
        }
    ));
    assert!(!restored.report().engine_verified && !restored.report().world_synchronized);
}
#[test]
fn replacement_checks_same_cut_checkpoint_clock_and_changed_context() {
    let (m, c, a, s) = join::tests::fixture();
    let pin = Pin::new(&context(&m, &c, &a), 1, 1).unwrap();
    let mut replica = Replica::new(context(&m, &c, &a), &pin).unwrap();
    replica.apply(&snapshot_reply(&pin, s.clone())).unwrap();
    let before = replica.checkpoint_bytes().unwrap();
    let mut bad = s.clone();
    bad.appearances[0].revision += 1;
    assert!(replica.apply(&snapshot_reply(&pin, bad)).is_err());
    let mut bad = s.clone();
    bad.sidecar.clock.world_ms -= 1;
    assert!(replica.apply(&snapshot_reply(&pin, bad)).is_err());
    let mut bad = s.clone();
    bad.sidecar.ranking[0].company.balance_minor += 1;
    assert!(replica.apply(&snapshot_reply(&pin, bad)).is_err());
    let mut bad = s.clone();
    bad.sidecar.checkpoint_revision = 0;
    assert!(replica.apply(&snapshot_reply(&pin, bad)).is_err());
    let mut changed = pin.clone();
    changed.authority_generation += 1;
    assert!(replica.apply(&snapshot_reply(&changed, s.clone())).is_err());
    assert!(Replica::from_bytes(context(&m, &c, &a), &changed, &before).is_err());
    assert_eq!(replica.checkpoint_bytes().unwrap(), before);
    replica.apply(&snapshot_reply(&pin, s)).unwrap();
    assert_eq!(replica.checkpoint_bytes().unwrap(), before);
}
#[test]
fn bad_pages_never_publish_a_partial_board_or_advance_cursor() {
    let (m, c, a, s) = join::tests::fixture();
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    let mut replica = Replica::new(context(&m, &c, &a), &pin).unwrap();
    replica.apply(&snapshot_reply(&pin, s.clone())).unwrap();
    publish(&mut host, &pin, &s, 2);
    publish(&mut host, &pin, &s, 3);
    let valid = host
        .execute(&s.appearances[0].player, &pin, &replica.next_pull())
        .unwrap();
    let before = replica.checkpoint_bytes().unwrap();
    let mut bad = valid.clone();
    if let Payload::Events { deltas, .. } = &mut bad.payload {
        deltas[1].change = Change::Remove {
            player: s.appearances[0].player.clone(),
            vehicle: "f".repeat(32),
            generation: 1,
        };
    }
    assert!(replica.apply(&bad).is_err());
    let mut gap = valid.clone();
    if let Payload::Events { deltas, .. } = &mut gap.payload {
        deltas.remove(0);
    }
    assert!(replica.apply(&gap).is_err());
    assert_eq!(replica.checkpoint_bytes().unwrap(), before);
    replica.apply(&valid).unwrap();
    let after = replica.checkpoint_bytes().unwrap();
    assert!(replica.apply(&valid).is_err()); // reply to an older request
    assert_eq!(replica.checkpoint_bytes().unwrap(), after);
}
#[test]
fn observed_head_survives_restart_and_expired_history_uses_covering_snapshot() {
    let (m, c, a, s) = join::tests::fixture();
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    let mut replica = Replica::new(context(&m, &c, &a), &pin).unwrap();
    replica.apply(&snapshot_reply(&pin, s.clone())).unwrap();
    for revision in 2..=91 {
        publish(&mut host, &pin, &s, revision);
    }
    replica
        .apply(
            &host
                .execute(&s.appearances[0].player, &pin, &replica.next_pull())
                .unwrap(),
        )
        .unwrap();
    let bytes = replica.checkpoint_bytes().unwrap();
    let mut resumed = Replica::from_bytes(context(&m, &c, &a), &pin, &bytes).unwrap();
    let mut behind = resumed.snapshot.clone().unwrap();
    behind.appearance_cut += 1;
    assert!(resumed.apply(&snapshot_reply(&pin, behind)).is_err());
    assert_eq!(resumed.checkpoint_bytes().unwrap(), bytes);
    for revision in 92..=250 {
        publish(&mut host, &pin, &s, revision);
    }
    let reply = host
        .execute(&s.appearances[0].player, &pin, &resumed.next_pull())
        .unwrap();
    assert!(matches!(reply.payload, Payload::Snapshot { .. }));
    resumed.apply(&reply).unwrap();
    assert_eq!(resumed.report().through_sequence, Some(259));
    assert!(resumed.report().at_observed_head);
}
#[test]
fn checkpoint_refuses_corruption_false_claims_unknown_fields_and_limits() {
    let (m, c, a, s) = join::tests::fixture();
    let pin = Pin::new(&context(&m, &c, &a), 1, 1).unwrap();
    let mut replica = Replica::new(context(&m, &c, &a), &pin).unwrap();
    replica.apply(&snapshot_reply(&pin, s)).unwrap();
    let original = replica.checkpoint_bytes().unwrap();
    for (field, value) in [
        ("schema", serde_json::json!(2)),
        ("observed_head", serde_json::json!(9)),
        ("engine_verified", serde_json::json!(true)),
        ("world_synchronized", serde_json::json!(true)),
        ("extra", serde_json::json!(null)),
        ("sha256", serde_json::json!("0".repeat(64))),
    ] {
        let mut v: serde_json::Value = serde_json::from_slice(&original).unwrap();
        v[field] = value;
        assert!(
            Replica::from_bytes(context(&m, &c, &a), &pin, &serde_json::to_vec(&v).unwrap())
                .is_err()
        );
    }
    assert!(
        Replica::from_bytes(
            context(&m, &c, &a),
            &pin,
            &vec![b' '; MAX_CHECKPOINT_JSON + 1]
        )
        .is_err()
    );
    assert!(
        Replica::from_bytes(context(&m, &c, &a), &pin, &original[..original.len() - 1]).is_err()
    );
}
#[test]
fn mutated_checkpoints_and_replies_do_not_panic_or_damage_staged_state() {
    let (m, c, a, s) = join::tests::fixture();
    let pin = Pin::new(&context(&m, &c, &a), 1, 1).unwrap();
    let reply = snapshot_reply(&pin, s);
    let mut replica = Replica::new(context(&m, &c, &a), &pin).unwrap();
    replica.apply(&reply).unwrap();
    let checkpoint = replica.checkpoint_bytes().unwrap();
    let encoded_reply = serde_json::to_vec(&reply).unwrap();
    let mut rng = crate::fuzz::Rng::new(9031);
    for _ in 0..3000 {
        let mutated = crate::fuzz::mutate(&mut rng, &checkpoint);
        let _ = Replica::from_bytes(context(&m, &c, &a), &pin, &mutated);
        let mutated = crate::fuzz::mutate(&mut rng, &encoded_reply);
        if let Ok(reply) = serde_json::from_slice::<Reply>(&mutated) {
            let before = replica.checkpoint_bytes().unwrap();
            if replica.apply(&reply).is_err() {
                assert_eq!(replica.checkpoint_bytes().unwrap(), before);
            }
        }
    }
}

#[test]
fn checkpoint_publication_is_private_new_only_and_ignores_interrupted_temporaries() {
    let t = crate::career_store::tests::Temp::new();
    let (m, c, a, s) = join::tests::fixture();
    let ctx = context(&m, &c, &a);
    let pin = Pin::new(&ctx, 1, 1).unwrap();
    let mut replica = Replica::new(ctx, &pin).unwrap();
    replica.apply(&snapshot_reply(&pin, s)).unwrap();
    let path = t.0.join("previous.json");
    replica.save_new(&path).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(replica.save_new(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let pending = t.0.join(".appearance-owned-interruption.tmp");
    fs::write(&pending, &before[..before.len() / 2]).unwrap();
    assert!(Replica::load(ctx, &pin, &pending).is_err());
    let loaded = Replica::load(ctx, &pin, &path).unwrap();
    assert_eq!(loaded.checkpoint_bytes().unwrap(), before);
    let next = t.0.join("next.json");
    loaded.save_new(&next).unwrap();
    assert_eq!(fs::read(&next).unwrap(), before);
    assert!(pending.is_file()); // never sweep arbitrary pending files
    assert!(replica.save_new(&t.0.join("missing/next.json")).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::{fs::PermissionsExt, fs::symlink};
        assert_eq!(
            fs::metadata(&next).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = t.0.join("link.json");
        symlink(&path, &link).unwrap();
        assert!(Replica::load(ctx, &pin, &link).is_err());
        assert!(replica.save_new(&link).is_err());
    }
}

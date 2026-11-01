use super::*;
fn context<'a>(
    m: &'a crate::career::Manifest,
    c: &'a crate::world_content::Manifest,
    a: &'a va::Catalogue,
) -> Context<'a> {
    Context {
        career: m,
        content: c,
        catalogue: a,
    }
}
#[test]
fn duplicate_retry_is_idempotent_and_reconnect_can_fetch_the_current_snapshot() {
    let (m, c, a, s) = join::tests::fixture();
    let who = s.appearances[0].player.clone();
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    let mut description = s.appearances[0].clone();
    description.revision += 1;
    let op = Operation::Publish {
        change: Change::Appearance { description },
    };
    let first = host.execute(&who, &pin, &op).unwrap();
    assert!(matches!(
        first.payload,
        Payload::Accepted {
            sequence: 11,
            disposition: Disposition::Published
        }
    ));
    let again = host.execute(&who, &pin, &op).unwrap();
    assert!(matches!(
        again.payload,
        Payload::Accepted {
            sequence: 11,
            disposition: Disposition::Duplicate
        }
    ));
    let fetch = Operation::Pull { after: None };
    let reply = host.execute(&who, &pin, &fetch).unwrap();
    reply.validate(context(&m, &c, &a), &pin, &fetch).unwrap();
    let Payload::Snapshot { snapshot } = reply.payload else {
        panic!()
    };
    assert_eq!(snapshot.appearance_cut, 11);
    assert_eq!(snapshot.appearances[0].revision, 2);
    assert_eq!(snapshot.sidecar.clock.world_ms, s.sidecar.clock.world_ms);
    assert!(!reply.engine_verified && !reply.world_synchronized);
}
#[test]
fn conflicting_publications_and_identity_context_mismatches_do_not_consume_sequences() {
    let (m, c, a, s) = join::tests::fixture();
    let who = s.appearances[0].player.clone();
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    let mut d = s.appearances[0].clone();
    d.paint = "unknown".into();
    assert!(
        host.execute(
            &who,
            &pin,
            &Operation::Publish {
                change: Change::Appearance { description: d }
            }
        )
        .is_err()
    );
    assert!(
        host.execute(&"f".repeat(32), &pin, &Operation::Pull { after: None })
            .is_err()
    );
    let mut wrong = pin.clone();
    wrong.authority_generation += 1;
    assert!(
        host.execute(&who, &wrong, &Operation::Pull { after: None })
            .is_err()
    );
    let remove = Operation::Publish {
        change: Change::Remove {
            player: who.clone(),
            vehicle: "e".repeat(32),
            generation: 1,
        },
    };
    assert!(host.execute(&who, &pin, &remove).is_err());
    assert_eq!(host.seed.appearance_cut, 10);
    assert!(host.history.is_empty());
    assert_eq!(host.seed.appearances, s.appearances);
}
#[test]
fn retained_history_pages_are_contiguous_and_lagging_readers_get_a_snapshot() {
    let (m, c, a, s) = join::tests::fixture();
    let who = s.appearances[0].player.clone();
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    for revision in 2..=150 {
        let mut d = s.appearances[0].clone();
        d.revision = revision;
        host.execute(
            &who,
            &pin,
            &Operation::Publish {
                change: Change::Appearance { description: d },
            },
        )
        .unwrap();
    }
    assert_eq!(host.history.len(), HISTORY);
    assert!(matches!(
        host.execute(&who, &pin, &Operation::Pull { after: Some(10) })
            .unwrap()
            .payload,
        Payload::Snapshot { .. }
    ));
    let mut cursor = host.history.front().unwrap().sequence - 1;
    while cursor < host.seed.appearance_cut {
        let op = Operation::Pull {
            after: Some(cursor),
        };
        let reply = host.execute(&who, &pin, &op).unwrap();
        reply.validate(context(&m, &c, &a), &pin, &op).unwrap();
        let Payload::Events {
            after,
            through,
            head,
            deltas,
        } = reply.payload
        else {
            panic!()
        };
        assert_eq!(after, cursor);
        assert!(through > cursor && through <= head && deltas.len() <= BATCH);
        cursor = through;
    }
    assert!(
        host.execute(
            &who,
            &pin,
            &Operation::Pull {
                after: Some(cursor + 1)
            }
        )
        .is_err()
    );
}
#[test]
fn retirement_cannot_be_undone_by_an_old_generation_after_retry() {
    let (m, c, a, s) = join::tests::fixture();
    let d = s.appearances[0].clone();
    let who = d.player.clone();
    let mut host = Authority::new(context(&m, &c, &a), s).unwrap();
    let pin = host.pin().clone();
    let op = Operation::Publish {
        change: Change::Remove {
            player: who.clone(),
            vehicle: d.vehicle.clone(),
            generation: 1,
        },
    };
    assert!(matches!(
        host.execute(&who, &pin, &op).unwrap().payload,
        Payload::Accepted {
            disposition: Disposition::Published,
            ..
        }
    ));
    host.execute(&who, &pin, &op).unwrap();
    assert!(matches!(
        host.execute(
            &who,
            &pin,
            &Operation::Publish {
                change: Change::Appearance { description: d }
            }
        )
        .unwrap()
        .payload,
        Payload::Accepted {
            sequence: 11,
            disposition: Disposition::Stale
        }
    ));
    assert!(host.seed.appearances.is_empty());
}
#[test]
fn exhausted_sequence_and_oversized_seed_leave_authority_unchanged() {
    let (m, c, a, mut s) = join::tests::fixture();
    s.appearance_cut = u64::MAX;
    let mut d = s.appearances[0].clone();
    let who = d.player.clone();
    d.revision += 1;
    let mut host = Authority::new(context(&m, &c, &a), s.clone()).unwrap();
    let pin = host.pin().clone();
    assert!(
        host.execute(
            &who,
            &pin,
            &Operation::Publish {
                change: Change::Appearance { description: d }
            }
        )
        .is_err()
    );
    assert_eq!(host.seed.appearances, s.appearances);
    assert!(host.history.is_empty());
    s.sidecar.ranking[0].company.name = "x".repeat(MAX_SEED_JSON);
    assert!(Authority::new(context(&m, &c, &a), s).is_err());
}
#[test]
fn altered_reply_context_order_and_claims_are_refused() {
    let (m, c, a, s) = join::tests::fixture();
    let who = s.appearances[0].player.clone();
    let mut host = Authority::new(context(&m, &c, &a), s).unwrap();
    let pin = host.pin().clone();
    let op = Operation::Pull { after: None };
    let mut reply = host.execute(&who, &pin, &op).unwrap();
    reply.engine_verified = true;
    assert!(reply.validate(context(&m, &c, &a), &pin, &op).is_err());
    reply.engine_verified = false;
    let mut wrong = pin.clone();
    wrong.content_sha256 = "e".repeat(64);
    reply.pin = wrong.clone();
    assert!(reply.validate(context(&m, &c, &a), &wrong, &op).is_err());
    let op = Operation::Pull { after: Some(10) };
    reply.pin = pin.clone();
    reply.payload = Payload::Events {
        after: 10,
        through: 11,
        head: 11,
        deltas: vec![],
    };
    assert!(reply.validate(context(&m, &c, &a), &pin, &op).is_err());
}

use super::*;
use crate::{
    career_store::tests::Temp,
    world_join::{self as join, Change, Context},
};
fn context<'a>(
    m: &'a Manifest,
    c: &'a crate::world_content::Manifest,
    a: &'a crate::vehicle_appearance::Catalogue,
) -> Context<'a> {
    Context {
        career: m,
        content: c,
        catalogue: a,
    }
}
fn receipt(m: &Manifest) -> Receipt {
    Receipt {
        schema: 1,
        career: m.career.clone(),
        manifest_sha256: crate::career_entry::manifest_hash(m).unwrap(),
        save_sha256: m.bootstrap.sha256.clone(),
        bytes: m.bootstrap.bytes,
        bootstrap_verified: true,
        engine_loaded: false,
        world_synchronized: false,
    }
}
#[tokio::test]
async fn real_career_room_exchanges_appearances_with_shared_identity_bound_tickets() {
    let dir = Temp::new();
    let (m, c, a, mut seed) = join::tests::fixture();
    let first = seed.appearances[0].clone();
    let mut second = first.clone();
    second.player = "b".repeat(32);
    second.vehicle = "d".repeat(32);
    let mut company = seed.sidecar.ranking[0].clone();
    company.player = second.player.clone();
    seed.sidecar.ranking.push(company);
    seed.appearances.push(second.clone());
    store::write_new(&dir.0.join("bootstrap.sav"), b"own save fixture").unwrap();
    store::write_new(&dir.0.join("manifest.json"), &store::json(&m).unwrap()).unwrap();
    let invite = Invite::new(&m).unwrap();
    store::write_new(&dir.0.join("invite.json"), &store::json(&invite).unwrap()).unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = tcp.local_addr().unwrap();
    let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let relay = udp.local_addr().unwrap();
    let authority = stream::Authority::new(context(&m, &c, &a), seed).unwrap();
    let pin = authority.pin().clone();
    let data = dir.0.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        serve_room_with_appearance(
            tcp,
            udp,
            &data,
            crate::session_hub::Settings {
                room: "own".into(),
                profile: session_protocol::PROFILE.into(),
                max_clients: 2,
                timeout_ms: 2000,
                echo: false,
            },
            relay,
            Some(authority),
            async {
                let _ = stopped.await;
            },
        )
        .await
    });
    let mut launches = Vec::new();
    for (index, who) in [&first.player, &second.player].into_iter().enumerate() {
        let import = dir.0.join(format!("guest{index}"));
        fetch(endpoint, &invite, m.compatibility.clone(), &import)
            .await
            .unwrap();
        launches.push(
            prepare_launch(endpoint, &invite, m.compatibility.clone(), &import, who)
                .await
                .unwrap(),
        );
    }
    let initial = exchange(
        endpoint,
        &launches[0],
        context(&m, &c, &a),
        &pin,
        stream::Operation::Pull { after: None },
    )
    .await
    .unwrap();
    assert!(matches!(initial.payload, stream::Payload::Snapshot { .. }));
    let mut next = first.clone();
    next.revision += 1;
    let publish = stream::Operation::Publish {
        change: Change::Appearance { description: next },
    };
    let accepted = exchange(
        endpoint,
        &launches[0],
        context(&m, &c, &a),
        &pin,
        publish.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        accepted.payload,
        stream::Payload::Accepted {
            sequence: 11,
            disposition: stream::Disposition::Published
        }
    ));
    let retry = exchange(
        endpoint,
        &launches[0],
        context(&m, &c, &a),
        &pin,
        publish.clone(),
    )
    .await
    .unwrap();
    assert!(matches!(
        retry.payload,
        stream::Payload::Accepted {
            sequence: 11,
            disposition: stream::Disposition::Duplicate
        }
    ));
    assert!(
        exchange(endpoint, &launches[1], context(&m, &c, &a), &pin, publish)
            .await
            .is_err()
    );
    second.revision += 1;
    exchange(
        endpoint,
        &launches[1],
        context(&m, &c, &a),
        &pin,
        stream::Operation::Publish {
            change: Change::Appearance {
                description: second,
            },
        },
    )
    .await
    .unwrap();
    let page = exchange(
        endpoint,
        &launches[0],
        context(&m, &c, &a),
        &pin,
        stream::Operation::Pull { after: Some(10) },
    )
    .await
    .unwrap();
    let stream::Payload::Events {
        through, deltas, ..
    } = page.payload
    else {
        panic!()
    };
    assert_eq!(through, 12);
    assert_eq!(deltas.len(), 2);
    let mut wrong = launches[0].clone();
    wrong.proof.ticket = "0".repeat(32);
    assert!(
        exchange(
            endpoint,
            &wrong,
            context(&m, &c, &a),
            &pin,
            stream::Operation::Pull { after: None }
        )
        .await
        .is_err()
    );
    stop.send(()).unwrap();
    let report = server.await.unwrap().unwrap();
    let summary = report.control.appearance.unwrap();
    assert_eq!(
        (
            summary.publications,
            summary.duplicates,
            summary.refused,
            summary.head
        ),
        (2, 1, 1, 12)
    );
    assert_eq!((summary.snapshot_replies, summary.event_pages), (1, 1));
    assert!(
        summary.seed_sidecar_frozen
            && !summary.engine_verified
            && !summary.world_synchronized
            && !report.game_tested
    );
    assert_eq!(store::load(&dir.0).unwrap().1, b"own save fixture");
    // Restart WITHOUT the opt-in service: old tickets do not enable this RPC.
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = tcp.local_addr().unwrap();
    let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let relay = udp.local_addr().unwrap();
    let data = dir.0.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        serve_room(
            tcp,
            udp,
            &data,
            crate::session_hub::Settings {
                room: "own".into(),
                profile: session_protocol::PROFILE.into(),
                max_clients: 2,
                timeout_ms: 2000,
                echo: false,
            },
            relay,
            async {
                let _ = stopped.await;
            },
        )
        .await
    });
    assert!(
        exchange(
            endpoint,
            &launches[0],
            context(&m, &c, &a),
            &pin,
            stream::Operation::Pull { after: None }
        )
        .await
        .is_err()
    );
    world_status(endpoint, &invite, m.compatibility.clone())
        .await
        .unwrap();
    stop.send(()).unwrap();
    assert!(server.await.unwrap().unwrap().control.appearance.is_none());
}
#[test]
fn budgets_reset_monotonically_and_restarted_registry_refuses_old_tickets() {
    let (m, c, a, s) = join::tests::fixture();
    let who = s.appearances[0].player.clone();
    let registry = Registry::new(m.clone(), "own".into(), "127.0.0.1:1".parse().unwrap()).unwrap();
    let launch = registry.issue(&who, &receipt(&m)).unwrap();
    let authority = stream::Authority::new(context(&m, &c, &a), s).unwrap();
    let pin = authority.pin().clone();
    let service = Service::new(authority);
    let request = || Request {
        identity: who.clone(),
        room: "own".into(),
        proof: launch.proof.clone(),
        pin: pin.clone(),
        operation: stream::Operation::Pull { after: None },
    };
    for _ in 0..8 {
        service
            .execute(&registry, &m.compatibility.profile, request())
            .unwrap();
    }
    assert!(
        service
            .execute(&registry, &m.compatibility.profile, request())
            .unwrap_err()
            .contains("budget")
    );
    {
        let mut state = service.0.lock().unwrap();
        state.start -= Duration::from_secs(1);
    }
    service
        .execute(&registry, &m.compatibility.profile, request())
        .unwrap();
    let restarted = Registry::new(m.clone(), "own".into(), "127.0.0.1:1".parse().unwrap()).unwrap();
    assert!(
        service
            .execute(&restarted, &m.compatibility.profile, request())
            .unwrap_err()
            .contains("ticket")
    );
    let summary = service.summary().unwrap();
    assert_eq!(
        (summary.requests, summary.refused, summary.snapshot_replies),
        (10, 1, 9)
    );
}
#[tokio::test]
async fn unexpected_trailing_and_altered_replies_never_return_a_verified_result() {
    let (m, c, a, s) = join::tests::fixture();
    let who = s.appearances[0].player.clone();
    let registry = Registry::new(m.clone(), "own".into(), "127.0.0.1:1".parse().unwrap()).unwrap();
    let launch = registry.issue(&who, &receipt(&m)).unwrap();
    let mut authority = stream::Authority::new(context(&m, &c, &a), s).unwrap();
    let pin = authority.pin().clone();
    let op = stream::Operation::Pull { after: None };
    let reply = authority.execute(&who, &pin, &op).unwrap();
    for variant in 0..3 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let mut reply = reply.clone();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _: super::super::Request = receive(&mut socket).await.unwrap();
            if variant == 0 {
                reply.world_synchronized = true;
            }
            if variant == 1 {
                send(
                    &mut socket,
                    &Response::Refused {
                        reason: "own refusal".into(),
                    },
                )
                .await
                .unwrap();
            } else {
                send(
                    &mut socket,
                    &Response::Appearance {
                        reply: Box::new(reply),
                    },
                )
                .await
                .unwrap();
            }
            if variant == 2 {
                socket.write_all(b"x").await.unwrap();
            }
        });
        assert!(
            exchange(endpoint, &launch, context(&m, &c, &a), &pin, op.clone())
                .await
                .is_err()
        );
        task.await.unwrap();
    }
}
#[test]
fn shared_budget_refuses_a_ninth_peer_after_eight_peers_exhaust_the_window() {
    let (m, c, a, mut seed) = join::tests::fixture();
    let last = seed.appearances[0].player.clone();
    for id in 1..=8 {
        let mut company = seed.sidecar.ranking[0].clone();
        company.player = format!("{id:032x}");
        seed.sidecar.ranking.push(company);
    }
    seed.sidecar.ranking.sort_by(|a, b| a.player.cmp(&b.player));
    let registry = Registry::new(m.clone(), "own".into(), "127.0.0.1:1".parse().unwrap()).unwrap();
    let authority = stream::Authority::new(context(&m, &c, &a), seed).unwrap();
    let pin = authority.pin().clone();
    let service = Service::new(authority);
    for id in 1..=8 {
        let identity = format!("{id:032x}");
        let launch = registry.issue(&identity, &receipt(&m)).unwrap();
        for _ in 0..8 {
            service
                .execute(
                    &registry,
                    &m.compatibility.profile,
                    Request {
                        identity: identity.clone(),
                        room: "own".into(),
                        proof: launch.proof.clone(),
                        pin: pin.clone(),
                        operation: stream::Operation::Pull { after: None },
                    },
                )
                .unwrap();
        }
    }
    let launch = registry.issue(&last, &receipt(&m)).unwrap();
    assert!(
        service
            .execute(
                &registry,
                &m.compatibility.profile,
                Request {
                    identity: last,
                    room: "own".into(),
                    proof: launch.proof,
                    pin,
                    operation: stream::Operation::Pull { after: None }
                }
            )
            .unwrap_err()
            .contains("budget")
    );
    let summary = service.summary().unwrap();
    assert_eq!(
        (summary.requests, summary.refused, summary.snapshot_replies),
        (65, 1, 64)
    );
    let mut value = 999_999_999;
    let mut truncated = false;
    inc(&mut value, &mut truncated);
    assert_eq!(value, 999_999_999);
    assert!(truncated);
}

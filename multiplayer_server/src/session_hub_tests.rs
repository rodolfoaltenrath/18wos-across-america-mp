use super::session_hub::*;
use crate::{
    protocol::Position,
    session_protocol::{
        self as w, ClientFrame, ClientMessage as C, Credentials, ServerMessage as S,
    },
};
fn address(port: u16) -> std::net::SocketAddr {
    ([127, 0, 0, 1], port).into()
}
fn hub(max_clients: usize, echo: bool) -> Hub {
    Hub::new(
        Settings {
            room: "lan".into(),
            profile: w::PROFILE.into(),
            max_clients,
            timeout_ms: 2000,
            echo,
        },
        11,
    )
    .unwrap()
}
fn join(identity: u32, request: u64, resume: Option<Credentials>) -> ClientFrame {
    ClientFrame::new(C::Join {
        request,
        room: "lan".into(),
        identity: format!("{identity:032x}"),
        profile: w::PROFILE.into(),
        resume,
    })
}
fn ingest(hub: &mut Hub, port: u16, frame: &ClientFrame, now: u64) -> Vec<Datagram> {
    hub.ingest(
        address(port),
        &w::encode_client(frame).unwrap(),
        now,
        || Ok("a".repeat(32)),
    )
    .unwrap()
}
fn credentials(output: &[Datagram]) -> (u32, Credentials) {
    let S::Accepted {
        player,
        credentials,
        ..
    } = &output[0].1.message
    else {
        panic!("not accepted: {output:?}")
    };
    (*player, credentials.clone())
}
fn pose(credentials: Credentials, sequence: u64, x: f32) -> ClientFrame {
    ClientFrame::new(C::Pose {
        credentials,
        sequence,
        client_ms: 0,
        position: Position { x, y: 0.0, z: 0.0 },
        rotation: None,
    })
}
#[test]
fn acceptance_is_idempotent_and_bad_version_room_profile_or_capacity_do_not_allocate() {
    let mut h = hub(1, false);
    for failure in 0..3 {
        let mut bad = join(1, 1, None);
        match failure {
            0 => bad.version = 255,
            1 => {
                if let C::Join { room, .. } = &mut bad.message {
                    *room = "other".into();
                }
            }
            _ => {
                if let C::Join { profile, .. } = &mut bad.message {
                    *profile = "other".into();
                }
            }
        }
        assert!(matches!(
            ingest(&mut h, 1, &bad, 0)[0].1.message,
            S::Rejected { .. }
        ));
        assert_eq!((h.clients(), h.members()), (0, 0));
    }
    let first = credentials(&ingest(&mut h, 1, &join(1, 1, None), 0));
    let repeated = credentials(&ingest(&mut h, 1, &join(1, 1, None), 1));
    assert_eq!(first, repeated);
    assert_eq!(h.metrics.accepted, 1);
    assert!(matches!(
        ingest(&mut h, 2, &join(2, 2, None), 1)[0].1.message,
        S::Rejected {
            reason: w::Refusal::Full,
            ..
        }
    ));
    assert_eq!((h.clients(), h.members()), (1, 1));
}
#[test]
fn resume_rotates_generation_preserves_player_and_revokes_old_credentials_without_kicking_others() {
    let mut h = hub(2, true);
    let (player, old) = credentials(&ingest(&mut h, 1, &join(1, 1, None), 0));
    ingest(&mut h, 1, &pose(old.clone(), 99, 99.0), 1);
    assert!(matches!(
        ingest(&mut h, 2, &join(1, 2, None), 2)[0].1.message,
        S::Rejected {
            reason: w::Refusal::IdentityInUse,
            ..
        }
    ));
    let (same, new) = credentials(&ingest(&mut h, 2, &join(1, 2, Some(old.clone())), 3));
    assert_eq!(same, player);
    assert_ne!(new.connection, old.connection);
    ingest(&mut h, 1, &pose(old, 100, 999.0), 4);
    ingest(&mut h, 2, &pose(new.clone(), 0, 3.0), 5);
    let poses: Vec<_> = h
        .tick(6)
        .into_iter()
        .filter_map(|(_, f)| {
            if let S::Snapshot { poses, .. } = f.message {
                Some(poses)
            } else {
                None
            }
        })
        .flatten()
        .collect();
    assert_eq!(poses.len(), 1);
    assert_eq!(poses[0].sequence, 0);
    assert_eq!(poses[0].position.x, 3.0);
    assert_eq!(poses[0].generation, new.connection);
    assert_eq!(h.metrics.unauthenticated, 1);
}
#[test]
fn heartbeat_without_poses_keeps_connection_and_expiry_then_rejoin_resets_sequence() {
    let mut h = hub(1, false);
    let (player, old) = credentials(&ingest(&mut h, 1, &join(1, 1, None), 0));
    for (sequence, now) in [(0, 1500), (1, 3000), (2, 4500)] {
        ingest(
            &mut h,
            1,
            &ClientFrame::new(C::Ping {
                credentials: old.clone(),
                sequence,
                client_ms: now,
            }),
            now,
        );
        h.tick(now);
        assert_eq!(h.clients(), 1);
    }
    assert_eq!(h.metrics.poses, 0);
    h.tick(6500);
    assert_eq!(h.clients(), 0);
    let (same, new) = credentials(&ingest(&mut h, 3, &join(1, 3, Some(old)), 6501));
    assert_eq!(same, player);
    assert_eq!(h.metrics.expired, 1);
    ingest(&mut h, 3, &pose(new, 0, 1.0), 6502);
    assert_eq!(h.metrics.poses, 1);
}
#[test]
fn duplicate_leave_is_acknowledged_and_old_leave_cannot_remove_new_connection() {
    let mut h = hub(1, false);
    let (_, old) = credentials(&ingest(&mut h, 1, &join(1, 1, None), 0));
    let bye = ClientFrame::new(C::Leave {
        credentials: old.clone(),
    });
    assert!(matches!(
        ingest(&mut h, 1, &bye, 1)[0].1.message,
        S::Left { .. }
    ));
    assert!(matches!(
        ingest(&mut h, 1, &bye, 2)[0].1.message,
        S::Left { .. }
    ));
    let (_, new) = credentials(&ingest(&mut h, 1, &join(1, 2, None), 3));
    assert_ne!(old.connection, new.connection);
    ingest(&mut h, 1, &bye, 4);
    assert_eq!(h.clients(), 1);
    assert_eq!(h.metrics.left, 1);
}
#[test]
fn raw_invalid_flood_is_bounded_before_parsing_and_cannot_keep_a_peer_alive() {
    let mut h = hub(1, false);
    let (_, c) = credentials(&ingest(&mut h, 1, &join(1, 1, None), 0));
    for _ in 0..1000 {
        h.ingest(address(1), b"bad", 1, || panic!("no entropy on junk"))
            .unwrap();
    }
    assert!(h.metrics.rate_limited > 0);
    assert!(h.metrics.invalid <= 240);
    let ping = ClientFrame::new(C::Ping {
        credentials: c,
        sequence: 1,
        client_ms: 1000,
    });
    ingest(&mut h, 1, &ping, 1000);
    assert_eq!(h.metrics.pings, 1);
    for i in 0..3000 {
        h.ingest(address((i % 1000 + 2) as u16), b"bad", 1001, || {
            panic!("no entropy on junk")
        })
        .unwrap();
    }
    assert_eq!(h.clients(), 1);
    assert_eq!(h.members(), 1);
    h.tick(3000);
    assert_eq!(h.clients(), 0);
}
#[test]
fn membership_cache_and_snapshot_batches_stay_bounded() {
    let mut h = hub(64, true);
    for i in 1..=64 {
        let (_, c) = credentials(&ingest(
            &mut h,
            i,
            &join(u32::from(i), u64::from(i), None),
            0,
        ));
        ingest(&mut h, i, &pose(c, 0, f32::from(i)), 0);
    }
    let frames = h.tick(1);
    assert!(frames.len() <= 64 * 17);
    for (_, frame) in frames {
        assert!(w::encode_server(&frame).unwrap().len() <= crate::protocol::MAX_PACKET);
    }
    h.tick(2000);
    for i in 65..=160 {
        let (_, c) = credentials(&ingest(
            &mut h,
            1,
            &join(i, u64::from(i), None),
            u64::from(i) * 3000,
        ));
        ingest(
            &mut h,
            1,
            &ClientFrame::new(C::Leave { credentials: c }),
            u64::from(i) * 3000 + 1,
        );
        assert!(h.members() <= 64);
    }
}
#[test]
fn authenticated_resume_cannot_reset_the_endpoint_rate_window() {
    let mut h = hub(1, false);
    let (_, mut c) = credentials(&ingest(&mut h, 1, &join(1, 1, None), 0));
    for request in 2..=1000 {
        let output = ingest(&mut h, 1, &join(1, request, Some(c.clone())), 1);
        if !output.is_empty() {
            c = credentials(&output).1;
        }
    }
    assert!(h.metrics.accepted <= 240);
    assert!(h.metrics.rate_limited > 0);
    assert_eq!(h.clients(), 1);
}

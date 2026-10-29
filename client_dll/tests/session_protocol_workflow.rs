//! Portable end-to-end room contracts with virtual time and own data only.
use multiplayer_server::{
    protocol::{Position, Rotation},
    session_client::{Client, Event, Options, Phase},
    session_hub::{Hub, Settings},
    session_protocol::{
        self as w, ClientFrame, ClientMessage as C, ServerFrame, ServerMessage as S,
    },
};
fn addr(port: u16) -> std::net::SocketAddr {
    ([127, 0, 0, 1], port).into()
}
fn hub() -> Hub {
    Hub::new(
        Settings {
            room: "lan".into(),
            profile: w::PROFILE.into(),
            max_clients: 16,
            timeout_ms: 2000,
            echo: false,
        },
        91,
    )
    .unwrap()
}
fn client(id: u64) -> Client {
    Client::new(
        Options {
            room: "lan".into(),
            identity: format!("{id:032x}"),
            profile: w::PROFILE.into(),
        },
        id,
    )
    .unwrap()
}
fn exchange(
    h: &mut Hub,
    c: &mut Client,
    port: u16,
    frame: ClientFrame,
    server_ms: u64,
    client_ms: u64,
) -> Vec<Event> {
    let output = h
        .ingest(
            addr(port),
            &w::encode_client(&frame).unwrap(),
            server_ms,
            || Ok("a".repeat(32)),
        )
        .unwrap();
    output
        .into_iter()
        .flat_map(|(_, f)| c.ingest(&w::encode_server(&f).unwrap(), client_ms))
        .collect()
}
fn connect(h: &mut Hub, c: &mut Client, port: u16, now: u64) {
    let frame = c.poll(now).0.remove(0);
    exchange(h, c, port, frame, now, now);
    assert_eq!(c.status().phase, Phase::Connected);
}
fn receive(h: &mut Hub, c: &mut Client, port: u16, now: u64) -> Vec<Event> {
    h.tick(now)
        .into_iter()
        .filter(|(a, _)| *a == addr(port))
        .flat_map(|(_, f)| c.ingest(&w::encode_server(&f).unwrap(), now))
        .collect()
}
#[test]
fn admitted_connection_timing_ignores_other_rooms_resets_on_accept_and_survives_leave() {
    let (mut h, mut a, mut b) = (hub(), client(1), client(2));
    connect(&mut h, &mut a, 1, 0);
    connect(&mut h, &mut b, 2, 0);
    let pose = b
        .pose(
            Position {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            None,
            1,
        )
        .unwrap();
    exchange(&mut h, &mut b, 2, pose, 1, 1);
    let frames = h.tick(1);
    let snapshot = frames
        .iter()
        .find(|(address, f)| *address == addr(1) && matches!(f.message, S::Snapshot { .. }))
        .unwrap()
        .1
        .clone();
    for (_, frame) in frames
        .into_iter()
        .filter(|(address, _)| *address == addr(1))
    {
        a.ingest(&w::encode_server(&frame).unwrap(), 101);
    }
    for (_, frame) in h
        .tick(17)
        .into_iter()
        .filter(|(address, _)| *address == addr(1))
    {
        a.ingest(&w::encode_server(&frame).unwrap(), 141);
    }
    assert_eq!(a.timing().snapshots, 2);
    assert_eq!(a.timing().arrival_jitter_us, 1500);
    let mut other_room = snapshot;
    if let S::Snapshot { session, .. } = &mut other_room.message {
        *session = 999;
    }
    let before = a.timing();
    a.ingest(&w::encode_server(&other_room).unwrap(), 150);
    assert_eq!(a.timing(), before);
    a.request_reconnect();
    let join = a.poll(151).0.remove(0);
    exchange(&mut h, &mut a, 1, join, 151, 151);
    assert_eq!(a.timing(), Default::default());
    for (_, frame) in h
        .tick(160)
        .into_iter()
        .filter(|(address, _)| *address == addr(1))
    {
        a.ingest(&w::encode_server(&frame).unwrap(), 260);
    }
    let before_close = a.timing();
    assert_eq!(before_close.snapshots, 1);
    a.close(261);
    let leave = a
        .poll(261)
        .0
        .into_iter()
        .find(|f| matches!(f.message, C::Leave { .. }))
        .unwrap();
    exchange(&mut h, &mut a, 1, leave, 261, 261);
    assert_eq!(a.status().phase, Phase::Closed);
    assert_eq!(a.timing(), before_close);
}
#[test]
fn lost_acceptance_retries_without_a_second_player_and_refuses_incompatible_rooms() {
    let (mut h, mut c) = (hub(), client(1));
    assert!(
        c.pose(
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            None,
            0
        )
        .is_none()
    );
    let join = c.poll(0).0.remove(0);
    h.ingest(addr(1), &w::encode_client(&join).unwrap(), 0, || {
        Ok("a".repeat(32))
    })
    .unwrap();
    assert!(c.poll(499).0.is_empty());
    let retry = c.poll(500).0.remove(0);
    exchange(&mut h, &mut c, 1, retry, 500, 500);
    assert_eq!(h.metrics.accepted, 1);
    assert_eq!(c.status().accepted, 1);
    let mut bad = client(2);
    let mut join = bad.poll(0).0.remove(0);
    if let C::Join { room, .. } = &mut join.message {
        *room = "missing".into();
    }
    exchange(&mut h, &mut bad, 2, join, 501, 0);
    assert_eq!(bad.status().phase, Phase::Rejected);
    assert_eq!(h.clients(), 1);
}
#[test]
fn reconnecting_player_keeps_id_but_resets_generation_and_old_datagrams_cannot_resurrect_it() {
    let (mut h, mut a, mut b) = (hub(), client(1), client(2));
    connect(&mut h, &mut a, 1, 0);
    connect(&mut h, &mut b, 2, 0);
    let player = a.status().player;
    let position = Position {
        x: 10.0,
        y: -2.0,
        z: 30.0,
    };
    let turn = Some(Rotation {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    });
    let pose = a.pose(position, turn, 1).unwrap();
    exchange(&mut h, &mut a, 1, pose, 1, 1);
    let old = h
        .tick(2)
        .into_iter()
        .find(|(a, f)| *a == addr(2) && matches!(f.message, S::Snapshot { .. }))
        .unwrap()
        .1;
    let events = b.ingest(&w::encode_server(&old).unwrap(), 2);
    assert!(matches!(
        &events[0],
        Event::Pose {
            reset_history: true,
            ..
        }
    ));
    assert!(b.ingest(&w::encode_server(&old).unwrap(), 3).is_empty());
    for frame in b.poll(1500).0 {
        exchange(&mut h, &mut b, 2, frame, 1500, 1500);
    }
    let (frames, events) = a.poll(2100);
    assert!(events.iter().any(|e| matches!(e, Event::Reset)));
    assert_eq!(a.status().phase, Phase::Reconnecting);
    let old_connection = a.status().connection;
    for frame in frames {
        exchange(&mut h, &mut a, 3, frame, 2100, 2100);
    }
    assert_eq!(a.status().player, player);
    assert_ne!(a.status().connection, old_connection);
    let pose = a.pose(position, turn, 2101).unwrap();
    if let C::Pose { sequence, .. } = pose.message {
        assert_eq!(sequence, 0);
    } else {
        panic!()
    }
    exchange(&mut h, &mut a, 3, pose, 2101, 2101);
    let events = receive(&mut h, &mut b, 2, 2102);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Pose {
            reset_history: true,
            ..
        }
    )));
    assert!(b.ingest(&w::encode_server(&old).unwrap(), 2103).is_empty());
    a.close(2104);
    for frame in a.poll(2104).0 {
        exchange(&mut h, &mut a, 3, frame, 2104, 2104);
    }
    assert!(a.status().leave_confirmed);
    let events = receive(&mut h, &mut b, 2, 2105);
    assert!(
        events
            .iter()
            .any(|e| matches!(e,Event::Roster(ids)if ids.is_empty()))
    );
    assert!(b.ingest(&w::encode_server(&old).unwrap(), 2106).is_empty());
}
#[test]
fn rtt_uses_matching_pending_ping_and_offset_is_a_diagnostic_estimate() {
    let (mut h, mut c) = (hub(), client(1));
    let join = c.poll(0).0.remove(0);
    exchange(&mut h, &mut c, 1, join, 1000, 0);
    let ping = c.poll(0).0.remove(0);
    let frames = h
        .ingest(
            addr(1),
            &w::encode_client(&ping).unwrap(),
            1010,
            || panic!(),
        )
        .unwrap();
    let bytes = w::encode_server(&frames[0].1).unwrap();
    c.ingest(&bytes, 20);
    assert_eq!(c.status().rtt_ms, Some(20));
    assert_eq!(c.status().clock_offset_ms, Some(1000));
    c.ingest(&bytes, 21);
    assert_eq!(c.status().old, 1);
    let ping = c.poll(500).0.remove(0);
    exchange(&mut h, &mut c, 1, ping, 1510, 540);
    assert_eq!(c.status().rtt_ms, Some(40));
    assert_eq!(c.status().best_rtt_ms, Some(20));
    assert_eq!(c.status().clock_offset_ms, Some(1000));
}
#[test]
fn oversized_rosters_invalid_versions_and_duplicate_players_do_not_mutate_live_tracks() {
    let (mut h, mut c) = (hub(), client(1));
    connect(&mut h, &mut c, 1, 0);
    for i in 0..20 {
        let poses = (0..4)
            .map(|j| w::Pose {
                player: 100 + i * 4 + j,
                generation: 1,
                sequence: 0,
                received_ms: 0,
                position: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: None,
            })
            .collect();
        let bytes = w::encode_server(&ServerFrame::new(S::Snapshot {
            session: 91,
            revision: 2,
            tick: 1,
            server_ms: 0,
            poses,
        }))
        .unwrap();
        c.ingest(&bytes, 0);
    }
    assert_eq!(c.status().invalid, 16);
    let before = c.status().invalid;
    c.ingest(&vec![b' '; 1201], 1);
    assert_eq!(c.status().invalid, before + 1);
    let mut frame = ServerFrame::new(S::Roster {
        session: 91,
        revision: 3,
        players: vec![100],
    });
    frame.version = 255;
    c.ingest(&multiplayer_server::protocol::encode(&frame).unwrap(), 1);
    assert_eq!(c.status().phase, Phase::Connected);
}
#[test]
fn identity_is_local_persistent_and_corrupt_or_oversized_files_are_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.json");
    let first = multiplayer_server::identity::load_or_create(&path).unwrap();
    assert_eq!(
        first,
        multiplayer_server::identity::load_or_create(&path).unwrap()
    );
    assert!(w::identity_valid(&first));
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(multiplayer_server::identity::load_or_create(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    std::fs::write(&path, [b' '; 1025]).unwrap();
    assert!(multiplayer_server::identity::load_or_create(&path).is_err());
}
#[test]
fn session_control_json_fits_existing_abi_even_with_large_counters_and_never_returns_tokens() {
    let mut status = client(1).status();
    status.session = u64::MAX;
    status.connection = u64::MAX;
    status.player = u32::MAX;
    status.reconnects = u64::MAX;
    status.accepted = u64::MAX;
    status.old = u64::MAX;
    status.invalid = u64::MAX;
    status.rtt_ms = Some(u64::MAX);
    status.best_rtt_ms = Some(u64::MAX);
    status.clock_offset_ms = Some(i64::MIN);
    status.phase = Phase::Reconnecting;
    status.refusal = Some(w::Refusal::EndpointInUse);
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.len() < 512, "{}", json.len());
    assert!(!json.contains("token"));
    assert_eq!(std::mem::size_of::<client_dll::api::Control>(), 1536);
}

#[test]
fn seeded_loss_duplication_and_reordering_converge_after_room_restart() {
    use client_dll::network_conditions::{Conditions, Link};
    fn down(
        frames: Vec<multiplayer_server::session_hub::Datagram>,
        links: &mut [Link; 2],
        now: u64,
    ) {
        for (address, frame) in frames {
            let index = usize::from(address.port() - 1);
            links[index]
                .submit(now, &w::encode_server(&frame).unwrap())
                .unwrap();
        }
    }
    for seed in 1..=8 {
        let conditions = Conditions {
            seed,
            loss_permille: 250,
            duplicate_permille: 300,
            delay_ms: 40,
            jitter_ms: 80,
            queue_capacity: 128,
        };
        let mut up = [
            Link::new(conditions.clone()).unwrap(),
            Link::new(conditions.reverse()).unwrap(),
        ];
        let mut dl = [
            Link::new(conditions.reverse().reverse()).unwrap(),
            Link::new(conditions.reverse().reverse().reverse()).unwrap(),
        ];
        let (mut h, mut clients, mut poses) = (hub(), [client(1), client(2)], [0u64; 2]);
        for now in 0..18000 {
            if now == 8000 {
                h = Hub::new(
                    Settings {
                        room: "lan".into(),
                        profile: w::PROFILE.into(),
                        max_clients: 2,
                        timeout_ms: 2000,
                        echo: false,
                    },
                    92,
                )
                .unwrap();
            }
            for index in 0..2 {
                if now % 16 == 0 {
                    let mut output = clients[index].poll(now).0;
                    if let Some(frame) = clients[index].pose(
                        Position {
                            x: index as f32 * 100.0,
                            y: 0.0,
                            z: 0.0,
                        },
                        None,
                        now,
                    ) {
                        output.push(frame);
                    }
                    for frame in output {
                        up[index]
                            .submit(now, &w::encode_client(&frame).unwrap())
                            .unwrap();
                    }
                }
                while let Some(bytes) = up[index].pop(now) {
                    let output = h
                        .ingest(addr(index as u16 + 1), &bytes, now, || Ok("a".repeat(32)))
                        .unwrap();
                    down(output, &mut dl, now);
                }
            }
            if now % 16 == 0 {
                down(h.tick(now), &mut dl, now);
            }
            for index in 0..2 {
                while let Some(bytes) = dl[index].pop(now) {
                    for event in clients[index].ingest(&bytes, now) {
                        if let Event::Pose { pose, .. } = event {
                            assert_ne!(pose.player, clients[index].status().player);
                            assert_eq!(pose.position.x, (1 - index) as f32 * 100.0);
                            poses[index] += 1;
                        }
                    }
                }
            }
            assert!(h.clients() <= 2 && h.members() <= 2);
        }
        for (index, c) in clients.iter().enumerate() {
            assert_eq!(
                c.status().phase,
                Phase::Connected,
                "seed {seed}: {:?}",
                c.status()
            );
            assert_eq!(c.status().session, 92);
            assert!(c.status().reconnects > 0);
            assert!(poses[index] > 100, "seed {seed}: no useful remote stream");
        }
        for link in up.iter().chain(&dl) {
            assert!(link.counters().peak_queue <= 128);
        }
    }
}

//! Reliable v2 events end to end with virtual time and own data only: ids,
//! acknowledgements, retransmission, deduplication, deadlines and bounds.
use client_dll::network_conditions::{Conditions, Link};
use multiplayer_server::{
    protocol::Position,
    session_client::{Client, EVENT_QUEUE, Event, EventError, Options, PING_LOSS_MS, Phase},
    session_hub::{Datagram, Hub, RELAY_QUEUE, RELAYS_PER_TICK, Settings},
    session_protocol::{
        self as w, ClientFrame, ClientMessage as C, EventBody, PlayerState, ServerMessage as S,
    },
};
use std::collections::BTreeMap;

fn addr(port: u16) -> std::net::SocketAddr {
    ([127, 0, 0, 1], port).into()
}

#[test]
fn old_relays_never_reappear_after_more_than_a_window_of_unique_events() {
    let (mut h, mut clients) = (hub(1, false), [client(1)]);
    step(&mut h, &mut clients, 0);
    let session = clients[0].status().session;
    let relay = |event| {
        w::encode_server(&w::ServerFrame::new(S::Relay {
            session,
            event,
            origin: 2,
            server_ms: event,
            body: EventBody::Marker { code: event as u32 },
        }))
        .unwrap()
    };
    for id in 1..=300 {
        assert_eq!(clients[0].ingest(&relay(id), id).len(), 1);
    }
    assert!(clients[0].ingest(&relay(1), 301).is_empty());
    assert_eq!(clients[0].reliability().relays, 300);
    assert_eq!(clients[0].reliability().relay_duplicates, 1);
    assert!(
        clients[0]
            .poll(302)
            .0
            .iter()
            .any(|f| matches!(f.message, C::RelayAck { event: 1, .. }))
    );
}

#[test]
fn replacing_pending_states_keeps_markers_and_does_not_fake_acknowledgements() {
    let (mut h, mut clients) = (hub(1, false), [client(1)]);
    step(&mut h, &mut clients, 0);
    clients[0]
        .event(EventBody::Marker { code: 7 }, 5000, 1)
        .unwrap();
    clients[0]
        .event(
            EventBody::State {
                state: PlayerState::Loading,
            },
            5000,
            1,
        )
        .unwrap();
    clients[0]
        .event(
            EventBody::State {
                state: PlayerState::Driving,
            },
            5000,
            2,
        )
        .unwrap();
    let r = clients[0].reliability();
    assert_eq!((r.pending, r.superseded, r.acked), (2, 1, 0));
    let events: Vec<_> = clients[0]
        .poll(3)
        .0
        .into_iter()
        .filter_map(|f| match f.message {
            C::Event { body, .. } => Some(body),
            _ => None,
        })
        .collect();
    assert_eq!(
        events,
        [
            EventBody::Marker { code: 7 },
            EventBody::State {
                state: PlayerState::Driving
            }
        ]
    );
}
fn hub(max_clients: usize, echo: bool) -> Hub {
    Hub::new(
        Settings {
            room: "lan".into(),
            profile: w::PROFILE.into(),
            max_clients,
            timeout_ms: 5000,
            echo,
        },
        77,
    )
    .unwrap()
}
fn client(id: u64) -> Client {
    Client::new(
        Options {
            admission: None,
            room: "lan".into(),
            identity: format!("{id:032x}"),
            profile: w::PROFILE.into(),
        },
        id,
    )
    .unwrap()
}
fn up(h: &mut Hub, port: u16, frame: &ClientFrame, now: u64) -> Vec<Datagram> {
    h.ingest(addr(port), &w::encode_client(frame).unwrap(), now, || {
        Ok(format!("{port:032x}"))
    })
    .unwrap()
}
/// Lossless exchange: client frames up, every reply/tick down to its owner.
fn step(h: &mut Hub, clients: &mut [Client], now: u64) -> Vec<Vec<Event>> {
    let mut events: Vec<Vec<Event>> = clients.iter().map(|_| Vec::new()).collect();
    let mut down = Vec::new();
    for (i, c) in clients.iter_mut().enumerate() {
        let (frames, local) = c.poll(now);
        events[i].extend(local);
        for frame in frames {
            down.extend(up(h, i as u16 + 1, &frame, now));
        }
    }
    down.extend(h.tick(now));
    for (address, frame) in down {
        let i = usize::from(address.port() - 1);
        events[i].extend(clients[i].ingest(&w::encode_server(&frame).unwrap(), now));
    }
    events
}
fn relays(events: &[Event]) -> Vec<(u32, EventBody)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Relay { origin, body, .. } => Some((*origin, *body)),
            _ => None,
        })
        .collect()
}

#[test]
fn markers_cross_seeded_loss_duplication_and_reordering_exactly_once() {
    fn deliver(frames: Vec<Datagram>, links: &mut [Link; 2], now: u64) {
        for (address, frame) in frames {
            links[usize::from(address.port() - 1)]
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
        let mut uplinks = [
            Link::new(conditions.clone()).unwrap(),
            Link::new(conditions.reverse()).unwrap(),
        ];
        let mut downlinks = [
            Link::new(conditions.reverse().reverse()).unwrap(),
            Link::new(conditions.reverse().reverse().reverse()).unwrap(),
        ];
        let (mut h, mut clients) = (hub(2, false), [client(1), client(2)]);
        let mut received: BTreeMap<u32, u32> = BTreeMap::new();
        let mut queued = 0u32;
        for now in 0..30_000u64 {
            // 20 markers from A, spaced 400 ms, once BOTH are in the room: relays
            // go only to players connected when the event is admitted.
            if now % 400 == 0 && queued < 20 && clients[0].connected() && h.clients() == 2 {
                queued += 1;
                clients[0]
                    .event(EventBody::Marker { code: queued }, 20_000, now)
                    .unwrap();
            }
            for i in 0..2 {
                if now % 16 == 0 {
                    let mut output = clients[i].poll(now).0;
                    output.extend(clients[i].pose(
                        Position {
                            x: i as f32,
                            y: 0.0,
                            z: 0.0,
                        },
                        None,
                        now,
                    ));
                    for frame in output {
                        uplinks[i]
                            .submit(now, &w::encode_client(&frame).unwrap())
                            .unwrap();
                    }
                }
                while let Some(bytes) = uplinks[i].pop(now) {
                    let output = h
                        .ingest(addr(i as u16 + 1), &bytes, now, || Ok("a".repeat(32)))
                        .unwrap();
                    deliver(output, &mut downlinks, now);
                }
            }
            if now % 16 == 0 {
                deliver(h.tick(now), &mut downlinks, now);
            }
            for i in 0..2 {
                while let Some(bytes) = downlinks[i].pop(now) {
                    for (origin, body) in relays(&clients[i].ingest(&bytes, now)) {
                        assert_eq!(i, 1, "seed {seed}: no echo configured");
                        assert_eq!(origin, clients[0].status().player);
                        let EventBody::Marker { code } = body else {
                            panic!("unexpected body {body:?}");
                        };
                        *received.entry(code).or_default() += 1;
                    }
                }
            }
        }
        assert_eq!(queued, 20, "seed {seed}: A never connected");
        assert_eq!(received.len(), 20, "seed {seed}: {received:?}");
        assert!(
            received.values().all(|n| *n == 1),
            "seed {seed}: {received:?}"
        );
        let r = clients[0].reliability();
        assert_eq!(
            (r.acked, r.expired, r.pending),
            (20, 0, 0),
            "seed {seed}: {r:?}"
        );
        assert!(
            r.retransmitted > 0,
            "seed {seed}: loss never exercised resend"
        );
        assert_eq!(h.metrics.events, 20);
        assert!(h.metrics.event_duplicates > 0 || h.metrics.relays_retransmitted > 0);
        assert_eq!(h.metrics.relays_expired, 0);
        // Pings were lost too; the estimate counts unanswered round trips only.
        assert!(r.pings_lost > 0 && r.pongs > 0 && r.pongs + r.pings_lost <= r.pings_sent);
    }
}

#[test]
fn reconnection_resends_with_new_credentials_and_the_room_deduplicates() {
    let (mut h, mut clients) = (hub(2, false), [client(1), client(2)]);
    step(&mut h, &mut clients, 0);
    step(&mut h, &mut clients, 1);
    assert!(clients.iter().all(Client::connected));
    clients[0]
        .event(EventBody::Marker { code: 9 }, 10_000, 2)
        .unwrap();
    // First copy reaches the room, but its acknowledgement is lost.
    let (frames, _) = clients[0].poll(2);
    let event = frames
        .into_iter()
        .find(|f| matches!(f.message, C::Event { .. }))
        .unwrap();
    let ack = up(&mut h, 1, &event, 2);
    assert!(matches!(ack[0].1.message, S::EventAck { .. }));
    assert_eq!(h.metrics.events, 1);
    clients[0].request_reconnect();
    let mut delivered = Vec::new();
    for now in 3..2000 {
        let events = step(&mut h, &mut clients, now);
        delivered.extend(relays(&events[1]));
    }
    assert_eq!(clients[0].status().phase, Phase::Connected);
    assert_eq!(clients[0].status().reconnects, 1);
    assert_eq!(h.metrics.event_duplicates, 1, "resend after reconnection");
    assert_eq!(delivered.len(), 1);
    assert_eq!(clients[0].reliability().acked, 1);
    assert_eq!(clients[0].reliability().pending, 0);
}

#[test]
fn latest_state_wins_and_is_replayed_to_a_late_joiner_once() {
    let mut h = hub(3, false);
    let mut first = [client(1)];
    step(&mut h, &mut first, 0);
    step(&mut h, &mut first, 1);
    for (now, state) in [(2, PlayerState::Loading), (3, PlayerState::Driving)] {
        first[0]
            .event(EventBody::State { state }, 5000, now)
            .unwrap();
        step(&mut h, &mut first, now);
    }
    assert_eq!(h.metrics.events, 2);
    let mut clients = [first.into_iter().next().unwrap(), client(2)];
    let mut states = Vec::new();
    for now in 10..3000 {
        states.extend(relays(&step(&mut h, &mut clients, now)[1]));
    }
    assert_eq!(
        states,
        vec![(
            clients[0].status().player,
            EventBody::State {
                state: PlayerState::Driving
            }
        )]
    );
    assert_eq!(h.metrics.relays_expired, 0);
    assert!(h.metrics.relays_acked >= 1);
}

#[test]
fn deadlines_bounded_queues_and_echo_never_retry_forever() {
    // Client: refuses beyond EVENT_QUEUE and expires without a server.
    let mut alone = client(5);
    alone.poll(0);
    for code in 0..EVENT_QUEUE as u32 {
        alone.event(EventBody::Marker { code }, 100, 0).unwrap();
    }
    assert_eq!(
        alone.event(EventBody::Marker { code: 99 }, 100, 0),
        Err(EventError::Full)
    );
    assert_eq!(
        alone.event(EventBody::Marker { code: 1 }, w::EVENT_TTL_MAX_MS + 1, 0),
        Err(EventError::InvalidLifetime)
    );
    alone.poll(100);
    let r = alone.reliability();
    assert_eq!((r.expired, r.refused, r.pending, r.sent), (16, 1, 0, 0));
    // Room: a recipient that never acknowledges gets bounded retries, then
    // expiry; a full recipient queue drops the oldest relay instead of growing.
    let mut h = hub(2, false);
    let mut clients = [client(1), client(2)];
    step(&mut h, &mut clients, 0);
    step(&mut h, &mut clients, 1);
    let extra = 6;
    for code in 0..(RELAY_QUEUE + extra) as u32 {
        clients[0]
            .event(EventBody::Marker { code }, 1000, 2)
            .unwrap();
        for frame in clients[0].poll(2).0 {
            // Acknowledgements return to A only; B never sees or acks relays.
            for (address, reply) in up(&mut h, 1, &frame, 2) {
                if address == addr(1) {
                    clients[0].ingest(&w::encode_server(&reply).unwrap(), 2);
                }
            }
        }
    }
    assert_eq!(clients[0].reliability().acked, (RELAY_QUEUE + extra) as u64);
    assert_eq!(h.metrics.relays_dropped, extra as u64);
    let mut max_per_tick = 0;
    for now in (3..1100).step_by(16) {
        let to_b = h
            .tick(now)
            .iter()
            .filter(|(a, f)| *a == addr(2) && matches!(f.message, S::Relay { .. }))
            .count();
        max_per_tick = max_per_tick.max(to_b);
    }
    assert_eq!(max_per_tick, RELAYS_PER_TICK);
    assert_eq!(h.metrics.relays_expired, RELAY_QUEUE as u64);
    assert!(h.metrics.relays_retransmitted > 0);
    assert!(
        !h.tick(1200)
            .iter()
            .any(|(_, f)| matches!(f.message, S::Relay { .. }))
    );
}

#[test]
fn echo_relays_the_sender_its_own_event_under_the_echo_identity() {
    let mut h = hub(1, true);
    let mut clients = [client(1)];
    step(&mut h, &mut clients, 0);
    step(&mut h, &mut clients, 1);
    clients[0]
        .event(EventBody::Marker { code: 42 }, 5000, 2)
        .unwrap();
    let mut got = Vec::new();
    for now in 2..200 {
        got.extend(relays(&step(&mut h, &mut clients, now)[0]));
    }
    assert_eq!(
        got,
        vec![(
            w::ECHO_BASE + clients[0].status().player,
            EventBody::Marker { code: 42 }
        )]
    );
}

#[test]
fn uplink_pose_gaps_are_counted_with_a_bounded_jump_and_ping_loss_needs_a_timeout() {
    let mut h = hub(1, false);
    let mut clients = [client(1)];
    step(&mut h, &mut clients, 0);
    step(&mut h, &mut clients, 1);
    let frame = clients[0]
        .pose(
            Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            None,
            2,
        )
        .unwrap();
    let C::Pose { credentials, .. } = frame.message else {
        unreachable!()
    };
    for (sequence, now) in [(10u64, 3), (15, 4), (14, 5), (u64::MAX, 6)] {
        up(
            &mut h,
            1,
            &ClientFrame::new(C::Pose {
                credentials: credentials.clone(),
                sequence,
                client_ms: now,
                position: Position {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: None,
            }),
            now,
        );
    }
    // First pose 10 has no reference; 15 skips 4; 14 is stale; MAX is bounded.
    assert_eq!(h.metrics.pose_gaps, 4 + 4096);
    assert_eq!(h.metrics.stale, 1);
    // Unanswered pings become lost only after PING_LOSS_MS.
    let mut lonely = client(3);
    let mut room = hub(1, false);
    let join = lonely.poll(0).0.remove(0);
    for (_, reply) in up(&mut room, 9, &join, 0) {
        lonely.ingest(&w::encode_server(&reply).unwrap(), 0);
    }
    assert!(lonely.connected());
    lonely.poll(1);
    assert_eq!(lonely.reliability().pings_sent, 1);
    lonely.poll(PING_LOSS_MS);
    assert_eq!(lonely.reliability().pings_lost, 0);
    lonely.poll(PING_LOSS_MS + 1);
    assert_eq!(lonely.reliability().pings_lost, 1);
}

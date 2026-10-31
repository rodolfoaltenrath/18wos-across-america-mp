//! Single-room authority, bounded membership, generations and ingress. No game,
//! sockets or wall clock; adapter supplies monotonic milliseconds and OS entropy.
use crate::session_protocol::{
    self as wire, BATCH, ClientFrame, ClientMessage as C, Credentials, ECHO_BASE, MAX_PLAYERS,
    Pose, Refusal, ServerFrame, ServerMessage as S,
};
use crate::traffic::Window;
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
};

#[derive(Clone, Debug, Serialize)]
pub struct Settings {
    pub room: String,
    pub profile: String,
    pub max_clients: usize,
    pub timeout_ms: u64,
    pub echo: bool,
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !wire::room_valid(&self.room)
            || !wire::profile_valid(&self.profile)
            || !(1..=MAX_PLAYERS).contains(&self.max_clients)
            || !(2000..=60000).contains(&self.timeout_ms)
        {
            return Err("invalid session room/settings".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Metrics {
    pub stale_states: u64,
    pub relays_superseded: u64,
    pub received: u64,
    pub invalid: u64,
    pub oversized: u64,
    pub rate_limited: u64,
    pub accepted: u64,
    pub refused: u64,
    pub poses: u64,
    pub pings: u64,
    pub stale: u64,
    pub unauthenticated: u64,
    pub expired: u64,
    pub left: u64,
    pub ticks: u64,
    pub snapshots: u64,
    pub sent: u64,
    pub bytes_sent: u64,
    pub send_errors: u64,
    pub late_ticks: u64,
    /// Sequence numbers skipped by uplink poses of a connection: loss OR
    /// reordering (late copies then count as `stale`), never decimation.
    pub pose_gaps: u64,
    /// Reliable events admitted for the first time / repeated by the sender.
    pub events: u64,
    pub event_duplicates: u64,
    /// Relay datagrams: first copies, retransmissions, receipts, give-ups.
    pub relays_sent: u64,
    pub relays_retransmitted: u64,
    pub relays_acked: u64,
    pub relays_expired: u64,
    /// Oldest pending relay discarded because a recipient queue was full.
    pub relays_dropped: u64,
}
/// Relay queue per recipient, recent ids per sender stream and their bounds.
pub const RELAY_QUEUE: usize = 64;
pub const EVENT_STREAMS: usize = 4;
pub const EVENT_WINDOW: usize = 128;
/// Relay datagrams per recipient per tick; keeps the full-room batch bounded.
pub const RELAYS_PER_TICK: usize = 2;
pub const RELAY_RETRY_MS: u64 = 250;
pub const RELAY_RETRY_MAX_MS: u64 = 1000;
/// Lifetime of the state replay sent to a newcomer.
const STATE_REPLAY_TTL_MS: u64 = 5000;
struct Member {
    player: u32,
    last_seen: u64,
    /// Recent event ids per client stream, deduplicating across reconnections.
    streams: VecDeque<(u64, crate::event_window::Window<EVENT_WINDOW>)>,
    /// Latest state event (room sequence), replayed to players who join later.
    // Room sequence, body, sender stream and id: receipt order is not causality.
    state: Option<(u64, wire::PlayerState, u64, u64)>,
}
#[derive(Clone, Copy)]
struct Relay {
    event: u64,
    origin: u32,
    server_ms: u64,
    body: wire::EventBody,
    expires: u64,
    next_at: u64,
    interval: u64,
    sent: bool,
}
struct Peer {
    identity: String,
    address: SocketAddr,
    credentials: Credentials,
    request: u64,
    last_seen: u64,
    ping: Option<u64>,
    pose: Option<Pose>,
    rate: Window,
    relays: VecDeque<Relay>,
}
pub struct Hub {
    pub settings: Settings,
    pub session: u64,
    pub metrics: Metrics,
    peers: BTreeMap<u32, Peer>,
    members: BTreeMap<String, Member>,
    leaves: VecDeque<(SocketAddr, Credentials, u64)>,
    next_player: u32,
    next_connection: u64,
    revision: u64,
    now: u64,
    unknown_rate: Window,
    roster_dirty: bool,
    roster_at: u64,
    admission: Option<crate::career_entry::Registry>,
    next_event: u64,
}
pub type Datagram = (SocketAddr, ServerFrame);
impl Hub {
    pub fn new(settings: Settings, session: u64) -> Result<Self, String> {
        settings.validate()?;
        if session == 0 {
            return Err("session id must be nonzero".into());
        }
        Ok(Self {
            settings,
            session,
            metrics: Metrics::default(),
            peers: BTreeMap::new(),
            members: BTreeMap::new(),
            leaves: VecDeque::new(),
            next_player: 1,
            next_connection: 1,
            revision: 1,
            now: 0,
            unknown_rate: Window::default(),
            roster_dirty: true,
            roster_at: 0,
            admission: None,
            next_event: 1,
        })
    }
    pub fn clients(&self) -> usize {
        self.peers.len()
    }
    pub fn with_admission(
        settings: Settings,
        session: u64,
        registry: crate::career_entry::Registry,
    ) -> Result<Self, String> {
        registry.check_settings(&settings.room, &settings.profile)?;
        let mut hub = Self::new(settings, session)?;
        hub.admission = Some(registry);
        Ok(hub)
    }
    pub fn members(&self) -> usize {
        self.members.len()
    }
    fn message(&self, address: SocketAddr, message: S) -> Datagram {
        (address, ServerFrame::new(message))
    }
    fn reject(&mut self, address: SocketAddr, request: u64, reason: Refusal) -> Vec<Datagram> {
        self.metrics.refused += 1;
        vec![self.message(
            address,
            S::Rejected {
                request,
                reason,
                supported: wire::VERSION,
            },
        )]
    }
    fn changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.roster_dirty = true;
    }
    fn remove(&mut self, player: u32) {
        if let Some(peer) = self.peers.remove(&player) {
            if let Some(member) = self.members.get_mut(&peer.identity) {
                member.last_seen = self.now;
            }
            self.changed();
        }
    }
    fn expire(&mut self, now: u64) {
        self.now = self.now.max(now);
        let expired: Vec<_> = self
            .peers
            .iter()
            .filter(|(_, p)| self.now.saturating_sub(p.last_seen) >= self.settings.timeout_ms)
            .map(|(id, _)| *id)
            .collect();
        for player in expired {
            self.remove(player);
            self.metrics.expired += 1;
        }
        self.leaves
            .retain(|(_, _, at)| self.now.saturating_sub(*at) < self.settings.timeout_ms);
    }
    fn accepted(&self, player: u32, peer: &Peer) -> Datagram {
        self.message(
            peer.address,
            S::Accepted {
                request: peer.request,
                credentials: peer.credentials.clone(),
                player,
                revision: self.revision,
                timeout_ms: self.settings.timeout_ms,
                server_ms: self.now,
            },
        )
    }
    fn join(
        &mut self,
        address: SocketAddr,
        frame: ClientFrame,
        token: impl FnOnce() -> Result<String, String>,
    ) -> Result<Vec<Datagram>, String> {
        let C::Join {
            request,
            room,
            identity,
            profile,
            resume,
            admission,
        } = frame.message
        else {
            unreachable!()
        };
        let version = frame.version;
        if version != wire::VERSION {
            return Ok(self.reject(address, request, Refusal::Version));
        }
        if room != self.settings.room {
            return Ok(self.reject(address, request, Refusal::Room));
        }
        if profile != self.settings.profile {
            return Ok(self.reject(address, request, Refusal::Profile));
        }
        let admitted = match &self.admission {
            Some(registry) => registry.allows(&identity, &room, &profile, admission.as_ref()),
            None => admission.is_none(),
        };
        if !admitted {
            return Ok(self.reject(address, request, Refusal::Admission));
        }
        let active = self
            .peers
            .iter()
            .find(|(_, p)| p.identity == identity)
            .map(|(id, p)| (*id, p));
        if let Some((id, peer)) = active {
            if peer.address == address && peer.request == request {
                return Ok(vec![self.accepted(id, peer)]);
            }
            if resume.as_ref() != Some(&peer.credentials) {
                return Ok(self.reject(address, request, Refusal::IdentityInUse));
            }
        }
        let active_id = active.map(|(id, _)| id);
        // Resuming on the same endpoint must not reset its ingress budget.
        let inherited_rate = active
            .filter(|(_, p)| p.address == address)
            .map(|(_, p)| p.rate.clone());
        // A resumed connection keeps its undelivered relays.
        let inherited_relays = active.map(|(_, p)| p.relays.clone());
        if self
            .peers
            .iter()
            .any(|(id, p)| p.address == address && Some(*id) != active_id)
        {
            return Ok(self.reject(address, request, Refusal::EndpointInUse));
        }
        if active_id.is_none() && self.peers.len() >= self.settings.max_clients {
            return Ok(self.reject(address, request, Refusal::Full));
        }
        if self.next_player >= ECHO_BASE || self.next_connection == u64::MAX {
            return Ok(self.reject(address, request, Refusal::Full));
        }
        let token = token()?;
        if !wire::identity_valid(&token) {
            return Err("invalid entropy token".into());
        }
        let player = if let Some(member) = self.members.get(&identity) {
            member.player
        } else {
            if self.members.len() >= MAX_PLAYERS {
                let oldest = self
                    .members
                    .iter()
                    .filter(|(_, m)| !self.peers.contains_key(&m.player))
                    .min_by_key(|(_, m)| m.last_seen)
                    .map(|(id, _)| id.clone())
                    .ok_or("membership capacity")?;
                self.members.remove(&oldest);
            }
            let id = self.next_player;
            self.next_player += 1;
            self.members.insert(
                identity.clone(),
                Member {
                    player: id,
                    last_seen: self.now,
                    streams: VecDeque::new(),
                    state: None,
                },
            );
            id
        };
        let relays = match inherited_relays {
            Some(relays) => relays,
            // Newcomer: replay the latest state of every other active player.
            None => self
                .peers
                .iter()
                .filter(|(other, _)| **other != player)
                .filter_map(|(other, p)| {
                    let (event, state, _, _) = self.members.get(&p.identity)?.state?;
                    Some(Relay {
                        event,
                        origin: *other,
                        server_ms: self.now,
                        body: wire::EventBody::State { state },
                        expires: self.now.saturating_add(STATE_REPLAY_TTL_MS),
                        next_at: self.now,
                        interval: RELAY_RETRY_MS,
                        sent: false,
                    })
                })
                .take(RELAY_QUEUE)
                .collect(),
        };
        let credentials = Credentials {
            session: self.session,
            connection: self.next_connection,
            token,
        };
        self.next_connection += 1;
        self.peers.insert(
            player,
            Peer {
                identity,
                address,
                credentials,
                request,
                last_seen: self.now,
                ping: None,
                pose: None,
                rate: inherited_rate.unwrap_or(Window {
                    start: self.now,
                    used: 1,
                }),
                relays,
            },
        );
        self.changed();
        self.metrics.accepted += 1;
        Ok(vec![self.accepted(player, &self.peers[&player])])
    }
    /// Limits apply BEFORE JSON decoding, including invalid traffic from a known
    /// endpoint. No per-unknown-address allocation: a shared 1024/s join budget.
    pub fn ingest(
        &mut self,
        address: SocketAddr,
        bytes: &[u8],
        now: u64,
        token: impl FnOnce() -> Result<String, String>,
    ) -> Result<Vec<Datagram>, String> {
        self.expire(now);
        self.metrics.received += 1;
        if bytes.len() > crate::protocol::MAX_PACKET {
            self.metrics.oversized += 1;
            return Ok(Vec::new());
        }
        let allowed = if let Some(peer) = self.peers.values_mut().find(|p| p.address == address) {
            peer.rate.admit(self.now, 240)
        } else {
            self.unknown_rate.admit(self.now, 1024)
        };
        if !allowed {
            self.metrics.rate_limited += 1;
            return Ok(Vec::new());
        }
        let Some(frame) = wire::decode_client(bytes) else {
            self.metrics.invalid += 1;
            return Ok(Vec::new());
        };
        if matches!(frame.message, C::Join { .. }) {
            return self.join(address, frame, token);
        }
        let credentials = match &frame.message {
            C::Pose { credentials, .. }
            | C::Ping { credentials, .. }
            | C::Leave { credentials }
            | C::Event { credentials, .. }
            | C::RelayAck { credentials, .. } => credentials,
            C::Join { .. } => unreachable!(),
        };
        let player = self
            .peers
            .iter()
            .find(|(_, p)| p.address == address && p.credentials == *credentials)
            .map(|(id, _)| *id);
        let Some(player) = player else {
            if matches!(frame.message, C::Leave { .. })
                && self
                    .leaves
                    .iter()
                    .any(|(a, c, _)| *a == address && c == credentials)
            {
                return Ok(vec![self.message(
                    address,
                    S::Left {
                        session: credentials.session,
                        connection: credentials.connection,
                    },
                )]);
            }
            self.metrics.unauthenticated += 1;
            return Ok(Vec::new());
        };
        match frame.message {
            C::Pose {
                sequence,
                position,
                rotation,
                ..
            } => {
                let peer = self.peers.get_mut(&player).unwrap();
                if peer.pose.is_some_and(|p| sequence <= p.sequence) {
                    self.metrics.stale += 1;
                } else {
                    // Bounded per jump: a restarted or hostile sequence cannot
                    // inflate the estimate arbitrarily.
                    if let Some(previous) = peer.pose {
                        let skipped = sequence - previous.sequence - 1;
                        self.metrics.pose_gaps =
                            self.metrics.pose_gaps.saturating_add(skipped.min(4096));
                    }
                    peer.last_seen = self.now;
                    peer.pose = Some(Pose {
                        player,
                        generation: peer.credentials.connection,
                        sequence,
                        received_ms: self.now,
                        position,
                        rotation,
                    });
                    self.metrics.poses += 1;
                }
                Ok(Vec::new())
            }
            C::Ping {
                sequence,
                client_ms,
                ..
            } => {
                let peer = self.peers.get_mut(&player).unwrap();
                if peer.ping.is_some_and(|last| sequence <= last) {
                    self.metrics.stale += 1;
                    return Ok(Vec::new());
                }
                peer.ping = Some(sequence);
                peer.last_seen = self.now;
                self.metrics.pings += 1;
                let connection = peer.credentials.connection;
                Ok(vec![self.message(
                    address,
                    S::Pong {
                        session: self.session,
                        connection,
                        sequence,
                        client_ms,
                        server_ms: self.now,
                    },
                )])
            }
            C::Leave { credentials } => {
                self.remove(player);
                self.metrics.left += 1;
                if self.leaves.len() == MAX_PLAYERS {
                    self.leaves.pop_front();
                }
                self.leaves
                    .push_back((address, credentials.clone(), self.now));
                Ok(vec![self.message(
                    address,
                    S::Left {
                        session: self.session,
                        connection: credentials.connection,
                    },
                )])
            }
            C::Event {
                credentials,
                stream,
                id,
                ttl_ms,
                body,
            } => {
                self.peers.get_mut(&player).unwrap().last_seen = self.now;
                let identity = self.peers[&player].identity.clone();
                let member = self.members.get_mut(&identity).ok_or("member missing")?;
                let duplicate = match member.streams.iter_mut().find(|(s, _)| *s == stream) {
                    Some((_, ids)) => !ids.admit(id),
                    None => {
                        if member.streams.len() == EVENT_STREAMS {
                            member.streams.pop_front();
                        }
                        let mut ids = crate::event_window::Window::default();
                        ids.admit(id);
                        member.streams.push_back((stream, ids));
                        false
                    }
                };
                let stale_state = matches!(body, wire::EventBody::State { .. })
                    && member
                        .state
                        .is_some_and(|(_, _, previous_stream, previous_id)| {
                            previous_stream == stream && id < previous_id
                        });
                if duplicate {
                    self.metrics.event_duplicates += 1;
                } else if stale_state {
                    self.metrics.stale_states += 1;
                } else {
                    self.metrics.events += 1;
                    let event = self.next_event;
                    self.next_event = self
                        .next_event
                        .checked_add(1)
                        .ok_or("event ids exhausted")?;
                    if let wire::EventBody::State { state } = body {
                        member.state = Some((event, state, stream, id));
                    }
                    let relay = Relay {
                        event,
                        origin: player,
                        server_ms: self.now,
                        body,
                        expires: self.now.saturating_add(ttl_ms),
                        next_at: self.now,
                        interval: RELAY_RETRY_MS,
                        sent: false,
                    };
                    let echo = self.settings.echo;
                    for (recipient, peer) in self.peers.iter_mut() {
                        let origin = if *recipient != player {
                            player
                        } else if echo {
                            ECHO_BASE + player
                        } else {
                            continue;
                        };
                        if matches!(body, wire::EventBody::State { .. }) {
                            let before = peer.relays.len();
                            peer.relays.retain(|r| {
                                !(r.origin == origin
                                    && matches!(r.body, wire::EventBody::State { .. }))
                            });
                            self.metrics.relays_superseded = self
                                .metrics
                                .relays_superseded
                                .saturating_add((before - peer.relays.len()) as u64);
                        }
                        if peer.relays.len() == RELAY_QUEUE {
                            peer.relays.pop_front();
                            self.metrics.relays_dropped += 1;
                        }
                        peer.relays.push_back(Relay { origin, ..relay });
                    }
                }
                Ok(vec![self.message(
                    address,
                    S::EventAck {
                        session: self.session,
                        connection: credentials.connection,
                        stream,
                        id,
                    },
                )])
            }
            C::RelayAck { event, .. } => {
                let peer = self.peers.get_mut(&player).unwrap();
                peer.last_seen = self.now;
                let before = peer.relays.len();
                peer.relays.retain(|r| r.event != event);
                if peer.relays.len() < before {
                    self.metrics.relays_acked += 1;
                } else {
                    self.metrics.stale += 1;
                }
                Ok(Vec::new())
            }
            C::Join { .. } => unreachable!(),
        }
    }
    /// Membership is full state, refreshed at least once a second; loss converges
    /// without treating an unreliable one-shot notification as a reliable event.
    pub fn tick(&mut self, now: u64) -> Vec<Datagram> {
        self.expire(now);
        self.metrics.ticks += 1;
        let roster = self.roster_dirty || self.now.saturating_sub(self.roster_at) >= 1000;
        let mut output = Vec::new();
        for (destination, peer) in &self.peers {
            if roster {
                let mut players: Vec<_> = self
                    .peers
                    .keys()
                    .filter(|id| *id != destination)
                    .copied()
                    .collect();
                if self.settings.echo {
                    players.push(ECHO_BASE + destination);
                }
                output.push(self.message(
                    peer.address,
                    S::Roster {
                        session: self.session,
                        revision: self.revision,
                        players,
                    },
                ));
            }
            let mut poses = Vec::new();
            for (source, state) in &self.peers {
                if let Some(mut pose) = state
                    .pose
                    .filter(|p| self.now.saturating_sub(p.received_ms) < 1000)
                {
                    if source == destination {
                        if !self.settings.echo {
                            continue;
                        }
                        pose.player += ECHO_BASE;
                    }
                    poses.push(pose);
                }
            }
            for chunk in poses.chunks(BATCH) {
                output.push(self.message(
                    peer.address,
                    S::Snapshot {
                        session: self.session,
                        revision: self.revision,
                        tick: self.metrics.ticks,
                        server_ms: self.now,
                        poses: chunk.to_vec(),
                    },
                ));
                self.metrics.snapshots += 1;
            }
        }
        if roster {
            self.roster_at = self.now;
            self.roster_dirty = false;
        }
        let (now, session) = (self.now, self.session);
        for peer in self.peers.values_mut() {
            let before = peer.relays.len();
            peer.relays.retain(|r| now < r.expires);
            self.metrics.relays_expired += (before - peer.relays.len()) as u64;
            for relay in peer
                .relays
                .iter_mut()
                .filter(|r| r.next_at <= now)
                .take(RELAYS_PER_TICK)
            {
                output.push((
                    peer.address,
                    ServerFrame::new(S::Relay {
                        session,
                        event: relay.event,
                        origin: relay.origin,
                        server_ms: relay.server_ms,
                        body: relay.body,
                    }),
                ));
                if relay.sent {
                    self.metrics.relays_retransmitted += 1;
                } else {
                    relay.sent = true;
                    self.metrics.relays_sent += 1;
                }
                relay.next_at = now.saturating_add(relay.interval);
                relay.interval = (relay.interval * 2).min(RELAY_RETRY_MAX_MS);
            }
        }
        output
    }
    pub fn closing(&self) -> Vec<Datagram> {
        self.peers
            .values()
            .map(|p| {
                self.message(
                    p.address,
                    S::Closing {
                        session: self.session,
                        connection: p.credentials.connection,
                    },
                )
            })
            .collect()
    }
}

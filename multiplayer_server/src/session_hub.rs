//! Single-room authority, bounded membership, generations and ingress. No game,
//! sockets or wall clock; adapter supplies monotonic milliseconds and OS entropy.
use crate::session_protocol::{
    self as wire, BATCH, ClientFrame, ClientMessage as C, Credentials, ECHO_BASE, MAX_PLAYERS,
    Pose, Refusal, ServerFrame, ServerMessage as S,
};
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
}
#[derive(Default, Clone)]
struct Window {
    start: u64,
    used: u32,
}
impl Window {
    fn admit(&mut self, now: u64, limit: u32) -> bool {
        if now.saturating_sub(self.start) >= 1000 {
            self.start = now;
            self.used = 0;
        }
        if self.used >= limit {
            return false;
        }
        self.used += 1;
        true
    }
}
struct Member {
    player: u32,
    last_seen: u64,
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
        })
    }
    pub fn clients(&self) -> usize {
        self.peers.len()
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
                },
            );
            id
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
            | C::Leave { credentials } => credentials,
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

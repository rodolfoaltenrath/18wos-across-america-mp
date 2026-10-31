//! Pure client session machine. No sockets, globals, game pointers or sleeping.
use crate::{
    protocol::{Position, Rotation},
    session_protocol::{
        self as wire, ClientFrame, ClientMessage as C, Credentials, EventBody, Pose, Refusal,
        ServerMessage as S,
    },
    session_timing::{Recorder, Timing},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub room: String,
    pub identity: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<crate::career_entry::Proof>,
}
impl Options {
    pub fn validate(&self) -> Result<(), String> {
        if !wire::room_valid(&self.room)
            || !wire::identity_valid(&self.identity)
            || !wire::profile_valid(&self.profile)
            || self.admission.as_ref().is_some_and(|p| !p.valid())
        {
            return Err("invalid room, identity or profile".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Joining,
    Connected,
    Reconnecting,
    Rejected,
    Closing,
    Closed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u8,
    pub phase: Phase,
    pub session: u64,
    pub connection: u64,
    pub player: u32,
    pub reconnects: u64,
    pub accepted: u64,
    pub old: u64,
    pub invalid: u64,
    pub rtt_ms: Option<u64>,
    pub best_rtt_ms: Option<u64>,
    pub clock_offset_ms: Option<i64>,
    pub refusal: Option<Refusal>,
    pub leave_confirmed: bool,
}
#[derive(Debug)]
pub enum Event {
    Reset,
    Roster(Vec<u32>),
    Pose {
        pose: Pose,
        reset_history: bool,
    },
    /// A reliable event from `origin`, delivered at most once per room session.
    Relay {
        origin: u32,
        event: u64,
        server_ms: u64,
        body: EventBody,
    },
}
/// Outgoing reliable events kept until acknowledged or expired.
pub const EVENT_QUEUE: usize = 16;
/// Relay sequences remembered for deduplication within one room session.
pub const RELAY_WINDOW: usize = 256;
const EVENT_RETRY_MS: u64 = 250;
const EVENT_RETRY_MAX_MS: u64 = 1000;
/// A ping without a pong after this long counts as lost (round trip).
pub const PING_LOSS_MS: u64 = 2000;
fn is_zero(value: &u64) -> bool {
    *value == 0
}
/// Reliability counters, separate from `Status` to keep that report compact.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reliability {
    /// Obsolete local states replaced in the queue. Markers are never coalesced.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub superseded: u64,
    pub queued: u64,
    pub sent: u64,
    pub retransmitted: u64,
    pub acked: u64,
    pub expired: u64,
    /// Refused because the outgoing queue was full.
    pub refused: u64,
    pub pending: u64,
    pub relays: u64,
    pub relay_duplicates: u64,
    pub pings_sent: u64,
    pub pongs: u64,
    /// Pings unanswered for PING_LOSS_MS or evicted from the window: a
    /// round-trip loss estimate that also includes server/scheduling stalls.
    pub pings_lost: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventError {
    Closed,
    Full,
    InvalidLifetime,
}
struct Outgoing {
    id: u64,
    body: EventBody,
    deadline: u64,
    next_at: u64,
    interval: u64,
    sent: bool,
}
pub struct Client {
    options: Options,
    status: Status,
    credentials: Option<Credentials>,
    request: u64,
    timeout_ms: u64,
    heard_at: u64,
    hello_at: Option<u64>,
    ping_at: Option<u64>,
    ping_sequence: u64,
    pose_sequence: u64,
    pings: VecDeque<(u64, u64)>,
    tracks: BTreeMap<u32, (u64, u64)>,
    revision: u64,
    roster: Option<(u64, Vec<u32>)>,
    now: u64,
    closing_at: u64,
    leave_at: Option<u64>,
    timing: Recorder,
    stream: u64,
    next_event: u64,
    outgoing: VecDeque<Outgoing>,
    relay_acks: VecDeque<u64>,
    seen: crate::event_window::Window<RELAY_WINDOW>,
    seen_session: u64,
    reliability: Reliability,
}
impl Client {
    pub fn new(options: Options, request: u64) -> Result<Self, String> {
        options.validate()?;
        if request == 0 {
            return Err("join nonce must be nonzero".into());
        }
        Ok(Self {
            options,
            request,
            status: Status {
                protocol: wire::VERSION,
                phase: Phase::Joining,
                session: 0,
                connection: 0,
                player: 0,
                reconnects: 0,
                accepted: 0,
                old: 0,
                invalid: 0,
                rtt_ms: None,
                best_rtt_ms: None,
                clock_offset_ms: None,
                refusal: None,
                leave_confirmed: false,
            },
            credentials: None,
            timeout_ms: 5000,
            heard_at: 0,
            hello_at: None,
            ping_at: None,
            ping_sequence: 0,
            pose_sequence: 0,
            pings: VecDeque::new(),
            tracks: BTreeMap::new(),
            revision: 0,
            roster: None,
            now: 0,
            closing_at: 0,
            leave_at: None,
            timing: Recorder::default(),
            // The random join nonce also names this process's event stream.
            stream: request,
            next_event: 1,
            outgoing: VecDeque::new(),
            relay_acks: VecDeque::new(),
            seen: Default::default(),
            seen_session: 0,
            reliability: Reliability::default(),
        })
    }
    pub fn status(&self) -> Status {
        self.status.clone()
    }
    pub fn timing(&self) -> Timing {
        self.timing.value
    }
    pub fn reliability(&self) -> Reliability {
        Reliability {
            pending: self.outgoing.len() as u64,
            ..self.reliability
        }
    }
    /// Queue a reliable event. It survives reconnections (same stream/id, so
    /// the server deduplicates) until acknowledged or `ttl_ms` elapses.
    pub fn event(&mut self, body: EventBody, ttl_ms: u64, now: u64) -> Result<u64, EventError> {
        if !(1..=wire::EVENT_TTL_MAX_MS).contains(&ttl_ms) {
            return Err(EventError::InvalidLifetime);
        }
        if matches!(
            self.status.phase,
            Phase::Rejected | Phase::Closing | Phase::Closed
        ) {
            return Err(EventError::Closed);
        }
        let next_event = self.next_event.checked_add(1).ok_or(EventError::Full)?;
        if matches!(body, EventBody::State { .. }) {
            let before = self.outgoing.len();
            self.outgoing
                .retain(|e| !matches!(e.body, EventBody::State { .. }));
            self.reliability.superseded = self
                .reliability
                .superseded
                .saturating_add((before - self.outgoing.len()) as u64);
        }
        if self.outgoing.len() == EVENT_QUEUE {
            self.reliability.refused += 1;
            return Err(EventError::Full);
        }
        self.now = self.now.max(now);
        let id = self.next_event;
        self.next_event = next_event;
        self.outgoing.push_back(Outgoing {
            id,
            body,
            deadline: self.now.saturating_add(ttl_ms),
            next_at: self.now,
            interval: EVENT_RETRY_MS,
            sent: false,
        });
        self.reliability.queued += 1;
        Ok(id)
    }
    fn reliable_frames(&mut self, now: u64, output: &mut Vec<ClientFrame>) {
        let before = self.outgoing.len();
        self.outgoing.retain(|e| now < e.deadline);
        self.reliability.expired += (before - self.outgoing.len()) as u64;
        while let Some(at) = self.pings.front().map(|(_, at)| *at) {
            if now.saturating_sub(at) < PING_LOSS_MS {
                break;
            }
            self.pings.pop_front();
            self.reliability.pings_lost += 1;
        }
        let Some(credentials) = self.credentials.clone().filter(|_| self.connected()) else {
            return;
        };
        for event in self.outgoing.iter_mut().filter(|e| e.next_at <= now) {
            output.push(ClientFrame::new(C::Event {
                credentials: credentials.clone(),
                stream: self.stream,
                id: event.id,
                ttl_ms: event.deadline - now,
                body: event.body,
            }));
            if event.sent {
                self.reliability.retransmitted += 1;
            } else {
                event.sent = true;
                self.reliability.sent += 1;
            }
            event.next_at = now.saturating_add(event.interval);
            event.interval = (event.interval * 2).min(EVENT_RETRY_MAX_MS);
        }
        for event in self.relay_acks.drain(..) {
            output.push(ClientFrame::new(C::RelayAck {
                credentials: credentials.clone(),
                event,
            }));
        }
    }
    pub fn connected(&self) -> bool {
        self.status.phase == Phase::Connected
    }
    /// Explicit resume request, also used by the native diagnostic client.
    pub fn request_reconnect(&mut self) -> Vec<Event> {
        if !self.connected() {
            return Vec::new();
        }
        self.reconnect();
        vec![Event::Reset]
    }
    fn reset(&mut self) {
        self.tracks.clear();
        self.roster = None;
        self.revision = 0;
        self.pings.clear();
        self.ping_at = None;
    }
    fn reconnect(&mut self) {
        self.status.phase = Phase::Reconnecting;
        self.status.reconnects += 1;
        self.request = self.request.wrapping_add(1).max(1);
        self.hello_at = None;
        self.reset();
    }
    /// Runs independently of coordinate availability, so menus keep heartbeats.
    pub fn poll(&mut self, now: u64) -> (Vec<ClientFrame>, Vec<Event>) {
        self.now = self.now.max(now);
        let now = self.now;
        let mut events = Vec::new();
        if self.connected() && now.saturating_sub(self.heard_at) >= self.timeout_ms {
            self.reconnect();
            events.push(Event::Reset);
        }
        let mut output = Vec::new();
        match self.status.phase {
            Phase::Joining | Phase::Reconnecting => {
                if self
                    .hello_at
                    .is_none_or(|at| now.saturating_sub(at) >= wire::RETRY_MS)
                {
                    self.hello_at = Some(now);
                    output.push(ClientFrame::new(C::Join {
                        request: self.request,
                        room: self.options.room.clone(),
                        identity: self.options.identity.clone(),
                        profile: self.options.profile.clone(),
                        resume: self.credentials.clone(),
                        admission: self.options.admission.clone(),
                    }));
                }
            }
            Phase::Connected => {
                if self
                    .ping_at
                    .is_none_or(|at| now.saturating_sub(at) >= wire::HEARTBEAT_MS)
                {
                    self.ping_sequence = self.ping_sequence.wrapping_add(1).max(1);
                    self.ping_at = Some(now);
                    if self.pings.len() == 8 {
                        self.pings.pop_front();
                        self.reliability.pings_lost += 1;
                    }
                    self.pings.push_back((self.ping_sequence, now));
                    self.reliability.pings_sent += 1;
                    output.push(ClientFrame::new(C::Ping {
                        credentials: self.credentials.clone().unwrap(),
                        sequence: self.ping_sequence,
                        client_ms: now,
                    }));
                }
            }
            Phase::Closing => {
                if now.saturating_sub(self.closing_at) >= 200 {
                    self.status.phase = Phase::Closed;
                    self.credentials = None;
                } else if self.leave_at.is_none_or(|at| now.saturating_sub(at) >= 50) {
                    self.leave_at = Some(now);
                    if let Some(credentials) = &self.credentials {
                        output.push(ClientFrame::new(C::Leave {
                            credentials: credentials.clone(),
                        }));
                    }
                }
            }
            Phase::Rejected | Phase::Closed => {}
        }
        self.reliable_frames(now, &mut output);
        (output, events)
    }
    pub fn pose(
        &mut self,
        position: Position,
        rotation: Option<Rotation>,
        now: u64,
    ) -> Option<ClientFrame> {
        if !self.connected() || !position.is_valid() || !rotation.is_none_or(Rotation::is_valid) {
            return None;
        }
        let sequence = self.pose_sequence;
        self.pose_sequence = self.pose_sequence.checked_add(1)?;
        Some(ClientFrame::new(C::Pose {
            credentials: self.credentials.clone()?,
            sequence,
            client_ms: now,
            position,
            rotation,
        }))
    }
    pub fn close(&mut self, now: u64) {
        self.now = self.now.max(now);
        self.closing_at = self.now;
        self.leave_at = None;
        self.status.phase = if self.credentials.is_some() {
            Phase::Closing
        } else {
            Phase::Closed
        };
        self.reset();
    }
    fn current(&self, session: u64, connection: u64) -> bool {
        self.credentials
            .as_ref()
            .is_some_and(|c| c.session == session && c.connection == connection)
    }
    pub fn ingest(&mut self, bytes: &[u8], now: u64) -> Vec<Event> {
        self.now = self.now.max(now);
        let now = self.now;
        let Some(frame) = wire::decode_server(bytes) else {
            self.status.invalid += 1;
            return Vec::new();
        };
        match frame.message {
            S::Accepted {
                request,
                credentials,
                player,
                revision,
                timeout_ms,
                ..
            } if matches!(self.status.phase, Phase::Joining | Phase::Reconnecting)
                && request == self.request =>
            {
                self.reset();
                if credentials.session != self.seen_session {
                    // A new room session restarts relay sequences.
                    self.seen = Default::default();
                    self.relay_acks.clear();
                    self.seen_session = credentials.session;
                }
                self.credentials = Some(credentials.clone());
                self.revision = revision;
                self.pose_sequence = 0;
                self.ping_sequence = 0;
                self.timeout_ms = timeout_ms;
                self.heard_at = now;
                self.status.phase = Phase::Connected;
                self.status.session = credentials.session;
                self.status.connection = credentials.connection;
                self.status.player = player;
                self.status.accepted += 1;
                self.status.refusal = None;
                self.status.rtt_ms = None;
                self.status.best_rtt_ms = None;
                self.status.clock_offset_ms = None;
                self.timing = Recorder::default();
                vec![Event::Reset]
            }
            S::Rejected {
                request, reason, ..
            } if matches!(self.status.phase, Phase::Joining | Phase::Reconnecting)
                && request == self.request =>
            {
                self.status.refusal = Some(reason);
                if matches!(
                    reason,
                    Refusal::Full
                        | Refusal::IdentityInUse
                        | Refusal::EndpointInUse
                        | Refusal::Closing
                ) {
                    self.hello_at = Some(now.saturating_add(1500));
                } else {
                    self.status.phase = Phase::Rejected;
                    self.reset();
                    self.credentials = None;
                }
                vec![Event::Reset]
            }
            S::Pong {
                session,
                connection,
                sequence,
                client_ms,
                server_ms,
            } if self.connected() && self.current(session, connection) => {
                let Some(index) = self
                    .pings
                    .iter()
                    .position(|(s, at)| *s == sequence && *at == client_ms)
                else {
                    self.status.old += 1;
                    return Vec::new();
                };
                if client_ms > now {
                    self.status.invalid += 1;
                    return Vec::new();
                }
                let rtt = now - client_ms;
                let offset = i128::from(server_ms) - (i128::from(client_ms) + i128::from(now)) / 2;
                let Ok(offset) = i64::try_from(offset) else {
                    self.status.invalid += 1;
                    return Vec::new();
                };
                self.pings.remove(index);
                self.reliability.pongs += 1;
                self.heard_at = now;
                self.status.rtt_ms = Some(rtt);
                self.timing.rtt(rtt);
                if self.status.best_rtt_ms.is_none_or(|best| rtt <= best) {
                    self.status.best_rtt_ms = Some(rtt);
                    self.status.clock_offset_ms = Some(offset);
                }
                Vec::new()
            }
            S::Roster {
                session,
                revision,
                players,
            } if self.connected()
                && session == self.status.session
                && revision >= self.revision =>
            {
                self.heard_at = now;
                self.revision = revision;
                self.tracks.retain(|id, _| players.contains(id));
                self.roster = Some((revision, players.clone()));
                vec![Event::Roster(players)]
            }
            S::Snapshot {
                session,
                revision,
                poses,
                tick,
                server_ms,
                ..
            } if self.connected()
                && session == self.status.session
                && revision >= self.revision =>
            {
                self.heard_at = now;
                self.timing.snapshot(tick, server_ms, now);
                self.revision = revision;
                let mut events = Vec::new();
                for pose in poses {
                    if pose.player == self.status.player
                        || self
                            .roster
                            .as_ref()
                            .is_some_and(|(r, ids)| *r == revision && !ids.contains(&pose.player))
                    {
                        self.status.old += 1;
                        continue;
                    }
                    let previous = self.tracks.get(&pose.player);
                    if previous.is_some_and(|(generation, sequence)| {
                        pose.generation < *generation
                            || (pose.generation == *generation && pose.sequence <= *sequence)
                    }) {
                        self.status.old += 1;
                        continue;
                    }
                    if previous.is_none() && self.tracks.len() == wire::MAX_PLAYERS {
                        self.status.invalid += 1;
                        continue;
                    }
                    let reset_history =
                        previous.is_none_or(|(generation, _)| *generation != pose.generation);
                    self.tracks
                        .insert(pose.player, (pose.generation, pose.sequence));
                    events.push(Event::Pose {
                        pose,
                        reset_history,
                    });
                }
                events
            }
            S::EventAck {
                session,
                connection,
                stream,
                id,
            } if self.connected() && self.current(session, connection) && stream == self.stream => {
                self.heard_at = now;
                let before = self.outgoing.len();
                self.outgoing.retain(|e| e.id != id);
                if self.outgoing.len() < before {
                    self.reliability.acked += 1;
                } else {
                    self.status.old += 1;
                }
                Vec::new()
            }
            S::Relay {
                session,
                event,
                origin,
                server_ms,
                body,
            } if self.connected() && session == self.status.session => {
                self.heard_at = now;
                // Acknowledge every copy: the first ack may have been lost.
                if self.relay_acks.len() == RELAY_WINDOW {
                    self.relay_acks.pop_front();
                }
                self.relay_acks.push_back(event);
                if !self.seen.admit(event) {
                    self.reliability.relay_duplicates += 1;
                    return Vec::new();
                }
                self.reliability.relays += 1;
                vec![Event::Relay {
                    origin,
                    event,
                    server_ms,
                    body,
                }]
            }
            S::Closing {
                session,
                connection,
            } if self.connected() && self.current(session, connection) => {
                self.reconnect();
                self.hello_at = Some(now.saturating_add(1500));
                vec![Event::Reset]
            }
            S::Left {
                session,
                connection,
            } if self.status.phase == Phase::Closing && self.current(session, connection) => {
                self.status.phase = Phase::Closed;
                self.status.leave_confirmed = true;
                self.credentials = None;
                Vec::new()
            }
            _ => {
                self.status.old += 1;
                Vec::new()
            }
        }
    }
}

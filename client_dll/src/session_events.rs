//! Reliable session events at the DLL boundary: local state publication with
//! debounce, queued diagnostic markers and what other players relayed. Pure
//! data; the worker owns the session client and the clock.
use multiplayer_server::{
    session_client::Reliability,
    session_protocol::{EventBody, PlayerState},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// A state must hold this long before it is published (menus flicker).
pub const STATE_DEBOUNCE_MS: u64 = 300;
pub const STATE_TTL_MS: u64 = 10_000;
/// Refresh inside the TTL so expired updates recover even in the same room.
pub const STATE_REFRESH_MS: u64 = 3000;
pub const MARKER_TTL_MS: u64 = 30_000;
/// Markers waiting for the worker; the Control call refuses beyond this.
pub const MARKER_QUEUE: usize = 16;
const RECENT_MARKERS: usize = 8;
/// Control v1 message buffer minus the terminator.
pub const REPORT_BYTES: usize = 511;
/// Counters are saturated here in the report so the JSON always fits.
pub const REPORT_MAX: u64 = 999_999_999;
fn saturated(r: Reliability) -> Reliability {
    let c = |v: u64| v.min(REPORT_MAX);
    Reliability {
        superseded: c(r.superseded),
        queued: c(r.queued),
        sent: c(r.sent),
        retransmitted: c(r.retransmitted),
        acked: c(r.acked),
        expired: c(r.expired),
        refused: c(r.refused),
        pending: c(r.pending),
        relays: c(r.relays),
        relay_duplicates: c(r.relay_duplicates),
        pings_sent: c(r.pings_sent),
        pongs: c(r.pongs),
        pings_lost: c(r.pings_lost),
    }
}
/// A remote that announced `loading`/`unavailable` is hidden once its poses
/// have been quiet this long, instead of freezing until the 1 s expiry.
pub const QUIET_MS: u64 = 150;

/// Render rule. Poses still arriving keep the truck visible whatever the state
/// (the state is debounced and may lag the stream by a few hundred ms).
pub fn hide_remote(state: Option<PlayerState>, since_pose: std::time::Duration) -> bool {
    matches!(state, Some(PlayerState::Loading | PlayerState::Unavailable))
        && since_pose >= std::time::Duration::from_millis(QUIET_MS)
}

/// Local state as far as the client can actually tell. Loading wins over the
/// sample: the gate hides the visual before the scene is replaced.
pub fn local_state(loading: bool, sample_available: bool) -> PlayerState {
    if loading {
        PlayerState::Loading
    } else if sample_available {
        PlayerState::Driving
    } else {
        PlayerState::Unavailable
    }
}

/// Debounced publication, repeated once per room session (a restarted server
/// forgets the latest state of each identity).
#[derive(Default)]
pub struct Publisher {
    candidate: Option<(PlayerState, u64)>,
    published: Option<(PlayerState, u64, u64)>,
}
impl Publisher {
    pub fn observe(&mut self, state: PlayerState, now: u64, session: u64) -> Option<PlayerState> {
        if session == 0 {
            return None;
        }
        match self.candidate {
            Some((current, _)) if current == state => {}
            _ => self.candidate = Some((state, now)),
        }
        let (current, since) = self.candidate?;
        if self.published.is_some_and(|(s, epoch, at)| {
            s == current && epoch == session && now.saturating_sub(at) < STATE_REFRESH_MS
        }) || now.saturating_sub(since) < STATE_DEBOUNCE_MS
        {
            return None;
        }
        self.published = Some((current, session, now));
        Some(current)
    }
    /// The queue refused the event: try again on a later observation.
    pub fn retry(&mut self) {
        self.published = None;
    }
    pub fn published(&self) -> Option<PlayerState> {
        self.published.map(|(s, _, _)| s)
    }
}

/// SESSION_EVENTS payload; strict so offline reports carry only typed fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EventsReport {
    pub local: Option<PlayerState>,
    pub reliability: Reliability,
    pub pending_markers: usize,
    pub markers_refused: u64,
    pub states: Vec<(u32, PlayerState)>,
    /// [origin, code, age_ms], newest last.
    pub markers: Vec<(u32, u32, u64)>,
    pub truncated: bool,
    /// Worker passes without sampling while the command gate was busy (absent in
    /// older reports).
    #[serde(default)]
    pub loading_skipped: u64,
}
impl EventsReport {
    pub fn validate(&self) -> Result<(), String> {
        let r = &self.reliability;
        if self.pending_markers > MARKER_QUEUE
            || self.states.len() > multiplayer_server::session_protocol::MAX_PLAYERS * 2
            || self.markers.len() > RECENT_MARKERS
            || r.pending > multiplayer_server::session_client::EVENT_QUEUE as u64
            || r.acked > r.sent
            || r.pongs > r.pings_sent
        {
            return Err("invalid session events report".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Marker {
    pub origin: u32,
    pub code: u32,
    pub event: u64,
    /// Local worker milliseconds at receipt.
    pub received_ms: u64,
}
/// What the room relayed. States are latest-wins by room sequence.
#[derive(Default)]
pub struct Board {
    pub reliability: Reliability,
    pub local: Option<PlayerState>,
    states: BTreeMap<u32, (u64, PlayerState)>,
    markers: VecDeque<Marker>,
    pending: VecDeque<u32>,
    pub markers_refused: u64,
    pub loading_skipped: u64,
}
impl Board {
    pub fn relay(&mut self, origin: u32, event: u64, body: EventBody, now: u64) {
        match body {
            EventBody::State { state } => {
                if !self.states.contains_key(&origin)
                    && self.states.len() >= multiplayer_server::session_protocol::MAX_PLAYERS * 2
                {
                    return;
                }
                if self
                    .states
                    .get(&origin)
                    .is_none_or(|(seen, _)| event > *seen)
                {
                    self.states.insert(origin, (event, state));
                }
            }
            EventBody::Marker { code } => {
                if self.markers.len() == RECENT_MARKERS {
                    self.markers.pop_front();
                }
                self.markers.push_back(Marker {
                    origin,
                    code,
                    event,
                    received_ms: now,
                });
            }
        }
    }
    /// Players no longer listed lose their state; markers stay as a log.
    pub fn roster(&mut self, players: &[u32]) {
        self.states.retain(|id, _| players.contains(id));
    }
    /// Reconnection or new session: remote states are no longer current.
    pub fn reset(&mut self) {
        self.states.clear();
    }
    pub fn state_of(&self, player: u32) -> Option<PlayerState> {
        self.states.get(&player).map(|(_, s)| *s)
    }
    pub fn queue_marker(&mut self, code: u32) -> Result<(), String> {
        if self.pending.len() == MARKER_QUEUE {
            self.markers_refused = self.markers_refused.saturating_add(1);
            return Err("marker queue full".into());
        }
        self.pending.push_back(code);
        Ok(())
    }
    pub fn take_markers(&mut self) -> Vec<u32> {
        self.pending.drain(..).collect()
    }
    /// Compact JSON for Control v1; trims the oldest lists entries to fit.
    pub fn report(&self, now: u64) -> String {
        let mut report = EventsReport {
            local: self.local,
            reliability: saturated(self.reliability),
            pending_markers: self.pending.len(),
            markers_refused: self.markers_refused.min(REPORT_MAX),
            states: self.states.iter().map(|(id, (_, s))| (*id, *s)).collect(),
            markers: self
                .markers
                .iter()
                .map(|m| (m.origin, m.code, now.saturating_sub(m.received_ms)))
                .collect(),
            truncated: saturated(self.reliability) != self.reliability
                || self.markers_refused > REPORT_MAX
                || self.loading_skipped > REPORT_MAX,
            loading_skipped: self.loading_skipped.min(REPORT_MAX),
        };
        loop {
            let json = serde_json::to_string(&report).unwrap_or_default();
            if json.len() <= REPORT_BYTES {
                return json;
            }
            report.truncated = true;
            if !report.markers.is_empty() {
                report.markers.remove(0);
            } else if report.states.pop().is_none() {
                return json; // Counters alone always fit; defensive exit.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn periodic_state_refresh_recovers_after_expiry_or_a_same_room_reconnection() {
        let mut p = Publisher::default();
        assert_eq!(p.observe(PlayerState::Loading, 0, 7), None);
        assert_eq!(
            p.observe(PlayerState::Loading, 300, 7),
            Some(PlayerState::Loading)
        );
        assert_eq!(
            p.observe(PlayerState::Loading, 300 + STATE_REFRESH_MS - 1, 7),
            None
        );
        assert_eq!(
            p.observe(PlayerState::Loading, 300 + STATE_REFRESH_MS, 7),
            Some(PlayerState::Loading)
        );
        assert_eq!(p.observe(PlayerState::Loading, 30_000, 0), None);
        assert_eq!(
            p.observe(PlayerState::Loading, 30_001, 7),
            Some(PlayerState::Loading)
        );
    }
    #[test]
    fn local_state_prefers_loading_and_publishes_only_stable_changes_per_session() {
        assert_eq!(local_state(true, true), PlayerState::Loading);
        assert_eq!(local_state(false, true), PlayerState::Driving);
        assert_eq!(local_state(false, false), PlayerState::Unavailable);
        let mut p = Publisher::default();
        assert_eq!(p.observe(PlayerState::Driving, 0, 0), None);
        assert_eq!(p.observe(PlayerState::Driving, 0, 7), None);
        assert_eq!(p.observe(PlayerState::Driving, 299, 7), None);
        assert_eq!(
            p.observe(PlayerState::Driving, 300, 7),
            Some(PlayerState::Driving)
        );
        assert_eq!(p.observe(PlayerState::Driving, 900, 7), None);
        // A flicker shorter than the debounce is never published.
        assert_eq!(p.observe(PlayerState::Unavailable, 1000, 7), None);
        assert_eq!(p.observe(PlayerState::Driving, 1100, 7), None);
        assert_eq!(p.observe(PlayerState::Driving, 2000, 7), None);
        assert_eq!(p.observe(PlayerState::Loading, 2100, 7), None);
        assert_eq!(
            p.observe(PlayerState::Loading, 2400, 7),
            Some(PlayerState::Loading)
        );
        // A new room session republishes the unchanged state.
        assert_eq!(
            p.observe(PlayerState::Loading, 2500, 8),
            Some(PlayerState::Loading)
        );
        p.retry();
        assert_eq!(
            p.observe(PlayerState::Loading, 2600, 8),
            Some(PlayerState::Loading)
        );
        assert_eq!(p.published(), Some(PlayerState::Loading));
    }
    #[test]
    fn board_orders_states_keeps_recent_markers_and_report_fits_control_v1() {
        let mut b = Board::default();
        b.relay(
            2,
            5,
            EventBody::State {
                state: PlayerState::Loading,
            },
            0,
        );
        b.relay(
            2,
            4,
            EventBody::State {
                state: PlayerState::Driving,
            },
            0,
        );
        assert_eq!(b.state_of(2), Some(PlayerState::Loading));
        b.relay(
            2,
            9,
            EventBody::State {
                state: PlayerState::Driving,
            },
            0,
        );
        assert_eq!(b.state_of(2), Some(PlayerState::Driving));
        b.roster(&[3]);
        assert_eq!(b.state_of(2), None);
        for i in 0..64u32 {
            b.relay(
                1_000_000_000 + i,
                100 + u64::from(i),
                EventBody::State {
                    state: PlayerState::Unavailable,
                },
                0,
            );
            b.relay(
                u32::MAX - i,
                1000 + u64::from(i),
                EventBody::Marker { code: u32::MAX },
                1,
            );
        }
        b.reliability = Reliability {
            superseded: u64::MAX,
            queued: u64::MAX,
            sent: u64::MAX,
            retransmitted: u64::MAX,
            acked: u64::MAX,
            expired: u64::MAX,
            refused: u64::MAX,
            pending: u64::MAX,
            relays: u64::MAX,
            relay_duplicates: u64::MAX,
            pings_sent: u64::MAX,
            pongs: u64::MAX,
            pings_lost: u64::MAX,
        };
        b.local = Some(PlayerState::Unavailable);
        b.loading_skipped = u64::MAX;
        b.markers_refused = u64::MAX;
        let report = b.report(u64::MAX);
        assert!(report.len() <= REPORT_BYTES, "{}", report.len());
        let value: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(value["truncated"], true);
        assert_eq!(value["reliability"]["pings_lost"], REPORT_MAX);
        let small = Board::default().report(0);
        assert!(small.contains("\"truncated\":false"));
        for code in 0..MARKER_QUEUE as u32 {
            b.queue_marker(code).unwrap();
        }
        assert!(b.queue_marker(99).is_err());
        assert_eq!(b.take_markers().len(), MARKER_QUEUE);
        assert!(b.take_markers().is_empty());
        b.reset();
        assert!(b.report(0).contains("\"states\":[]"));
    }
}
#[cfg(test)]
mod render_rule_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn only_quiet_loading_or_unavailable_remotes_are_hidden() {
        let quiet = Duration::from_millis(QUIET_MS);
        let fresh = Duration::from_millis(QUIET_MS - 1);
        assert!(hide_remote(Some(PlayerState::Loading), quiet));
        assert!(hide_remote(Some(PlayerState::Unavailable), quiet * 3));
        assert!(
            !hide_remote(Some(PlayerState::Loading), fresh),
            "poses still flowing"
        );
        assert!(!hide_remote(Some(PlayerState::Driving), quiet * 10));
        assert!(
            !hide_remote(None, quiet * 10),
            "no state: keep the normal expiry"
        );
        let old: EventsReport = serde_json::from_str(
            r#"{"local":null,"reliability":{"queued":0,"sent":0,"retransmitted":0,"acked":0,"expired":0,"refused":0,"pending":0,"relays":0,"relay_duplicates":0,"pings_sent":0,"pongs":0,"pings_lost":0},"pending_markers":0,"markers_refused":0,"states":[],"markers":[],"truncated":false}"#,
        )
        .unwrap();
        assert_eq!(old.loading_skipped, 0);
    }
}

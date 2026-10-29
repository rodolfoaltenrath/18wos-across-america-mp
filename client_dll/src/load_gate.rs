//! Decision and status contract of the ACTIVE map gate (commands 54..57).
//!
//! A `map` request that arrives while the test visual is allocated is copied
//! and swallowed; the outer loop hides, releases, replays the original callback
//! exactly once with own argc/argv words, then recreates the visual in the new
//! world. Hooks live in `engine_load_gate`; this module holds no addresses.
use crate::{api::EngineControl, deferred_load::Phase};
use serde::Serialize;

/// Default deadline from admission to recreation, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u32 = 120_000;
const SCHEMA: u32 = 0x4c47_5431;

/// What the descriptor hook does with one `map` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// Call the original now (no retained visual, foreign thread or no-op).
    Forward(Note),
    /// Copy arguments and admit a deferred job; the original is not called.
    Defer,
    /// Do not call the original; the request is not admitted.
    Swallow(Note),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum Note {
    #[default]
    None = 0,
    /// Not the owner (window/loop) thread: the gate cannot sequence it.
    Foreign = 1,
    /// Nested call during the gate's own replay: belongs to that load.
    Replay = 2,
    /// Another deferred load is still active: duplicate request dropped.
    Busy = 3,
    /// argc < 2: the handler's map branch returns without loading.
    NoArguments = 4,
    /// Nothing retained by the mod: the original load is safe as is.
    NoVisual = 5,
    /// Arguments unreadable/out of bounds: visual released, load dropped.
    CaptureFailed = 6,
    /// No valid world witness (actor/scene/epoch): visual released, load dropped.
    InvalidWorld = 7,
    /// Gate refused admission: visual released, load dropped.
    AdmitFailed = 8,
}
impl Note {
    pub(crate) fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Foreign,
            2 => Self::Replay,
            3 => Self::Busy,
            4 => Self::NoArguments,
            5 => Self::NoVisual,
            6 => Self::CaptureFailed,
            7 => Self::InvalidWorld,
            8 => Self::AdmitFailed,
            _ => Self::None,
        }
    }
}
/// Inputs sampled by the hook before deciding. Never reads argv[1..] here.
#[derive(Clone, Copy, Debug, Default)]
pub struct EntryState {
    pub owner_thread: bool,
    pub replaying: bool,
    /// A non-terminal job exists (or its state could not be sampled).
    pub job_active: bool,
    pub argc: u32,
    pub visual_allocated: bool,
}
/// Order matters: a replay-nested call is forwarded even while the job is
/// active, and an active job swallows duplicates even without a visual (its
/// pending execute would otherwise load twice).
pub fn decide(s: EntryState) -> Entry {
    if !s.owner_thread {
        return Entry::Forward(Note::Foreign);
    }
    if s.replaying {
        return Entry::Forward(Note::Replay);
    }
    if s.job_active {
        return Entry::Swallow(Note::Busy);
    }
    if s.argc < 2 {
        return Entry::Forward(Note::NoArguments);
    }
    if !s.visual_allocated {
        return Entry::Forward(Note::NoVisual);
    }
    Entry::Defer
}
pub(crate) fn phase_code(p: Phase) -> u32 {
    match p {
        Phase::Idle => 0,
        Phase::Hide => 1,
        Phase::Check => 2,
        Phase::Release => 3,
        Phase::Execute => 4,
        Phase::AwaitWorld => 5,
        Phase::Recreate => 6,
        Phase::Complete => 7,
        Phase::Recoverable => 8,
        Phase::Quarantined => 9,
    }
}
pub(crate) fn phase_from(v: u32) -> Option<Phase> {
    Some(match v {
        0 => Phase::Idle,
        1 => Phase::Hide,
        2 => Phase::Check,
        3 => Phase::Release,
        4 => Phase::Execute,
        5 => Phase::AwaitWorld,
        6 => Phase::Recreate,
        7 => Phase::Complete,
        8 => Phase::Recoverable,
        9 => Phase::Quarantined,
        _ => return None,
    })
}
/// Gate reasons travel as indices; unknown text maps to "other".
pub const REASONS: [&str; 10] = [
    "",
    "deadline_expired",
    "unexpected_world_change",
    "operation_outcome_unknown",
    "invalid_release_context",
    "release_refused",
    "release_refused_after_check",
    "callback_not_invoked",
    "cancel_requested",
    "other",
];
pub fn reason_code(reason: Option<&str>) -> u32 {
    match reason {
        None => 0,
        Some(r) => REASONS
            .iter()
            .position(|k| *k == r)
            .unwrap_or(REASONS.len() - 1) as u32,
    }
}
/// Remote-readable snapshot published by the hook/loop as atomics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub installed: bool,
    pub fixture: bool,
    pub owner_thread: u32,
    pub timeout_ms: u32,
    pub phase: Option<Phase>,
    pub job: u32,
    pub in_flight: bool,
    pub execute_started: bool,
    pub release_confirmed: bool,
    pub reason: &'static str,
    /// Calls that reached the hook, by outcome.
    pub calls: u32,
    pub deferred: u32,
    pub forwarded: u32,
    pub foreign: u32,
    pub replay_nested: u32,
    pub swallowed_busy: u32,
    pub dropped: u32,
    pub last_note: Note,
    /// Originals invoked by the loop replay and their raw EAX.
    pub replays: u32,
    pub last_return: u32,
    /// Replays whose own argc/argv words were still published and restored.
    pub arguments_restored: u32,
    /// Replays whose words were changed by another writer; frame kept alive.
    pub arguments_retained: u32,
    pub recreated: u32,
    /// Releases without a load after a dropped request (capture/world/admit).
    pub fallback_releases: u32,
    pub fallback_pending: bool,
    /// Last world witness used by the gate (draw-observed actor and epoch).
    pub actor: u32,
    pub scene: u32,
    pub epoch: u32,
    /// A deferred load still pending/complete never proves a SUCCESSFUL load.
    pub automatic_load_supported: bool,
}
impl Status {
    pub fn encode(&self, b: &mut EngineControl) {
        let f = &mut b.frames;
        f[0] = SCHEMA;
        f[1] = u32::from(self.installed)
            | u32::from(self.fixture) << 1
            | u32::from(self.in_flight) << 2
            | u32::from(self.execute_started) << 3
            | u32::from(self.release_confirmed) << 4
            | u32::from(self.fallback_pending) << 5
            | u32::from(self.automatic_load_supported) << 6;
        f[2] = self.owner_thread;
        f[3] = self.timeout_ms;
        f[4] = self.phase.map_or(u32::MAX, phase_code);
        f[5] = self.job;
        f[6] = reason_code(Some(self.reason).filter(|r| !r.is_empty()));
        f[7] = self.calls;
        f[8] = self.deferred;
        f[9] = self.forwarded;
        f[10] = self.foreign;
        f[11] = self.replay_nested;
        f[12] = self.swallowed_busy;
        f[13] = self.dropped;
        f[14] = self.last_note as u32;
        f[15] = self.replays;
        f[16] = self.last_return;
        f[17] = self.arguments_restored;
        f[18] = self.arguments_retained;
        f[19] = self.recreated;
        f[20] = self.fallback_releases;
        f[21] = self.actor;
        f[22] = self.scene;
        f[23] = self.epoch;
    }
    pub fn decode(b: &EngineControl) -> Result<Self, String> {
        let f = &b.frames;
        if f[0] != SCHEMA {
            return Err("load gate status schema mismatch".into());
        }
        let bit = |n: u32| f[1] & (1 << n) != 0;
        Ok(Self {
            installed: bit(0),
            fixture: bit(1),
            in_flight: bit(2),
            execute_started: bit(3),
            release_confirmed: bit(4),
            fallback_pending: bit(5),
            automatic_load_supported: bit(6),
            owner_thread: f[2],
            timeout_ms: f[3],
            phase: phase_from(f[4]),
            job: f[5],
            reason: REASONS.get(f[6] as usize).copied().unwrap_or("other"),
            calls: f[7],
            deferred: f[8],
            forwarded: f[9],
            foreign: f[10],
            replay_nested: f[11],
            swallowed_busy: f[12],
            dropped: f[13],
            last_note: Note::from_u32(f[14]),
            replays: f[15],
            last_return: f[16],
            arguments_restored: f[17],
            arguments_retained: f[18],
            recreated: f[19],
            fallback_releases: f[20],
            actor: f[21],
            scene: f[22],
            epoch: f[23],
        })
    }
    /// Idle or terminal: nothing will act without a new request.
    pub fn quiescent(&self) -> bool {
        !self.in_flight
            && !self.fallback_pending
            && matches!(
                self.phase,
                Some(Phase::Idle | Phase::Complete | Phase::Recoverable | Phase::Quarantined)
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> EntryState {
        EntryState {
            owner_thread: true,
            replaying: false,
            job_active: false,
            argc: 2,
            visual_allocated: true,
        }
    }
    #[test]
    fn defers_only_owner_calls_with_arguments_while_the_visual_is_retained() {
        assert_eq!(decide(state()), Entry::Defer);
        for (s, expected) in [
            (
                EntryState {
                    owner_thread: false,
                    ..state()
                },
                Entry::Forward(Note::Foreign),
            ),
            (
                EntryState {
                    replaying: true,
                    job_active: true,
                    ..state()
                },
                Entry::Forward(Note::Replay),
            ),
            (
                EntryState {
                    job_active: true,
                    visual_allocated: false,
                    ..state()
                },
                Entry::Swallow(Note::Busy),
            ),
            (
                EntryState { argc: 1, ..state() },
                Entry::Forward(Note::NoArguments),
            ),
            (
                EntryState {
                    visual_allocated: false,
                    ..state()
                },
                Entry::Forward(Note::NoVisual),
            ),
        ] {
            assert_eq!(decide(s), expected, "{s:?}");
        }
    }
    #[test]
    fn status_round_trips_without_arguments_and_reports_quiescence() {
        let s = Status {
            installed: true,
            fixture: true,
            owner_thread: 432,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            phase: Some(Phase::AwaitWorld),
            job: 3,
            in_flight: false,
            execute_started: true,
            release_confirmed: true,
            reason: "deadline_expired",
            calls: 9,
            deferred: 2,
            forwarded: 3,
            foreign: 1,
            replay_nested: 1,
            swallowed_busy: 1,
            dropped: 1,
            last_note: Note::CaptureFailed,
            replays: 2,
            last_return: 0xfedc_ba98,
            arguments_restored: 2,
            arguments_retained: 0,
            recreated: 1,
            fallback_releases: 1,
            fallback_pending: false,
            actor: 0x1234,
            scene: 0x5678,
            epoch: 4,
            automatic_load_supported: false,
        };
        let mut b = EngineControl::new(57);
        s.encode(&mut b);
        assert_eq!(Status::decode(&b).unwrap(), s);
        assert!(!s.quiescent());
        for (phase, quiet) in [
            (Phase::Idle, true),
            (Phase::Complete, true),
            (Phase::Recoverable, true),
            (Phase::Quarantined, true),
            (Phase::Check, false),
        ] {
            assert_eq!(
                Status {
                    phase: Some(phase),
                    ..s
                }
                .quiescent(),
                quiet
            );
        }
        assert!(
            !Status {
                phase: Some(Phase::Idle),
                fallback_pending: true,
                ..s
            }
            .quiescent()
        );
        assert_eq!(reason_code(Some("never seen")), 9);
        assert_eq!(reason_code(None), 0);
        b.frames[0] = 0;
        assert!(Status::decode(&b).is_err());
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"phase\":\"await_world\""));
        assert!(!json.contains(".sav"));
    }
}

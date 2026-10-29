//! Decision and status contract of the ACTIVE world-command gate (54..57).
//!
//! The six commands whose shared game handler deletes the scene (`map`,
//! `preview`, `edit`, `exit`, `menu`, `quit`) are deferred while the test
//! visual is allocated: the request is copied and swallowed; the outer loop
//! hides, releases, replays the original callback exactly once with own
//! argc/argv words, then recreates the visual once a player world is drawn.
//! Hooks live in `engine_load_gate`; this module holds no addresses.
use crate::{api::EngineControl, deferred_load::Phase};
use serde::Serialize;

/// Default deadline from admission to recreation, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u32 = 120_000;
const SCHEMA: u32 = 0x4c47_5431;

/// What the descriptor hook does with one world-command call.
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
    /// argv[0] unreadable or not a gated command: forwarded unchanged.
    Unclassified = 9,
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
            9 => Self::Unclassified,
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
    /// Classified from argv[0], which cmd_execute publishes before the call.
    pub command: Option<WorldCommand>,
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
    let Some(command) = s.command else {
        return Entry::Forward(Note::Unclassified);
    };
    if s.argc < command.minimum_argc() {
        return Entry::Forward(Note::NoArguments);
    }
    if !s.visual_allocated {
        return Entry::Forward(Note::NoVisual);
    }
    Entry::Defer
}
/// Commands of the shared handler `game+0xd3570` that delete the scene
/// (`this+0x14c` through `0xcc4a0` and the deleting destructor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum WorldCommand {
    Map = 1,
    Preview = 2,
    Edit = 3,
    Exit = 4,
    Menu = 5,
    Quit = 6,
}
pub const WORLD_COMMANDS: [WorldCommand; 6] = [
    WorldCommand::Map,
    WorldCommand::Preview,
    WorldCommand::Edit,
    WorldCommand::Exit,
    WorldCommand::Menu,
    WorldCommand::Quit,
];
impl WorldCommand {
    pub fn name(self) -> &'static str {
        match self {
            Self::Map => "map",
            Self::Preview => "preview",
            Self::Edit => "edit",
            Self::Exit => "exit",
            Self::Menu => "menu",
            Self::Quit => "quit",
        }
    }
    pub fn classify(name: &[u8]) -> Option<Self> {
        WORLD_COMMANDS
            .into_iter()
            .find(|c| c.name().as_bytes() == name)
    }
    pub(crate) fn from_u32(v: u32) -> Option<Self> {
        WORLD_COMMANDS.into_iter().find(|c| *c as u32 == v)
    }
    /// map/preview return before deleting the scene with argc < 2
    /// (`game+0xd367f`, `+0xd38db`); the others delete it unconditionally.
    pub fn minimum_argc(self) -> u32 {
        match self {
            Self::Map | Self::Preview => 2,
            _ => 1,
        }
    }
    /// Only `map` leads directly to a drawn player world; after the others the
    /// gate completes on return and restores the visual on a later new actor.
    pub fn awaits_player(self) -> bool {
        self == Self::Map
    }
    /// `quit` ends the process: never schedule a restoration after it.
    pub fn restorable(self) -> bool {
        self != Self::Quit
    }
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
    /// A non-player command (exit/menu/edit/preview) released a shown visual:
    /// it is recreated once the draw sees a new player entity.
    pub restore_pending: bool,
    /// Command of the last deferred request.
    pub last_command: Option<WorldCommand>,
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
            | u32::from(self.automatic_load_supported) << 6
            | u32::from(self.restore_pending) << 7;
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
        f[24] = self.last_command.map_or(0, |c| c as u32);
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
            restore_pending: bit(7),
            last_command: WorldCommand::from_u32(f[24]),
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
    /// Idle or terminal: nothing will act without a new request. A pending
    /// restoration only recreates in a later world and does not block.
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
            command: Some(WorldCommand::Map),
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
            (
                EntryState {
                    command: None,
                    ..state()
                },
                Entry::Forward(Note::Unclassified),
            ),
            (
                EntryState {
                    command: Some(WorldCommand::Preview),
                    argc: 1,
                    ..state()
                },
                Entry::Forward(Note::NoArguments),
            ),
        ] {
            assert_eq!(decide(s), expected, "{s:?}");
        }
    }
    #[test]
    fn classifies_the_six_scene_deleting_commands_and_their_argument_rules() {
        for c in WORLD_COMMANDS {
            assert_eq!(WorldCommand::classify(c.name().as_bytes()), Some(c));
            assert_eq!(WorldCommand::from_u32(c as u32), Some(c));
            let s = EntryState {
                command: Some(c),
                argc: 1,
                ..state()
            };
            let expected = if c.minimum_argc() == 2 {
                Entry::Forward(Note::NoArguments)
            } else {
                Entry::Defer
            };
            assert_eq!(decide(s), expected, "{c:?}");
        }
        for ui in [b"game_menu".as_slice(), b"pause", b"MAP", b"map ", b""] {
            assert_eq!(WorldCommand::classify(ui), None);
        }
        assert!(WorldCommand::Map.awaits_player());
        assert!(!WorldCommand::Exit.awaits_player() && WorldCommand::Exit.restorable());
        assert!(!WorldCommand::Quit.restorable());
        assert_eq!(WorldCommand::from_u32(0), None);
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
            restore_pending: true,
            last_command: Some(WorldCommand::Exit),
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

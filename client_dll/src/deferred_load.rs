//! Owned, single-flight load coordination, exercised by fixtures only.
//! No game hooks, engine calls, replay formatting or command deferral ABI here.
use crate::release_policy::{self, Context, Refusal, Report};
use serde::Serialize;
use std::{
    ffi::{CString, c_char},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
pub mod adapter;
pub mod lab;
pub const MAX_ARGUMENTS: usize = 32;
pub const MAX_ARGUMENT_BYTES: usize = 4096;
const MAX_TIMEOUT_MS: u64 = 600_000;
static NEXT_GATE: AtomicU64 = AtomicU64::new(1);

/// Owns exact bytes, including non-UTF-8 arguments. Never serializes arguments.
pub struct OwnedCommand {
    values: Vec<CString>,
    bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::lab::{command, context, report, world};
    use super::*;
    fn step(g: &mut Gate, kind: ActionKind, now: u64) -> Action {
        let a = g.poll(context(), world(1), now).unwrap().unwrap();
        assert_eq!(a.kind, kind);
        a
    }
    fn to_execute(g: &mut Gate) -> u64 {
        let id = g.admit(9, command(), world(1), true, 0, 100).unwrap();
        let a = step(g, ActionKind::Hide, 1);
        g.acknowledge(a.token, Ack::Hidden, 1).unwrap();
        let a = step(g, ActionKind::Check, 2);
        g.acknowledge(a.token, Ack::Checked(report(false)), 2)
            .unwrap();
        let a = step(g, ActionKind::Release, 3);
        g.acknowledge(a.token, Ack::Released, 3).unwrap();
        id
    }
    #[test]
    fn owns_bounded_arguments_and_redacts_diagnostics_after_source_changes() {
        let mut bytes = b"private fixture \"quoted\" \\ save".to_vec();
        let c = OwnedCommand::new(&[b"map", &bytes, &[0xff], b""]).unwrap();
        bytes.fill(b'x');
        assert_eq!(
            c.argument(1),
            Some(b"private fixture \"quoted\" \\ save".as_slice())
        );
        assert_eq!(c.argument(2), Some([0xff].as_slice()));
        assert_eq!(c.argument(3), Some(b"".as_slice()));
        assert!(!format!("{c:?}").contains("private"));
        assert!(OwnedCommand::new(&[b"map", b"bad\0save"]).is_err());
        assert!(OwnedCommand::new(&[b"quit", b"x"]).is_err());
        assert!(OwnedCommand::new(&[b"map"]).is_err());
        assert!(OwnedCommand::new(&vec![b"map".as_slice(); 33]).is_err());
        assert!(OwnedCommand::new(&[b"map", &vec![b'x'; MAX_ARGUMENT_BYTES]]).is_err());
    }
    #[test]
    fn capture_copies_live_argv_with_bounds_and_rejects_missing_terminators() {
        let mut memory = vec![0u8; 5000];
        memory[4..8].copy_from_slice(&64u32.to_le_bytes());
        memory[8..12].copy_from_slice(&80u32.to_le_bytes());
        memory[64..68].copy_from_slice(b"map\0");
        memory[80..86].copy_from_slice(b"saved\0");
        let read = |at: u32, out: &mut [u8]| {
            out.copy_from_slice(
                memory
                    .get(at as usize..at as usize + out.len())
                    .ok_or("unreadable")?,
            );
            Ok(())
        };
        let owned = OwnedCommand::capture(2, 4, read).unwrap();
        memory.fill(b'x');
        assert_eq!(owned.argument(1), Some(b"saved".as_slice()));
        assert_eq!(
            OwnedCommand::capture(33, 4, |_, _| unreachable!()).unwrap_err(),
            Error::InvalidArguments
        );
        assert_eq!(
            OwnedCommand::capture(2, u32::MAX, |_, _| unreachable!()).unwrap_err(),
            Error::InvalidArguments
        );
        assert_eq!(
            OwnedCommand::capture(2, 4, |_, out| {
                out.fill(0);
                Ok(())
            })
            .unwrap_err(),
            Error::UnreadableArguments
        );
        let mut reads = 0;
        let error = OwnedCommand::capture(2, 4, |at, out| {
            reads += 1;
            if at == 4 {
                out.copy_from_slice(&64u32.to_le_bytes());
            } else {
                out.fill(b'x');
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error, Error::InvalidArguments);
        assert_eq!(reads, MAX_ARGUMENT_BYTES + 1);
    }
    #[test]
    fn admits_one_request_and_rejects_stale_cross_gate_and_wrong_acknowledgements() {
        let mut a = Gate::new(9).unwrap();
        a.admit(9, command(), world(1), true, 0, 100).unwrap();
        assert_eq!(
            a.admit(9, command(), world(1), true, 0, 100),
            Err(Error::Busy)
        );
        let token = step(&mut a, ActionKind::Hide, 1).token;
        assert_eq!(
            a.acknowledge(token, Ack::Released, 1),
            Err(Error::WrongAcknowledgement)
        );
        assert!(a.snapshot().in_flight);
        let mut b = Gate::new(9).unwrap();
        b.admit(9, command(), world(1), true, 0, 100).unwrap();
        let own = step(&mut b, ActionKind::Hide, 1).token;
        assert_eq!(
            b.acknowledge(token, Ack::Hidden, 1),
            Err(Error::StaleAction)
        );
        b.acknowledge(own, Ack::Hidden, 1).unwrap();
        assert_eq!(b.acknowledge(own, Ack::Hidden, 1), Err(Error::StaleAction));
    }
    #[test]
    fn validates_owner_outer_phase_clock_and_ticket_exhaustion() {
        let mut g = Gate::new(9).unwrap();
        assert_eq!(
            g.admit(8, command(), world(1), true, 0, 100),
            Err(Error::WrongThread)
        );
        assert_eq!(
            g.admit(9, command(), world(0), true, 0, 100),
            Err(Error::InvalidWorld)
        );
        assert_eq!(
            g.admit(9, command(), world(1), true, u64::MAX, 1),
            Err(Error::InvalidTimeout)
        );
        g.admit(9, command(), world(1), true, 10, 100).unwrap();
        assert!(matches!(
            g.poll(
                Context {
                    pump_depth: 0,
                    ..context()
                },
                world(1),
                10
            ),
            Err(Error::WrongPhase)
        ));
        assert!(matches!(
            g.poll(
                Context {
                    draw_depth: 1,
                    ..context()
                },
                world(1),
                10
            ),
            Err(Error::WrongPhase)
        ));
        assert!(matches!(
            g.poll(context(), world(1), 9),
            Err(Error::ClockReversed)
        ));
        assert!(!g.snapshot().in_flight);
        let mut exhausted = Gate::new(9).unwrap();
        exhausted.next_id = u64::MAX;
        assert_eq!(
            exhausted.admit(9, command(), world(1), true, 0, 10),
            Err(Error::Exhausted)
        );
    }
    #[test]
    fn rechecks_release_evidence_and_does_not_execute_on_a_forged_ready_flag() {
        let mut g = Gate::new(9).unwrap();
        let id = g.admit(9, command(), world(1), true, 0, 100).unwrap();
        let a = step(&mut g, ActionKind::Hide, 1);
        g.acknowledge(a.token, Ack::Hidden, 1).unwrap();
        let a = step(&mut g, ActionKind::Check, 2);
        let mut held = report(true);
        held.ready = true;
        held.refusal = None;
        g.acknowledge(a.token, Ack::Checked(held), 2).unwrap();
        assert_eq!(g.snapshot().phase, Phase::Check);
        let a = step(&mut g, ActionKind::Check, 3);
        let mut fatal = report(false);
        fatal.evidence.counters = None;
        g.acknowledge(a.token, Ack::Checked(fatal), 3).unwrap();
        assert_eq!(g.snapshot().phase, Phase::Recoverable);
        assert!(!g.snapshot().execute_started);
        g.retry_preparation(id, context(), world(1), 4, 100)
            .unwrap();
        assert_eq!(g.snapshot().phase, Phase::Hide);
    }
    #[test]
    fn waits_for_a_ready_new_epoch_then_recreates_even_with_reused_addresses() {
        let mut g = Gate::new(9).unwrap();
        let id = to_execute(&mut g);
        let a = step(&mut g, ActionKind::Execute, 4);
        assert!(a.command().is_some());
        assert!(!serde_json::to_string(&a).unwrap().contains("fixture.sav"));
        g.acknowledge(a.token, Ack::ExecuteReturned, 4).unwrap();
        assert!(
            g.poll(
                context(),
                World {
                    scene: 101,
                    actor: 201,
                    ..world(1)
                },
                5
            )
            .unwrap()
            .is_none()
        );
        assert!(
            g.poll(
                context(),
                World {
                    ready: false,
                    ..world(2)
                },
                6
            )
            .unwrap()
            .is_none()
        );
        let a = g.poll(context(), world(2), 7).unwrap().unwrap();
        assert_eq!(a.kind, ActionKind::Recreate);
        g.acknowledge(a.token, Ack::Recreated, 7).unwrap();
        g.finish(id).unwrap();
        assert_eq!(g.snapshot().phase, Phase::Idle);
    }
    #[test]
    fn callback_rejected_before_invocation_can_retry_but_unknown_outcome_cannot() {
        let mut g = Gate::new(9).unwrap();
        let id = to_execute(&mut g);
        let a = step(&mut g, ActionKind::Execute, 4);
        g.acknowledge(a.token, Ack::NotInvoked, 4).unwrap();
        assert!(!g.snapshot().execute_started);
        g.retry_preparation(id, context(), world(1), 5, 100)
            .unwrap();
        let a = step(&mut g, ActionKind::Hide, 6);
        g.acknowledge(a.token, Ack::Hidden, 6).unwrap();
        let a = step(&mut g, ActionKind::Check, 7);
        let mut absent = report(false);
        absent.evidence.allocated = false;
        g.acknowledge(a.token, Ack::Checked(absent), 7).unwrap();
        let a = step(&mut g, ActionKind::Execute, 8);
        g.acknowledge(a.token, Ack::Unknown, 8).unwrap();
        g.cancel(id).unwrap();
        assert_eq!(g.snapshot().phase, Phase::Quarantined);
        assert_eq!(
            g.retry_preparation(id, context(), world(1), 9, 100),
            Err(Error::NotRecoverable)
        );
        assert_eq!(
            g.recover(id, context(), world(1), true, false),
            Err(Error::NotRecoverable)
        );
        g.recover(id, context(), world(1), true, true).unwrap();
        // The dispatched action retains its argument ownership even after recovery.
        assert_eq!(
            a.command().unwrap().argument(1),
            Some(b"fixture.sav".as_slice())
        );
    }
    #[test]
    fn expired_check_cannot_trigger_a_late_release_or_load() {
        let mut g = Gate::new(9).unwrap();
        g.admit(9, command(), world(1), true, 0, 10).unwrap();
        let a = step(&mut g, ActionKind::Hide, 1);
        g.acknowledge(a.token, Ack::Hidden, 1).unwrap();
        let a = step(&mut g, ActionKind::Check, 2);
        g.acknowledge(a.token, Ack::Checked(report(false)), 10)
            .unwrap();
        assert_eq!(g.snapshot().phase, Phase::Recoverable);
        assert!(g.poll(context(), world(1), 11).unwrap().is_none());
        assert!(!g.snapshot().execute_started);
    }
}
impl fmt::Debug for OwnedCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnedCommand")
            .field("command", &"map")
            .field("argc", &self.values.len())
            .field("bytes", &self.bytes)
            .field("arguments", &"<redacted>")
            .finish()
    }
}
impl OwnedCommand {
    pub fn new(tokens: &[&[u8]]) -> Result<Self, Error> {
        if !(2..=MAX_ARGUMENTS).contains(&tokens.len()) || tokens[0] != b"map" {
            return Err(Error::InvalidArguments);
        }
        let mut values = Vec::new();
        let mut bytes = 0usize;
        for token in tokens {
            bytes = bytes
                .checked_add(token.len() + 1)
                .ok_or(Error::InvalidArguments)?;
            if bytes > MAX_ARGUMENT_BYTES {
                return Err(Error::InvalidArguments);
            }
            values.push(CString::new(*token).map_err(|_| Error::InvalidArguments)?);
        }
        Ok(Self { values, bytes })
    }
    /// Borrowed argv is valid only inside the closure; no textual reconstruction.
    pub fn with_argv<R>(&self, f: impl FnOnce(u32, *const *const c_char) -> R) -> R {
        let pointers: Vec<_> = self.values.iter().map(|s| s.as_ptr()).collect();
        f(pointers.len() as u32, pointers.as_ptr())
    }
    pub fn argument(&self, index: usize) -> Option<&[u8]> {
        self.values.get(index).map(|s| s.as_bytes())
    }
    /// Fixture/adaptor primitive: copy live argv before its stack frame returns.
    /// Not wired to the game. No addresses or argument text appear in errors.
    pub fn capture(
        argc: u32,
        argv: u32,
        mut read: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
    ) -> Result<Self, Error> {
        if !(2..=MAX_ARGUMENTS as u32).contains(&argc) || argv == 0 {
            return Err(Error::InvalidArguments);
        }
        let mut tokens = Vec::new();
        let mut budget = MAX_ARGUMENT_BYTES;
        for i in 0..argc {
            let at = argv
                .checked_add(i * 4)
                .filter(|v| v.checked_add(3).is_some())
                .ok_or(Error::InvalidArguments)?;
            let mut word = [0; 4];
            read(at, &mut word).map_err(|_| Error::UnreadableArguments)?;
            let start = u32::from_le_bytes(word);
            if start == 0 {
                return Err(Error::UnreadableArguments);
            }
            let mut token = Vec::new();
            loop {
                if budget == 0 {
                    return Err(Error::InvalidArguments);
                }
                let mut b = [0];
                let at = start
                    .checked_add(token.len() as u32)
                    .ok_or(Error::UnreadableArguments)?;
                read(at, &mut b).map_err(|_| Error::UnreadableArguments)?;
                budget -= 1;
                if b[0] == 0 {
                    break;
                }
                token.push(b[0]);
            }
            if i == 0 && token != b"map" {
                return Err(Error::InvalidArguments);
            }
            tokens.push(token);
        }
        Self::new(&tokens.iter().map(Vec::as_slice).collect::<Vec<_>>())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Busy,
    WrongThread,
    WrongPhase,
    InvalidWorld,
    InvalidArguments,
    UnreadableArguments,
    InvalidTimeout,
    ClockReversed,
    Exhausted,
    StaleAction,
    WrongAcknowledgement,
    NotRecoverable,
    NotComplete,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct World {
    /// Monotonic lifecycle witness supplied by a future adapter, NOT guessed
    /// from a changed pointer. The current lab supplies synthetic epochs only.
    pub epoch: u64,
    pub scene: u32,
    pub actor: u32,
    pub ready: bool,
}
impl World {
    fn valid(self) -> bool {
        self.epoch != 0 && self.scene != 0 && self.actor != 0 && self.ready
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Hide,
    Check,
    Release,
    Execute,
    AwaitWorld,
    Recreate,
    Complete,
    Recoverable,
    Quarantined,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Hide,
    Check,
    Release,
    Execute,
    Recreate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Token {
    gate: u64,
    id: u64,
    sequence: u64,
    kind: ActionKind,
}
#[derive(Debug, Serialize)]
pub struct Action {
    pub token: Token,
    pub kind: ActionKind,
    #[serde(skip)]
    command: Option<Arc<OwnedCommand>>,
}
impl Action {
    pub fn command(&self) -> Option<&OwnedCommand> {
        self.command.as_deref()
    }
}
/// Acknowledgements describe completed adapter work. ExecuteReturned means
/// callback return, NOT successful load; a new ready world must still be seen.
pub enum Ack {
    Hidden,
    Checked(Report),
    Released,
    /// The release revalidation refused WITHOUT freeing; nothing was executed.
    ReleaseRefused,
    ExecuteReturned,
    NotInvoked,
    Recreated,
    Unknown,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Snapshot {
    pub id: Option<u64>,
    pub phase: Phase,
    pub in_flight: bool,
    pub execute_started: bool,
    pub release_confirmed: bool,
    pub reason: Option<&'static str>,
    pub automatic_load_supported: bool,
}
struct Job {
    id: u64,
    command: Arc<OwnedCommand>,
    old: World,
    new: Option<World>,
    deadline: u64,
    restore_visual: bool,
    phase: Phase,
    sequence: u64,
    in_flight: Option<Token>,
    execute_started: bool,
    release_confirmed: bool,
    reason: Option<&'static str>,
}
pub struct Gate {
    instance: u64,
    owner: u32,
    next_id: u64,
    last_now: u64,
    job: Option<Job>,
}
fn outer(context: Context, owner: u32) -> Result<(), Error> {
    if owner == 0 || context.owner_thread != owner || context.current_thread != owner {
        return Err(Error::WrongThread);
    }
    if !context.pump_installed
        || context.pump_depth != 1
        || context.window_depth != 0
        || context.draw_depth != 0
        || context.render_depth != 0
    {
        return Err(Error::WrongPhase);
    }
    Ok(())
}
impl Gate {
    /// Recheck immediately before a fixture adapter dispatches an issued action.
    /// Polling earlier does not authorize dispatch after cancellation or expiry.
    pub(crate) fn validate_execute(
        &mut self,
        action: &Action,
        context: Context,
        world: World,
        now: u64,
    ) -> Result<(), Error> {
        outer(context, self.owner)?;
        let j = self.job.as_ref().ok_or(Error::StaleAction)?;
        if j.in_flight != Some(action.token)
            || action.kind != ActionKind::Execute
            || !action
                .command
                .as_ref()
                .is_some_and(|c| Arc::ptr_eq(c, &j.command))
        {
            return Err(Error::StaleAction);
        }
        if j.phase != Phase::Execute || !j.release_confirmed {
            return Err(Error::WrongPhase);
        }
        if !world.valid() || world != j.old {
            return Err(Error::InvalidWorld);
        }
        if now >= j.deadline {
            return Err(Error::InvalidTimeout);
        }
        self.clock(now)
    }
    pub fn new(owner: u32) -> Result<Self, Error> {
        if owner == 0 {
            return Err(Error::WrongThread);
        }
        Ok(Self {
            instance: NEXT_GATE
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
                .map_err(|_| Error::Exhausted)?,
            owner,
            next_id: 1,
            last_now: 0,
            job: None,
        })
    }
    fn clock(&mut self, now: u64) -> Result<(), Error> {
        if now < self.last_now {
            return Err(Error::ClockReversed);
        }
        self.last_now = now;
        Ok(())
    }
    fn deadline(now: u64, timeout: u64) -> Result<u64, Error> {
        if timeout == 0 || timeout > MAX_TIMEOUT_MS {
            return Err(Error::InvalidTimeout);
        }
        now.checked_add(timeout).ok_or(Error::InvalidTimeout)
    }
    /// Admission is on the callback owner, not a release from that callback.
    pub fn admit(
        &mut self,
        thread: u32,
        command: OwnedCommand,
        world: World,
        restore_visual: bool,
        now: u64,
        timeout: u64,
    ) -> Result<u64, Error> {
        if thread != self.owner {
            return Err(Error::WrongThread);
        }
        if self.job.is_some() {
            return Err(Error::Busy);
        }
        if !world.valid() {
            return Err(Error::InvalidWorld);
        }
        let deadline = Self::deadline(now, timeout)?;
        let next = self.next_id.checked_add(1).ok_or(Error::Exhausted)?;
        self.clock(now)?;
        let id = self.next_id;
        self.next_id = next;
        self.job = Some(Job {
            id,
            command: Arc::new(command),
            old: world,
            new: None,
            deadline,
            restore_visual,
            phase: Phase::Hide,
            sequence: 0,
            in_flight: None,
            execute_started: false,
            release_confirmed: false,
            reason: None,
        });
        Ok(id)
    }
    pub fn snapshot(&self) -> Snapshot {
        match &self.job {
            Some(j) => Snapshot {
                id: Some(j.id),
                phase: j.phase,
                in_flight: j.in_flight.is_some(),
                execute_started: j.execute_started,
                release_confirmed: j.release_confirmed,
                reason: j.reason,
                automatic_load_supported: false,
            },
            None => Snapshot {
                id: None,
                phase: Phase::Idle,
                in_flight: false,
                execute_started: false,
                release_confirmed: false,
                reason: None,
                automatic_load_supported: false,
            },
        }
    }
    /// At most one action per acknowledgement; never waits for rendering here.
    pub fn poll(
        &mut self,
        context: Context,
        world: World,
        now: u64,
    ) -> Result<Option<Action>, Error> {
        outer(context, self.owner)?;
        self.clock(now)?;
        let Some(j) = &mut self.job else {
            return Ok(None);
        };
        if matches!(
            j.phase,
            Phase::Complete | Phase::Recoverable | Phase::Quarantined
        ) {
            return Ok(None);
        }
        if now >= j.deadline {
            j.phase = if j.execute_started || j.in_flight.is_some() {
                Phase::Quarantined
            } else {
                Phase::Recoverable
            };
            j.reason = Some("deadline_expired");
            return Ok(None);
        }
        if j.in_flight.is_some() {
            return Ok(None);
        }
        if j.phase == Phase::AwaitWorld {
            if world.valid() && world.epoch > j.old.epoch {
                j.new = Some(world);
                j.phase = if j.restore_visual {
                    Phase::Recreate
                } else {
                    Phase::Complete
                };
            } else {
                return Ok(None);
            }
        }
        if j.phase == Phase::Complete {
            return Ok(None);
        }
        let expected = if j.phase == Phase::Recreate {
            j.new.unwrap()
        } else {
            j.old
        };
        if !world.valid() || world != expected {
            j.phase = Phase::Quarantined;
            j.reason = Some("unexpected_world_change");
            return Ok(None);
        }
        let kind = match j.phase {
            Phase::Hide => ActionKind::Hide,
            Phase::Check => ActionKind::Check,
            Phase::Release => ActionKind::Release,
            Phase::Execute => ActionKind::Execute,
            Phase::Recreate => ActionKind::Recreate,
            _ => unreachable!(),
        };
        j.sequence = j.sequence.checked_add(1).ok_or(Error::Exhausted)?;
        let token = Token {
            gate: self.instance,
            id: j.id,
            sequence: j.sequence,
            kind,
        };
        j.in_flight = Some(token);
        if kind == ActionKind::Execute {
            j.execute_started = true;
        }
        Ok(Some(Action {
            token,
            kind,
            command: (kind == ActionKind::Execute).then(|| Arc::clone(&j.command)),
        }))
    }
    pub fn acknowledge(&mut self, token: Token, ack: Ack, now: u64) -> Result<(), Error> {
        let j = self.job.as_ref().ok_or(Error::StaleAction)?;
        if j.in_flight != Some(token) {
            return Err(Error::StaleAction);
        }
        let valid = matches!(
            (&ack, token.kind),
            (Ack::Hidden, ActionKind::Hide)
                | (Ack::Checked(_), ActionKind::Check)
                | (Ack::Released | Ack::ReleaseRefused, ActionKind::Release)
                | (Ack::ExecuteReturned | Ack::NotInvoked, ActionKind::Execute)
                | (Ack::Recreated, ActionKind::Recreate)
        ) || matches!(ack, Ack::Unknown);
        if !valid {
            return Err(Error::WrongAcknowledgement);
        }
        self.clock(now)?;
        let j = self.job.as_mut().unwrap();
        j.in_flight = None;
        if j.phase == Phase::Quarantined {
            return Ok(());
        } // Late work never restarts the job.
        if matches!(ack, Ack::Unknown) {
            j.phase = Phase::Quarantined;
            j.reason = Some("operation_outcome_unknown");
            return Ok(());
        }
        j.phase = match ack {
            Ack::Hidden => Phase::Check,
            Ack::Checked(report) => {
                // Do not trust a caller-mutated ready/retryable boolean.
                let report = release_policy::assess(report.context, report.evidence);
                if outer(report.context, self.owner).is_err() {
                    j.reason = Some("invalid_release_context");
                    Phase::Recoverable
                } else {
                    match report.refusal {
                        None => Phase::Release,
                        Some(Refusal::NotAllocated) => {
                            j.release_confirmed = true;
                            Phase::Execute
                        }
                        Some(r) if r.retryable() => Phase::Check,
                        _ => {
                            j.reason = Some("release_refused");
                            Phase::Recoverable
                        }
                    }
                }
            }
            Ack::Released => {
                j.release_confirmed = true;
                Phase::Execute
            }
            Ack::ReleaseRefused => {
                j.reason = Some("release_refused_after_check");
                Phase::Recoverable
            }
            Ack::ExecuteReturned => Phase::AwaitWorld,
            Ack::NotInvoked => {
                j.execute_started = false;
                j.reason = Some("callback_not_invoked");
                Phase::Recoverable
            }
            Ack::Recreated => Phase::Complete,
            Ack::Unknown => unreachable!(),
        };
        if now >= j.deadline && j.phase != Phase::Complete {
            j.phase = if j.execute_started {
                Phase::Quarantined
            } else {
                Phase::Recoverable
            };
            j.reason = Some("deadline_expired");
        }
        Ok(())
    }
    pub fn cancel(&mut self, id: u64) -> Result<(), Error> {
        let j = self
            .job
            .as_mut()
            .filter(|j| j.id == id)
            .ok_or(Error::StaleAction)?;
        if j.phase == Phase::Complete {
            return Err(Error::NotRecoverable);
        }
        if j.phase == Phase::Quarantined {
            return Ok(()); // Cancellation never turns an unknown outcome into a retry.
        }
        j.phase = if j.execute_started || j.in_flight.is_some() {
            Phase::Quarantined
        } else {
            Phase::Recoverable
        };
        j.reason = Some("cancel_requested");
        Ok(())
    }
    /// Explicit retry only before callback invocation, still in the old world.
    pub fn retry_preparation(
        &mut self,
        id: u64,
        context: Context,
        world: World,
        now: u64,
        timeout: u64,
    ) -> Result<(), Error> {
        outer(context, self.owner)?;
        let deadline = Self::deadline(now, timeout)?;
        let j = self
            .job
            .as_ref()
            .filter(|j| j.id == id)
            .ok_or(Error::StaleAction)?;
        if j.phase != Phase::Recoverable
            || j.in_flight.is_some()
            || j.execute_started
            || world != j.old
            || !world.valid()
        {
            return Err(Error::NotRecoverable);
        }
        self.clock(now)?;
        let j = self.job.as_mut().unwrap();
        j.phase = Phase::Hide;
        j.release_confirmed = false;
        j.deadline = deadline;
        j.reason = None;
        Ok(())
    }
    /// Discard only after explicit recovery confirms no visual/callback remains.
    pub fn recover(
        &mut self,
        id: u64,
        context: Context,
        world: World,
        visual_absent: bool,
        callbacks_idle: bool,
    ) -> Result<(), Error> {
        outer(context, self.owner)?;
        let j = self
            .job
            .as_ref()
            .filter(|j| j.id == id)
            .ok_or(Error::StaleAction)?;
        if !matches!(j.phase, Phase::Recoverable | Phase::Quarantined)
            || j.in_flight.is_some()
            || !visual_absent
            || !callbacks_idle
            || !world.valid()
            || world.epoch < j.old.epoch
        {
            return Err(Error::NotRecoverable);
        }
        self.job = None;
        Ok(())
    }
    pub fn finish(&mut self, id: u64) -> Result<(), Error> {
        let j = self
            .job
            .as_ref()
            .filter(|j| j.id == id)
            .ok_or(Error::StaleAction)?;
        if j.phase != Phase::Complete || j.in_flight.is_some() {
            return Err(Error::NotComplete);
        }
        self.job = None;
        Ok(())
    }
}
impl Gate {
    /// Drop a job that can no longer act, so a NEW request may be admitted.
    /// Never replays the old command: Recoverable/Quarantined/Complete only,
    /// with no action in flight. A quarantined outcome stays unknown. Returns
    /// its phase and whether a requested visual restoration never happened.
    pub fn discard_terminal(&mut self) -> Result<Option<(Phase, bool)>, Error> {
        let Some(j) = &self.job else {
            return Ok(None);
        };
        if j.in_flight.is_some()
            || !matches!(
                j.phase,
                Phase::Recoverable | Phase::Quarantined | Phase::Complete
            )
        {
            return Err(Error::Busy);
        }
        let outcome = (j.phase, j.restore_visual && j.phase != Phase::Complete);
        self.job = None;
        Ok(Some(outcome))
    }
    /// True while a job may still issue an action (not terminal).
    pub fn active(&self) -> bool {
        self.job.as_ref().is_some_and(|j| {
            !matches!(
                j.phase,
                Phase::Recoverable | Phase::Quarantined | Phase::Complete
            )
        })
    }
}
#[cfg(test)]
mod terminal_tests {
    use super::lab::{command, context, report, world};
    use super::*;
    #[test]
    fn refused_release_is_recoverable_and_terminal_jobs_can_be_discarded_without_replay() {
        let mut g = Gate::new(9).unwrap();
        assert_eq!(g.discard_terminal(), Ok(None));
        g.admit(9, command(), world(1), true, 0, 100).unwrap();
        assert!(g.active());
        assert_eq!(g.discard_terminal(), Err(Error::Busy));
        let a = g.poll(context(), world(1), 1).unwrap().unwrap();
        assert_eq!(g.discard_terminal(), Err(Error::Busy));
        g.acknowledge(a.token, Ack::Hidden, 1).unwrap();
        let a = g.poll(context(), world(1), 2).unwrap().unwrap();
        g.acknowledge(a.token, Ack::Checked(report(false)), 2)
            .unwrap();
        let a = g.poll(context(), world(1), 3).unwrap().unwrap();
        assert_eq!(a.kind, ActionKind::Release);
        assert_eq!(g.acknowledge(a.token, Ack::ReleaseRefused, 3), Ok(()));
        let s = g.snapshot();
        assert_eq!(s.phase, Phase::Recoverable);
        assert_eq!(s.reason, Some("release_refused_after_check"));
        assert!(!s.execute_started && !s.release_confirmed && !g.active());
        assert!(g.poll(context(), world(1), 4).unwrap().is_none());
        assert_eq!(g.discard_terminal(), Ok(Some((Phase::Recoverable, true))));
        assert_eq!(g.snapshot().phase, Phase::Idle);
        // ReleaseRefused is only valid for a release action.
        g.admit(9, command(), world(1), true, 5, 100).unwrap();
        let a = g.poll(context(), world(1), 5).unwrap().unwrap();
        assert_eq!(
            g.acknowledge(a.token, Ack::ReleaseRefused, 5),
            Err(Error::WrongAcknowledgement)
        );
        g.acknowledge(a.token, Ack::Unknown, 5).unwrap();
        assert_eq!(g.snapshot().phase, Phase::Quarantined);
        assert_eq!(g.discard_terminal(), Ok(Some((Phase::Quarantined, true))));
    }
}

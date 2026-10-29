//! Deterministic, game-free scenarios. The adapters here model their effects;
//! they do not validate engine callback ABI, world epochs or resource ownership.
use super::*;
use anyhow::{Result, ensure};
#[derive(Default, Debug, Serialize)]
pub struct Totals {
    pub scenarios: u64,
    pub callbacks: u64,
    pub hides: u64,
    pub checks: u64,
    pub releases: u64,
    pub recreations: u64,
    pub recoveries: u64,
}
#[derive(Debug, Serialize)]
pub struct LabReport {
    pub iterations: u32,
    pub totals: Totals,
    pub scenarios: Vec<&'static str>,
    pub game_tested: bool,
    pub automatic_load_supported: bool,
}
pub fn context() -> Context {
    Context {
        current_thread: 9,
        owner_thread: 9,
        pump_installed: true,
        pump_depth: 1,
        ..Context::default()
    }
}
pub fn world(epoch: u64) -> World {
    World {
        epoch,
        scene: 100,
        actor: 200,
        ready: true,
    }
}
pub fn command() -> OwnedCommand {
    OwnedCommand::new(&[
        b"map",
        b"fixture.sav",
        b"test argument with spaces",
        &[0xe7],
    ])
    .unwrap()
}
pub fn report(held: bool) -> Report {
    release_policy::assess(
        context(),
        release_policy::Evidence {
            allocated: true,
            item: 99,
            drawn: 1,
            ever_drawn: true,
            flushes: 20,
            last_draw_flush: 1,
            hidden_at: 2,
            counters: Some(if held { [2, 1, 1] } else { [1, 1, 0] }),
            bindings_present: true,
            ..release_policy::Evidence::default()
        },
    )
}
fn take(
    g: &mut Gate,
    w: World,
    t: &mut u64,
    kind: ActionKind,
    totals: &mut Totals,
) -> Result<Action> {
    *t += 1;
    let a = g
        .poll(context(), w, *t)?
        .ok_or_else(|| anyhow::anyhow!("expected {kind:?}"))?;
    ensure!(a.kind == kind, "wrong action");
    ensure!(
        g.poll(context(), w, *t)?.is_none(),
        "unacknowledged action duplicated"
    );
    match kind {
        ActionKind::Hide => totals.hides += 1,
        ActionKind::Check => totals.checks += 1,
        ActionKind::Release => totals.releases += 1,
        ActionKind::Execute => totals.callbacks += 1,
        ActionKind::Recreate => totals.recreations += 1,
    }
    Ok(a)
}
fn prepare(g: &mut Gate, w: World, t: &mut u64, totals: &mut Totals) -> Result<()> {
    let a = take(g, w, t, ActionKind::Hide, totals)?;
    g.acknowledge(a.token, Ack::Hidden, *t)?;
    for _ in 0..3 {
        let a = take(g, w, t, ActionKind::Check, totals)?;
        g.acknowledge(a.token, Ack::Checked(report(true)), *t)?;
    }
    ensure!(
        !g.snapshot().execute_started,
        "executed while references retained"
    );
    let a = take(g, w, t, ActionKind::Check, totals)?;
    g.acknowledge(a.token, Ack::Checked(report(false)), *t)?;
    let a = take(g, w, t, ActionKind::Release, totals)?;
    g.acknowledge(a.token, Ack::Released, *t)?;
    Ok(())
}
fn execute(g: &mut Gate, w: World, t: &mut u64, totals: &mut Totals, ack: Ack) -> Result<()> {
    let a = take(g, w, t, ActionKind::Execute, totals)?;
    let cmd = a
        .command()
        .ok_or_else(|| anyhow::anyhow!("lost owned arguments"))?;
    ensure!(cmd.argument(1) == Some(b"fixture.sav".as_slice()));
    ensure!(cmd.argument(2) == Some(b"test argument with spaces".as_slice()));
    ensure!(cmd.argument(3) == Some([0xe7].as_slice()));
    g.acknowledge(a.token, ack, *t)?;
    Ok(())
}
pub fn run(iterations: u32) -> Result<LabReport> {
    ensure!(
        (1..=10_000).contains(&iterations),
        "iterations must be 1..=10000"
    );
    let mut totals = Totals::default();
    for n in 0..iterations {
        let w = world(n as u64 * 2 + 1);
        let next = world(w.epoch + 1);
        // Success, including retained references and reused numeric addresses.
        let mut t = 0;
        let mut g = Gate::new(9)?;
        let id = g.admit(9, command(), w, true, t, 100)?;
        prepare(&mut g, w, &mut t, &mut totals)?;
        execute(&mut g, w, &mut t, &mut totals, Ack::ExecuteReturned)?;
        ensure!(g.poll(context(), w, t)?.is_none());
        let a = take(&mut g, next, &mut t, ActionKind::Recreate, &mut totals)?;
        g.acknowledge(a.token, Ack::Recreated, t)?;
        g.finish(id)?;
        ensure!(g.snapshot().phase == Phase::Idle);
        totals.scenarios += 1;
        // A timed out preparation can be explicitly retried in the same world.
        t = 0;
        g = Gate::new(9)?;
        let id = g.admit(9, command(), w, false, 0, 10)?;
        let a = take(&mut g, w, &mut t, ActionKind::Hide, &mut totals)?;
        g.acknowledge(a.token, Ack::Hidden, t)?;
        ensure!(g.poll(context(), w, 10)?.is_none());
        ensure!(g.snapshot().phase == Phase::Recoverable);
        g.retry_preparation(id, context(), w, 11, 100)?;
        t = 11;
        prepare(&mut g, w, &mut t, &mut totals)?;
        execute(&mut g, w, &mut t, &mut totals, Ack::ExecuteReturned)?;
        ensure!(g.poll(context(), next, t)?.is_none());
        g.finish(id)?;
        totals.recoveries += 1;
        totals.scenarios += 1;
        // Cancellation requires explicit cleanup proof, and never calls the load.
        g = Gate::new(9)?;
        let id = g.admit(9, command(), w, true, 0, 100)?;
        g.cancel(id)?;
        ensure!(g.recover(id, context(), w, false, true).is_err());
        g.recover(id, context(), w, true, true)?;
        totals.recoveries += 1;
        totals.scenarios += 1;
        // An ambiguous callback result never becomes a second execution.
        t = 0;
        g = Gate::new(9)?;
        let id = g.admit(9, command(), w, true, 0, 100)?;
        prepare(&mut g, w, &mut t, &mut totals)?;
        execute(&mut g, w, &mut t, &mut totals, Ack::Unknown)?;
        ensure!(g.snapshot().phase == Phase::Quarantined);
        g.cancel(id)?;
        ensure!(g.snapshot().phase == Phase::Quarantined);
        ensure!(g.retry_preparation(id, context(), w, t, 100).is_err());
        ensure!(g.poll(context(), next, t)?.is_none());
        ensure!(g.recover(id, context(), next, true, false).is_err());
        g.recover(id, context(), next, true, true)?;
        totals.recoveries += 1;
        totals.scenarios += 1;
        // A world transition outside the admitted load cannot free stale items.
        t = 0;
        g = Gate::new(9)?;
        let id = g.admit(9, command(), w, true, 0, 100)?;
        let a = take(&mut g, w, &mut t, ActionKind::Hide, &mut totals)?;
        g.acknowledge(a.token, Ack::Hidden, t)?;
        ensure!(g.poll(context(), next, t)?.is_none());
        ensure!(g.snapshot().phase == Phase::Quarantined);
        g.recover(id, context(), next, true, true)?;
        totals.recoveries += 1;
        totals.scenarios += 1;
        // Lost/late acknowledgements retain ownership and cannot restart a job.
        t = 0;
        g = Gate::new(9)?;
        let id = g.admit(9, command(), w, true, 0, 10)?;
        let a = take(&mut g, w, &mut t, ActionKind::Hide, &mut totals)?;
        ensure!(g.poll(context(), w, 10)?.is_none());
        ensure!(g.recover(id, context(), w, true, true).is_err());
        g.acknowledge(a.token, Ack::Hidden, 10)?;
        ensure!(g.acknowledge(a.token, Ack::Hidden, 10).is_err());
        ensure!(g.snapshot().phase == Phase::Quarantined);
        g.recover(id, context(), w, true, true)?;
        totals.recoveries += 1;
        totals.scenarios += 1;
    }
    Ok(LabReport {
        iterations,
        totals,
        scenarios: vec![
            "success_after_references_drain",
            "retry_after_timeout",
            "cancel_before_execution",
            "unknown_outcome_no_replay",
            "unexpected_world_change",
            "late_acknowledgement",
        ],
        game_tested: false,
        automatic_load_supported: false,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn exercises_384_complete_failure_and_recovery_scenarios_without_game() {
        let r = super::run(64).unwrap();
        assert_eq!(r.totals.scenarios, 384);
        assert_eq!(r.totals.callbacks, 192);
        assert_eq!(r.totals.recoveries, 320);
        assert!(!r.game_tested && !r.automatic_load_supported);
    }
}

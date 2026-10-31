//! Each sample opens/drops its controller; waits never monopolize session-stop.
use anyhow::{Context, Result, ensure};
use client_dll::{
    controller::Controller,
    recording::{End, Plan, Recorder, Summary},
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
unsafe extern "system" fn interrupt(event: u32) -> i32 {
    if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        INTERRUPTED.store(true, Ordering::Relaxed);
        1
    } else {
        0
    }
}
struct Handler;
impl Drop for Handler {
    fn drop(&mut self) {
        unsafe {
            SetConsoleCtrlHandler(Some(interrupt), 0);
        }
    }
}
pub fn run(pid: u32, dll: &Path, fixture: bool, output: &Path, plan: Plan) -> Result<Summary> {
    plan.validate()?;
    let initial = Controller::open(pid)?;
    let executable = initial.executable();
    let name = executable
        .file_name()
        .context("target executable has no name")?;
    ensure!(
        if fixture {
            name.eq_ignore_ascii_case("engine_fixture.exe")
                || name.eq_ignore_ascii_case("memory_tool.exe")
        } else {
            name.eq_ignore_ascii_case("prism3d.exe")
        },
        "unexpected recording target"
    );
    // This is passive: no load, attach, start, connect or ownership of teardown.
    initial.runtime_info(dll)?;
    let identity = initial.identity().clone();
    drop(initial);
    INTERRUPTED.store(false, Ordering::Relaxed);
    ensure!(
        unsafe { SetConsoleCtrlHandler(Some(interrupt), 1) } != 0,
        "cannot install recorder interrupt handler"
    );
    let _handler = Handler;
    let mut recorder = Recorder::create(output, plan)?;
    let start = Instant::now();
    let mut next = Duration::ZERO;
    let mut failure = None;
    let end = loop {
        if INTERRUPTED.load(Ordering::Relaxed) {
            break End::Interrupted;
        }
        if start.elapsed() < next {
            std::thread::sleep((next - start.elapsed().min(next)).min(Duration::from_millis(100)));
            continue;
        }
        let sample = (|| -> Result<bool> {
            let controller = Controller::open(pid)?;
            if controller.identity() != &identity {
                return Ok(false);
            }
            let status = super::status(&controller, dll)?;
            recorder.push(
                start.elapsed().as_millis() as u64,
                &serde_json::to_vec(&status)?,
            )?;
            Ok(true)
        })();
        match sample {
            Ok(true) => (),
            Ok(false) => break End::TargetChanged,
            Err(error) => {
                failure = Some(error);
                break End::CollectionFailed;
            }
        }
        if start.elapsed() >= Duration::from_secs(plan.seconds) {
            break End::DurationReached;
        }
        // Skip missed slots; never enqueue catch-up calls or hold the controller while waiting.
        next = Duration::from_millis(
            ((start.elapsed().as_millis() as u64 / plan.interval_ms) + 1) * plan.interval_ms,
        )
        .min(Duration::from_secs(plan.seconds));
    };
    let summary = recorder.finish(end, start.elapsed().as_millis() as u64)?;
    if let Some(error) = failure {
        eprintln!(
            "Recorder stopped after a collection failure; session cleanup remains explicit: {error:#}"
        );
    }
    Ok(summary)
}

//! Timed external recording for `field_probe`: only ReadProcessMemory, with the
//! same profile checks as the appearance reader. No injection, calls or writes.
use super::{appearance, process::Process};
use crate::{
    field_probe::{MAX_BYTES, Phase, Recorder, WINDOWS},
    pose_capture,
};
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Default, serde::Serialize)]
pub struct Summary {
    pub frames: usize,
    pub bytes: usize,
    /// Ticks without the actor chain: menus, loading or a game that closed.
    pub skipped: usize,
    /// A window read failed; the frame kept that window's previous contents.
    pub partial: usize,
    pub complete: bool,
}

/// Saves after every phase, so a crash keeps everything up to the last one.
pub fn record(
    process: &Process,
    phases: &[Phase],
    hz: u32,
    output: &Path,
    mut announce: impl FnMut(usize, &Phase),
) -> Result<Summary> {
    ensure!((1..=30).contains(&hz), "sample rate must be 1..=30 Hz");
    let (_, layout) = appearance::verified(process)?;
    let mut file =
        File::create_new(output).with_context(|| format!("{} already exists", output.display()))?;
    let mut recorder = Recorder::new(phases)?;
    let mut buffers: Vec<Vec<u8>> = WINDOWS.iter().map(|(_, n)| vec![0u8; *n]).collect();
    let interval = Duration::from_secs_f64(1.0 / f64::from(hz));
    let started = Instant::now();
    let mut summary = Summary::default();
    let mut written = 0usize;
    let mut next = Instant::now();
    'script: for (index, phase) in phases.iter().enumerate() {
        announce(index, phase);
        let end = Instant::now() + Duration::from_secs(u64::from(phase.seconds));
        while Instant::now() < end {
            if process.ensure_running().is_err() {
                break 'script;
            }
            let mut read =
                |at: u32, out: &mut [u8]| process.read(u64::from(at), out).map_err(|_| ());
            match pose_capture::witness(layout.actor, &mut read) {
                Ok(chain) => {
                    // actor, physics, body, scene: values the pose chain already verified.
                    let starts = [chain[8], chain[10], chain[11], chain[1]];
                    let mut partial = false;
                    for (buffer, start) in buffers.iter_mut().zip(starts) {
                        partial |= read(start, buffer).is_err();
                    }
                    summary.partial += usize::from(partial);
                    let refs: Vec<&[u8]> = buffers.iter().map(Vec::as_slice).collect();
                    let ms = u32::try_from(started.elapsed().as_millis())?;
                    recorder.frame(ms, u16::try_from(index)?, chain[8], &refs)?;
                    summary.frames += 1;
                }
                Err(_) => summary.skipped += 1,
            }
            next += interval;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
        file.write_all(&recorder.bytes()[written..])?;
        file.sync_data()?;
        written = recorder.len();
        ensure!(written as u64 <= MAX_BYTES, "recording size limit");
        summary.complete = index + 1 == phases.len();
    }
    file.write_all(&recorder.bytes()[written..])?;
    file.sync_all()?;
    summary.bytes = recorder.len();
    Ok(summary)
}

//! Atomic local tuning and approximate diagnostic counters. No engine calls,
//! I/O, allocations or blocking on the render path. Settings change as one word.
use crate::interpolation::Config;
#[cfg(all(windows, target_arch = "x86"))]
use crate::interpolation::{Evaluation, Mode};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static CONFIG: AtomicU32 = AtomicU32::new(100 | (200 << 16));
static REVISION: AtomicU32 = AtomicU32::new(0);
static COUNTS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub config: Config,
    pub revision: u32,
    pub buffering: u64,
    pub interpolated: u64,
    pub predicted: u64,
    pub capped: u64,
    pub held: u64,
    pub snapped: u64,
    pub stale: u64,
    pub empty_queries: u64,
    pub lock_misses: u64,
}

pub fn config() -> Config {
    let value = CONFIG.load(Ordering::Acquire);
    Config {
        delay_ms: value as u16,
        extrapolation_ms: (value >> 16) as u16,
    }
}
pub fn configure(value: Config) -> Result<(), String> {
    value.validate()?;
    CONFIG.store(
        u32::from(value.delay_ms) | (u32::from(value.extrapolation_ms) << 16),
        Ordering::Release,
    );
    REVISION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
pub fn status() -> Status {
    let counts: [u64; 9] = std::array::from_fn(|i| COUNTS[i].load(Ordering::Relaxed));
    Status {
        config: config(),
        revision: REVISION.load(Ordering::Relaxed),
        buffering: counts[0],
        interpolated: counts[1],
        predicted: counts[2],
        capped: counts[3],
        held: counts[4],
        snapped: counts[5],
        stale: counts[6],
        empty_queries: counts[7],
        lock_misses: counts[8],
    }
}
#[cfg(all(windows, target_arch = "x86"))]
pub(crate) fn record(value: &Evaluation) {
    let index = match value.mode {
        Mode::Buffering => 0,
        Mode::Interpolated => 1,
        Mode::Predicted => 2,
        Mode::Capped => 3,
        Mode::Held => 4,
        Mode::Snapped => 5,
    };
    COUNTS[index].fetch_add(1, Ordering::Relaxed);
}
#[cfg(all(windows, target_arch = "x86"))]
pub(crate) fn stale() {
    COUNTS[6].fetch_add(1, Ordering::Relaxed);
}
#[cfg(all(windows, target_arch = "x86"))]
pub(crate) fn empty() {
    COUNTS[7].fetch_add(1, Ordering::Relaxed);
}
#[cfg(all(windows, target_arch = "x86"))]
pub(crate) fn lock_miss() {
    COUNTS[8].fetch_add(1, Ordering::Relaxed);
}

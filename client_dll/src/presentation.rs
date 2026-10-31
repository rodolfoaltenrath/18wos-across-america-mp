//! Atomic local tuning and approximate diagnostic counters. No engine calls,
//! I/O, allocations or blocking on the render path. Settings change as one word.
use crate::interpolation::{Config, Timebase};
#[cfg(all(windows, target_arch = "x86"))]
use crate::interpolation::{Evaluation, Mode};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

static CONFIG: AtomicU32 = AtomicU32::new(100 | (200 << 16));
static REVISION: AtomicU32 = AtomicU32::new(0);
static COUNTS: [AtomicU64; 11] = [const { AtomicU64::new(0) }; 11];
/// Bit 31 of CONFIG selects the server timebase; delay/extrapolation below it.
const SERVER_TIMEBASE: u32 = 1 << 31;

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
    /// Samples stamped by server receipt time / kept at arrival time (absent
    /// in older reports). Arrival includes v1 and missing clock estimates.
    #[serde(default)]
    pub server_timed: u64,
    #[serde(default)]
    pub arrival_timed: u64,
}

pub fn config() -> Config {
    let value = CONFIG.load(Ordering::Acquire);
    Config {
        delay_ms: value as u16,
        extrapolation_ms: ((value >> 16) & 0x7fff) as u16,
        timebase: if value & SERVER_TIMEBASE != 0 {
            Timebase::Server
        } else {
            Timebase::Arrival
        },
    }
}
pub fn configure(value: Config) -> Result<(), String> {
    value.validate()?;
    CONFIG.store(
        u32::from(value.delay_ms)
            | (u32::from(value.extrapolation_ms) << 16)
            | if value.timebase == Timebase::Server {
                SERVER_TIMEBASE
            } else {
                0
            },
        Ordering::Release,
    );
    REVISION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}
pub fn status() -> Status {
    let counts: [u64; 11] = std::array::from_fn(|i| COUNTS[i].load(Ordering::Relaxed));
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
        server_timed: counts[9],
        arrival_timed: counts[10],
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
/// Receive path: count which clock stamped a sample (worker thread).
pub(crate) fn timed(server: bool) {
    COUNTS[if server { 9 } else { 10 }].fetch_add(1, Ordering::Relaxed);
}

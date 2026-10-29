//! Per-connection diagnostic timing. Relative server/arrival intervals do not
//! require clock synchronization. Missing ticks are not a packet-loss estimate.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub rtt_samples: u64,
    pub rtt_max_ms: u64,
    pub rtt_jitter_us: u64,
    pub snapshots: u64,
    pub arrival_jitter_us: u64,
    pub max_receive_gap_ms: u64,
    pub tick_gap_events: u64,
    pub largest_tick_step: u64,
    pub nonmonotonic_snapshots: u64,
}

#[derive(Default)]
pub(crate) struct Recorder {
    pub value: Timing,
    last_rtt: Option<u64>,
    last_snapshot: Option<(u64, u64, u64)>,
}

/// Integer EMA with weight 1/16, no wrapping intermediate or clock subtraction.
fn smooth(old: u64, sample: u64) -> u64 {
    if sample >= old {
        old + (sample - old) / 16
    } else {
        old - (old - sample) / 16
    }
}

impl Recorder {
    pub fn rtt(&mut self, milliseconds: u64) {
        self.value.rtt_samples = self.value.rtt_samples.saturating_add(1);
        self.value.rtt_max_ms = self.value.rtt_max_ms.max(milliseconds);
        if let Some(last) = self.last_rtt {
            self.value.rtt_jitter_us = smooth(
                self.value.rtt_jitter_us,
                milliseconds.abs_diff(last).saturating_mul(1000),
            );
        }
        self.last_rtt = Some(milliseconds);
    }
    /// Count all admitted snapshot datagrams; measure only advancing ticks.
    /// Batches/duplicates with the same tick never reset the interval baseline.
    pub fn snapshot(&mut self, tick: u64, server_ms: u64, arrival_ms: u64) {
        self.value.snapshots = self.value.snapshots.saturating_add(1);
        if let Some((last_tick, server, arrival)) = self.last_snapshot {
            if tick < last_tick || server_ms < server || arrival_ms < arrival {
                self.value.nonmonotonic_snapshots =
                    self.value.nonmonotonic_snapshots.saturating_add(1);
                return;
            }
            if tick == last_tick {
                return;
            }
            let step = tick - last_tick;
            self.value.largest_tick_step = self.value.largest_tick_step.max(step);
            if step > 1 {
                self.value.tick_gap_events = self.value.tick_gap_events.saturating_add(1);
            }
            let gap = arrival_ms - arrival;
            self.value.max_receive_gap_ms = self.value.max_receive_gap_ms.max(gap);
            let variation = gap.abs_diff(server_ms - server).saturating_mul(1000);
            self.value.arrival_jitter_us = smooth(self.value.arrival_jitter_us, variation);
        }
        self.last_snapshot = Some((tick, server_ms, arrival_ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regular_arrivals_batches_and_duplicate_ticks_have_zero_variation() {
        let mut recorder = Recorder::default();
        for tick in 0..=10 {
            recorder.snapshot(tick, tick * 16, 100 + tick * 16);
            recorder.snapshot(tick, tick * 16, 101 + tick * 16);
        }
        assert_eq!(recorder.value.snapshots, 22);
        assert_eq!(recorder.value.arrival_jitter_us, 0);
        assert_eq!(recorder.value.max_receive_gap_ms, 16);
        assert_eq!(recorder.value.tick_gap_events, 0);
        assert_eq!(recorder.value.largest_tick_step, 1);
    }
    #[test]
    fn delayed_arrivals_and_rtt_variation_are_measured_without_an_offset() {
        let mut recorder = Recorder::default();
        recorder.snapshot(1, 100, 1100);
        recorder.snapshot(2, 116, 1140);
        assert_eq!(recorder.value.arrival_jitter_us, 1500);
        recorder.snapshot(3, 132, 1140);
        assert!(recorder.value.arrival_jitter_us > 1500);
        assert_eq!(recorder.value.max_receive_gap_ms, 40);
        recorder.rtt(20);
        recorder.rtt(60);
        assert_eq!(recorder.value.rtt_jitter_us, 2500);
        assert_eq!(recorder.value.rtt_max_ms, 60);
        assert_eq!(recorder.value.rtt_samples, 2);
    }
    #[test]
    fn reordered_ticks_do_not_replace_baseline_and_tick_gaps_do_not_claim_loss() {
        let mut recorder = Recorder::default();
        recorder.snapshot(10, 100, 1100);
        recorder.snapshot(9, 90, 1101);
        recorder.snapshot(10, 100, 1102);
        recorder.snapshot(13, 148, 1148);
        assert_eq!(recorder.value.nonmonotonic_snapshots, 1);
        assert_eq!(recorder.value.arrival_jitter_us, 0);
        assert_eq!(recorder.value.tick_gap_events, 1);
        assert_eq!(recorder.value.largest_tick_step, 3);
        assert_eq!(recorder.value.max_receive_gap_ms, 48);
    }
    #[test]
    fn extreme_counters_and_clock_values_saturate_without_wrapping() {
        let mut recorder = Recorder::default();
        recorder.value.rtt_samples = u64::MAX;
        recorder.value.snapshots = u64::MAX;
        recorder.rtt(0);
        recorder.rtt(u64::MAX);
        recorder.snapshot(0, 0, 0);
        recorder.snapshot(u64::MAX, 0, u64::MAX);
        assert_eq!(recorder.value.rtt_samples, u64::MAX);
        assert_eq!(recorder.value.snapshots, u64::MAX);
        assert_eq!(recorder.value.arrival_jitter_us, u64::MAX / 16);
        assert_eq!(recorder.value.rtt_jitter_us, u64::MAX / 16);
        assert_eq!(recorder.value.max_receive_gap_ms, u64::MAX);
    }
}

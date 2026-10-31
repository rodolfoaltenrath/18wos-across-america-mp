//! Constant-space ingress windows and observed cost of a bounded tick batch.
use serde::Serialize;
#[derive(Default, Clone)]
pub(crate) struct Window {
    pub(crate) start: u64,
    pub(crate) used: u32,
}
impl Window {
    pub(crate) fn admit(&mut self, now: u64, limit: u32) -> bool {
        if now.saturating_sub(self.start) >= 1000 {
            self.start = now;
            self.used = 0;
        }
        if self.used >= limit {
            return false;
        }
        self.used += 1;
        true
    }
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Broadcast {
    pub batches: u64,
    pub attempted_datagrams: u64,
    pub encoded_bytes: u64,
    pub max_datagram_bytes: usize,
    pub max_datagrams_per_batch: usize,
    pub max_bytes_per_batch: u64,
    pub processing_ns: u64,
    pub max_processing_ns: u64,
}
impl Broadcast {
    pub fn record(&mut self, packets: usize, bytes: u64, max_datagram: usize, nanos: u64) {
        self.batches = self.batches.saturating_add(1);
        self.attempted_datagrams = self.attempted_datagrams.saturating_add(packets as u64);
        self.encoded_bytes = self.encoded_bytes.saturating_add(bytes);
        self.max_datagram_bytes = self.max_datagram_bytes.max(max_datagram);
        self.max_datagrams_per_batch = self.max_datagrams_per_batch.max(packets);
        self.max_bytes_per_batch = self.max_bytes_per_batch.max(bytes);
        self.processing_ns = self.processing_ns.saturating_add(nanos);
        self.max_processing_ns = self.max_processing_ns.max(nanos);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_is_bounded_before_parse_and_clock_regression_cannot_reset_it() {
        let mut w = Window::default();
        for _ in 0..240 {
            assert!(w.admit(100, 240));
        }
        assert!(!w.admit(50, 240));
        assert!(!w.admit(999, 240));
        assert!(w.admit(1000, 240));
        let mut other = Window::default();
        assert!(other.admit(999, 240));
    }
    #[test]
    fn cost_keeps_totals_peaks_and_saturates() {
        let mut b = Broadcast::default();
        b.record(4, 1000, 300, 50);
        b.record(2, 700, 400, 80);
        assert_eq!(
            (b.batches, b.attempted_datagrams, b.encoded_bytes),
            (2, 6, 1700)
        );
        assert_eq!(
            (
                b.max_datagrams_per_batch,
                b.max_bytes_per_batch,
                b.max_datagram_bytes
            ),
            (4, 1000, 400)
        );
        b.processing_ns = u64::MAX;
        b.record(0, 0, 0, 1);
        assert_eq!(b.processing_ns, u64::MAX);
    }
}

//! Remote pose buffering: render a fixed delay behind the newest sample,
//! interpolate between neighbours, extrapolate briefly, never across teleports.
//! Pure and allocation-free on the read path; units are game centimeters.
use std::collections::VecDeque;
use std::{sync::OnceLock, time::Instant};

/// Seconds on a process-local monotonic clock shared by receiver and renderer.
pub fn now() -> f64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Samples kept per remote player (~0.25 s at 60 Hz).
pub const HISTORY: usize = 16;
/// Render this far behind the newest receive time to absorb jitter.
pub const DELAY: f64 = 0.1;
/// Maximum forward prediction past the newest sample.
pub const MAX_EXTRAPOLATION: f64 = 0.2;
/// Faster implied motion (cm/s, ~720 km/h) is treated as a teleport: snap.
pub const TELEPORT_SPEED: f32 = 20_000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Receive time in seconds on a local monotonic clock.
    pub at: f64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct History(VecDeque<Sample>);

impl History {
    /// Append in receive order; older or equal timestamps replace nothing.
    pub fn push(&mut self, sample: Sample) {
        if self.0.back().is_some_and(|last| sample.at < last.at) {
            return;
        }
        if self.0.len() == HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(sample);
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn newest(&self) -> Option<&Sample> {
        self.0.back()
    }
    /// Pose to draw at local time `now` (already includes no delay).
    pub fn pose(&self, now: f64) -> Option<([f32; 3], [f32; 4])> {
        pose_at(self.0.iter().copied(), now - DELAY)
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (b[i] - a[i]).powi(2)).sum::<f32>().sqrt()
}
/// Normalized linear quaternion blend along the shorter arc.
pub fn nlerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    let q: [f32; 4] = std::array::from_fn(|i| a[i] + (sign * b[i] - a[i]) * t);
    let norm = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    if norm.is_finite() && norm > 1e-6 {
        q.map(|c| c / norm)
    } else {
        b
    }
}
fn teleport(a: &Sample, b: &Sample) -> bool {
    let dt = (b.at - a.at) as f32;
    dt <= 0.0 || distance(a.position, b.position) > TELEPORT_SPEED * dt.max(1.0 / 60.0)
}

/// Samples must be in non-decreasing time order.
pub fn pose_at(
    samples: impl DoubleEndedIterator<Item = Sample> + Clone,
    t: f64,
) -> Option<([f32; 3], [f32; 4])> {
    let mut previous: Option<Sample> = None;
    for s in samples.clone() {
        if s.at >= t {
            return Some(match previous {
                None => (s.position, s.rotation),
                Some(p) if teleport(&p, &s) => (s.position, s.rotation),
                Some(p) => {
                    let alpha = ((t - p.at) / (s.at - p.at)) as f32;
                    (
                        lerp(p.position, s.position, alpha),
                        nlerp(p.rotation, s.rotation, alpha),
                    )
                }
            });
        }
        previous = Some(s);
    }
    // Past the newest sample: bounded linear prediction from the last two.
    let mut newest = samples.rev();
    let last = newest.next()?;
    let ahead = (t - last.at).min(MAX_EXTRAPOLATION) as f32;
    match newest.next() {
        Some(before) if !teleport(&before, &last) => {
            let dt = (last.at - before.at) as f32;
            let velocity: [f32; 3] =
                std::array::from_fn(|i| (last.position[i] - before.position[i]) / dt);
            Some((
                std::array::from_fn(|i| last.position[i] + velocity[i] * ahead),
                last.rotation,
            ))
        }
        _ => Some((last.position, last.rotation)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const I: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    fn s(at: f64, x: f32) -> Sample {
        Sample {
            at,
            position: [x, 0.0, 0.0],
            rotation: I,
        }
    }
    #[test]
    fn interpolates_between_neighbours_and_clamps_before_history() {
        let h: Vec<_> = vec![s(1.0, 0.0), s(1.1, 100.0), s(1.2, 300.0)];
        let at = |t| pose_at(h.iter().copied(), t).unwrap().0[0];
        assert_eq!(at(0.5), 0.0);
        assert!((at(1.05) - 50.0).abs() < 1e-3);
        assert!((at(1.15) - 200.0).abs() < 1e-2);
        assert_eq!(at(1.2), 300.0);
    }
    #[test]
    fn extrapolates_briefly_then_holds() {
        let h = [s(1.0, 0.0), s(1.1, 100.0)];
        let at = |t| pose_at(h.iter().copied(), t).unwrap().0[0];
        assert!((at(1.15) - 150.0).abs() < 1e-2);
        // Capped at MAX_EXTRAPOLATION (0.2 s at 1000 cm/s).
        assert!((at(5.0) - 300.0).abs() < 1e-2);
        assert_eq!(pose_at([s(1.0, 7.0)].into_iter(), 3.0).unwrap().0[0], 7.0);
        assert!(pose_at(std::iter::empty(), 1.0).is_none());
    }
    #[test]
    fn teleports_snap_instead_of_sliding_or_extrapolating() {
        let h = [s(1.0, 0.0), s(1.1, 1_000_000.0)];
        assert_eq!(pose_at(h.iter().copied(), 1.05).unwrap().0[0], 1_000_000.0);
        assert_eq!(pose_at(h.iter().copied(), 1.3).unwrap().0[0], 1_000_000.0);
    }
    #[test]
    fn quaternion_blend_takes_short_arc_and_stays_unit() {
        let a = I;
        let b = [0.0, 1.0, 0.0, 0.0]; // 180 degrees about y
        let neg = [0.0, 0.0, 0.0, -1.0]; // same orientation as identity
        assert_eq!(nlerp(a, neg, 0.5), I);
        let mid = nlerp(a, b, 0.5);
        let norm: f32 = mid.iter().map(|c| c * c).sum();
        assert!((norm - 1.0).abs() < 1e-5);
        assert!((mid[1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
    }
    #[test]
    fn history_is_bounded_ordered_and_rendered_with_delay() {
        let mut h = History::default();
        for i in 0..40 {
            h.push(s(f64::from(i) / 60.0, i as f32 * 10.0));
        }
        h.push(s(0.0, -1.0)); // out of order: ignored
        assert_eq!(h.len(), HISTORY);
        assert_eq!(h.newest().unwrap().position[0], 390.0);
        let newest = h.newest().unwrap().at;
        let x = h.pose(newest).unwrap().0[0];
        // DELAY = 6 ticks behind the newest sample.
        assert!((x - 330.0).abs() < 1e-2, "{x}");
    }
}

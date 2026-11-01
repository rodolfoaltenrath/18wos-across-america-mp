//! Remote pose buffering: render a fixed delay behind the newest sample,
//! interpolate between neighbours, extrapolate briefly, never across teleports.
//! Pure and allocation-free on the read path; units are game centimeters.
use serde::{Deserialize, Serialize};
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
/// A receive burst cannot measure velocity faster than the nominal source tick.
pub const MIN_SAMPLE_INTERVAL: f64 = 1.0 / multiplayer_server::protocol::TICK_HZ as f64;
/// A remote hidden by receive age starts a new movement segment on return.
/// This is a presentation timeout, not a measurement of network packet loss.
pub const STALE_AFTER_MS: u64 = 1000;

/// Clock that stamps each received sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timebase {
    /// Local arrival time (v1 and v2): includes downlink jitter and tick waits.
    #[default]
    Arrival,
    /// v2 server receipt time mapped by the clock-offset estimate; falls back
    /// to arrival per sample while no usable estimate exists.
    Server,
}
/// Explicit local presentation budget, unrelated to server/game physics time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub delay_ms: u16,
    pub extrapolation_ms: u16,
    /// Absent in older JSON: arrival.
    #[serde(default)]
    pub timebase: Timebase,
}
impl Config {
    pub const DEFAULT: Self = Self {
        delay_ms: 100,
        extrapolation_ms: 200,
        timebase: Timebase::Arrival,
    };
    pub fn validate(self) -> Result<(), String> {
        if self.delay_ms > 200 || self.extrapolation_ms > 200 {
            return Err("presentation delay and extrapolation must be 0..=200 ms".into());
        }
        Ok(())
    }
}
impl Default for Config {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Buffering,
    Interpolated,
    Predicted,
    Capped,
    Held,
    Snapped,
}

#[derive(Clone, Copy, Debug)]
pub struct Evaluation {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub mode: Mode,
    pub prediction_ms: u16,
}
impl Evaluation {
    fn new(position: [f32; 3], rotation: [f32; 4], mode: Mode) -> Self {
        Self {
            position,
            rotation,
            mode,
            prediction_ms: 0,
        }
    }
    fn pose(self) -> ([f32; 3], [f32; 4]) {
        (self.position, self.rotation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Receive time in seconds on a local monotonic clock.
    pub at: f64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

impl Sample {
    fn valid(self) -> bool {
        self.at.is_finite()
            && self.at >= 0.0
            && self.position.into_iter().all(f32::is_finite)
            && multiplayer_server::protocol::Rotation {
                x: self.rotation[0],
                y: self.rotation[1],
                z: self.rotation[2],
                w: self.rotation[3],
            }
            .is_valid()
    }
}

/// Presentation time of a sample the server stamped at `received_ms` (server
/// clock). `offset_ms` estimates server minus local; `local_now_ms` is the local
/// clock read at `arrival` (presentation seconds). A small negative age (offset
/// error) keeps a stamp up to 250 ms ahead; an implausible one returns None so
/// the caller keeps the arrival time. Uplink jitter remains; downlink jitter
/// and tick waits go.
pub fn server_sample_time(
    arrival: f64,
    local_now_ms: u64,
    received_ms: u64,
    offset_ms: i64,
) -> Option<f64> {
    let local_received = i128::from(received_ms) - i128::from(offset_ms);
    let age_ms = i128::from(local_now_ms) - local_received;
    if !(-250..=1000).contains(&age_ms) {
        return None;
    }
    // Slightly future stamps are kept: mixing in arrival times would bring
    // the jitter back, and rendering already runs `delay` behind.
    let at = arrival - age_ms as f64 / 1000.0;
    (at.is_finite() && at >= 0.0).then_some(at)
}
#[derive(Clone, Debug, Default)]
pub struct History(VecDeque<Sample>);

impl History {
    /// Ignore invalid/older samples; equal receive times retain only the latest pose.
    pub fn push(&mut self, sample: Sample) {
        if !sample.valid() || self.0.back().is_some_and(|last| sample.at < last.at) {
            return;
        }
        if let Some(last) = self.0.back_mut()
            && sample.at == last.at
        {
            *last = sample;
            return;
        }
        if self.0.len() == HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(sample);
    }
    /// Like `push`, but a sample older than the newest (a clock estimate that
    /// moved backwards) is re-stamped to the newest time instead of dropped.
    pub fn push_monotonic(&mut self, mut sample: Sample) {
        // f64::max would replace NaN with the previous valid time. Reject first,
        // otherwise an invalid timestamp could replace the newest valid pose.
        if !sample.valid() {
            return;
        }
        if let Some(last) = self.0.back() {
            sample.at = sample.at.max(last.at);
        }
        self.push(sample);
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
        self.evaluate(now, Config::DEFAULT).map(Evaluation::pose)
    }
    pub fn evaluate(&self, now: f64, config: Config) -> Option<Evaluation> {
        config.validate().ok()?;
        evaluate_at(
            self.0.iter().copied(),
            now - f64::from(config.delay_ms) / 1000.0,
            f64::from(config.extrapolation_ms) / 1000.0,
        )
    }
}

/// Receive-side history with explicit timestamp provenance. Server receipt and
/// local arrival are different timelines even though both are mapped to local
/// seconds. Never infer velocity between them, or across a stale receive gap.
/// Connection/pose-generation changes still require the caller to replace this
/// value; packet sequence validation remains the receiver's responsibility.
#[derive(Clone, Debug, Default)]
pub struct ReceiveHistory {
    history: History,
    arrival: Option<f64>,
    timebase: Option<Timebase>,
}
impl ReceiveHistory {
    /// Insert a validated, ordered packet. `arrival` is the monotonic receive
    /// time, independent of the possibly future server stamp in `sample.at`.
    /// Return false without changing anything for malformed/backwards arrivals.
    pub fn receive(&mut self, sample: Sample, arrival: f64, timebase: Timebase) -> bool {
        if !sample.valid()
            || !arrival.is_finite()
            || arrival < 0.0
            || self.arrival.is_some_and(|last| arrival < last)
        {
            return false;
        }
        if self.timebase.is_some_and(|last| last != timebase)
            || self
                .arrival
                .is_some_and(|last| arrival - last >= STALE_AFTER_MS as f64 / 1000.0)
        {
            self.history = History::default();
        }
        self.history.push_monotonic(sample);
        self.arrival = Some(arrival);
        self.timebase = Some(timebase);
        true
    }
    pub fn len(&self) -> usize {
        self.history.len()
    }
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }
    pub fn newest(&self) -> Option<&Sample> {
        self.history.newest()
    }
    pub fn evaluate(&self, now: f64, config: Config) -> Option<Evaluation> {
        self.history.evaluate(now, config)
    }
}

fn lerp(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| {
        (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * f64::from(t)) as f32
    })
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f64 {
    (0..3)
        .map(|i| (f64::from(b[i]) - f64::from(a[i])).powi(2))
        .sum::<f64>()
        .sqrt()
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
    let dt = b.at - a.at;
    dt <= 0.0
        || distance(a.position, b.position)
            > f64::from(TELEPORT_SPEED) * dt.max(MIN_SAMPLE_INTERVAL)
}

/// Samples must be in non-decreasing time order.
pub fn pose_at(
    samples: impl DoubleEndedIterator<Item = Sample> + Clone,
    t: f64,
) -> Option<([f32; 3], [f32; 4])> {
    evaluate_at(samples, t, MAX_EXTRAPOLATION).map(Evaluation::pose)
}

fn evaluate_at(
    samples: impl DoubleEndedIterator<Item = Sample> + Clone,
    t: f64,
    max_extrapolation: f64,
) -> Option<Evaluation> {
    if !t.is_finite() || !(0.0..=MAX_EXTRAPOLATION).contains(&max_extrapolation) {
        return None;
    }
    let samples = samples.filter(|sample| sample.valid());
    let mut previous: Option<Sample> = None;
    for s in samples.clone() {
        if s.at >= t {
            return Some(match previous {
                None => Evaluation::new(s.position, s.rotation, Mode::Buffering),
                Some(p) if teleport(&p, &s) => {
                    Evaluation::new(s.position, s.rotation, Mode::Snapped)
                }
                Some(p) => {
                    let alpha = ((t - p.at) / (s.at - p.at)) as f32;
                    Evaluation::new(
                        lerp(p.position, s.position, alpha),
                        nlerp(p.rotation, s.rotation, alpha),
                        Mode::Interpolated,
                    )
                }
            });
        }
        previous = Some(s);
    }
    // Past the newest sample: bounded linear prediction from the last two.
    let mut newest = samples.rev();
    let last = newest.next()?;
    let requested = (t - last.at).max(0.0);
    let ahead = requested.min(max_extrapolation);
    match newest.next() {
        Some(before) if !teleport(&before, &last) => {
            let dt = (last.at - before.at).max(MIN_SAMPLE_INTERVAL);
            let position: [f32; 3] = std::array::from_fn(|i| {
                (f64::from(last.position[i])
                    + (f64::from(last.position[i]) - f64::from(before.position[i])) / dt * ahead)
                    as f32
            });
            if !position.into_iter().all(f32::is_finite) || max_extrapolation == 0.0 {
                return Some(Evaluation::new(last.position, last.rotation, Mode::Held));
            }
            Some(Evaluation {
                position,
                rotation: last.rotation,
                mode: if requested > max_extrapolation {
                    Mode::Capped
                } else {
                    Mode::Predicted
                },
                prediction_ms: (ahead * 1000.0).round() as u16,
            })
        }
        Some(_) => Some(Evaluation::new(last.position, last.rotation, Mode::Snapped)),
        None => Some(Evaluation::new(last.position, last.rotation, Mode::Held)),
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
#[cfg(test)]
mod timebase_tests {
    use super::*;
    #[test]
    fn server_time_removes_arrival_delay_and_rejects_implausible_estimates() {
        // Server stamped 1000; offset +200 means local 800; read at local 860.
        let at = server_sample_time(10.0, 860, 1000, 200).unwrap();
        assert!((at - 9.94).abs() < 1e-9);
        // Estimate slightly ahead (age -100 ms): kept consistently on the server clock.
        assert!((server_sample_time(10.0, 700, 1000, 200).unwrap() - 10.1).abs() < 1e-9);
        assert_eq!(server_sample_time(10.0, 500, 1000, 200), None);
        assert_eq!(server_sample_time(10.0, 3000, 1000, 200), None);
        assert_eq!(server_sample_time(0.5, 860, 0, -100), None, "before epoch");
        assert_eq!(server_sample_time(10.0, u64::MAX, u64::MAX, i64::MIN), None);
    }
    #[test]
    fn monotonic_push_restamps_a_backwards_estimate_instead_of_dropping_it() {
        let sample = |at: f64, x: f32| Sample {
            at,
            position: [x, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        let mut h = History::default();
        h.push_monotonic(sample(1.0, 1.0));
        h.push_monotonic(sample(0.9, 2.0));
        assert_eq!(h.len(), 1);
        assert_eq!(h.newest().unwrap().position[0], 2.0);
        h.push_monotonic(sample(1.1, 3.0));
        assert_eq!(h.len(), 2);
    }
    #[test]
    fn older_presentation_json_defaults_to_arrival() {
        let old: Config = serde_json::from_str(r#"{"delay_ms":50,"extrapolation_ms":0}"#).unwrap();
        assert_eq!(old.timebase, Timebase::Arrival);
        let new: Config =
            serde_json::from_str(r#"{"delay_ms":50,"extrapolation_ms":0,"timebase":"server"}"#)
                .unwrap();
        assert_eq!(new.timebase, Timebase::Server);
        assert!(
            serde_json::from_str::<Config>(
                r#"{"delay_ms":1,"extrapolation_ms":0,"timebase":"physics"}"#
            )
            .is_err()
        );
    }
}

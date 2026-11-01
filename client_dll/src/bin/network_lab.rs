//! Deterministic, virtual-clock receive-path lab. Uses the production packet
//! codec and interpolation; no sockets, engine, assets or game callbacks.
use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use client_dll::{
    interpolation::{
        Config, Mode, ReceiveHistory, STALE_AFTER_MS, Sample, Timebase, server_sample_time,
    },
    network_conditions::{Conditions, Counters, Link},
};
use multiplayer_server::protocol::{self, Position, Rotation, ServerUpdate, TICK_HZ, VERSION};
use serde::Serialize;
use std::{io::Write, path::PathBuf};

#[derive(Clone, Debug, Parser, Serialize)]
struct Parameters {
    /// Number of synthetic 60 Hz server poses (at least 120).
    #[arg(long, default_value_t = 3600, value_parser = clap::value_parser!(u32).range(120..=1000000))]
    packets: u32,
    #[command(flatten)]
    #[serde(flatten)]
    conditions: Conditions,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(0..=200))]
    interpolation_ms: u16,
    #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u16).range(0..=200))]
    extrapolation_ms: u16,
    #[arg(long, value_enum, default_value = "linear")]
    trajectory: Trajectory,
    /// Stamp samples at local arrival or at the (synthetic) server send time.
    #[arg(long, value_enum, default_value = "arrival")]
    timebase: TimebaseArg,
    /// Error of the simulated server-minus-local offset estimate (server timebase).
    #[arg(long, default_value_t = 0, allow_hyphen_values = true, value_parser = clap::value_parser!(i16).range(-200..=200))]
    offset_error_ms: i16,
    /// Add this error to the clock estimate at offset-step-at-ms (server mode).
    #[arg(long, default_value_t = 0, allow_hyphen_values = true, value_parser = clap::value_parser!(i16).range(-200..=200))]
    offset_step_ms: i16,
    #[arg(long, default_value_t = 5000, value_parser = clap::value_parser!(u32).range(1..))]
    offset_step_at_ms: u32,
    /// A new report file; its parent directory must already exist.
    #[arg(long)]
    #[serde(skip)]
    output: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
enum TimebaseArg {
    Arrival,
    Server,
}
#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
enum Trajectory {
    Linear,
    StopTurn,
    Circular,
}

/// Own piecewise path, with two stops and changes of direction every 8 s.
/// No game units/physics/tick convention is inferred from this synthetic path.
fn truth(kind: Trajectory, milliseconds: f64) -> Position {
    let [x, z] = match kind {
        Trajectory::Linear => [milliseconds * 0.6, 100.0],
        Trajectory::StopTurn => {
            let t = milliseconds % 8000.0;
            if t < 2000.0 {
                [t * 0.6, 100.0]
            } else if t < 3500.0 {
                [1200.0, 100.0]
            } else if t < 5500.0 {
                [1200.0, 100.0 + (t - 3500.0) * 0.6]
            } else if t < 7000.0 {
                [1200.0, 1300.0]
            } else {
                [1200.0 - (t - 7000.0) * 1.2, 1300.0 - (t - 7000.0) * 1.2]
            }
        }
        Trajectory::Circular => {
            let angle = milliseconds * 0.0004;
            [1500.0 * angle.sin(), 100.0 + 1500.0 * (1.0 - angle.cos())]
        }
    };
    Position {
        x: x as f32,
        y: 0.0,
        z: z as f32,
    }
}

/// Own synthetic Y-axis turn, unrelated to the game's axis convention. Alternate
/// quaternion signs on the wire to exercise equivalent q/-q under packet loss.
fn rotation(kind: Trajectory, milliseconds: f64) -> [f32; 4] {
    let half = if matches!(kind, Trajectory::Circular) {
        milliseconds * 0.0002
    } else {
        0.0
    };
    [0.0, half.sin() as f32, 0.0, half.cos() as f32]
}
fn angular_error(a: [f32; 4], b: [f32; 4]) -> u64 {
    let norm = |q: [f32; 4]| {
        q.into_iter()
            .map(|v| f64::from(v).powi(2))
            .sum::<f64>()
            .sqrt()
    };
    let dot = a
        .into_iter()
        .zip(b)
        .map(|(a, b)| f64::from(a) * f64::from(b))
        .sum::<f64>();
    (2.0 * (dot.abs() / (norm(a) * norm(b))).clamp(0.0, 1.0).acos() * 180_000.0
        / std::f64::consts::PI)
        .round() as u64
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Scenario {
    name: &'static str,
    virtual_duration_ms: u64,
    generated: u32,
    blackout_start_ms: Option<u64>,
    blackout_duration_ms: u64,
    blackout_loss: u64,
    network: Counters,
    accepted: u64,
    duplicates_or_old: u64,
    last_sequence: Option<u64>,
    rendered_frames: u64,
    stale_hidden_frames: u64,
    max_sample_age_ms: u64,
    resumed_after_blackout: bool,
    final_queue: usize,
    finite_poses: bool,
    modes: Modes,
    max_prediction_ms: u16,
    /// Error against the selected synthetic path, not real game physics.
    mean_error_milli_cm: u64,
    max_error_milli_cm: u64,
    /// Mean |error(frame) - error(previous frame)|: wobble independent of a
    /// constant presentation lag. Synthetic path only.
    mean_error_step_milli_cm: u64,
    /// Shortest orientation difference against the current synthetic trajectory.
    /// Includes intentional presentation delay; it is not an engine-axis proof.
    mean_rotation_error_millideg: u64,
    max_rotation_error_millideg: u64,
    max_rotation_norm_error_ppm: u64,
    clock_step_observed: bool,
    /// Auxiliary forecast of the constant linear path 50 ms after each newest
    /// sample, with delay 0/extrapolation 50 independent of rendering budgets.
    /// None for nonlinear paths or before two samples; not game physics error.
    prediction_probe50_max_error_milli_cm: Option<u64>,
    prediction_probe50_samples: u64,
}

#[derive(Default, Debug, PartialEq, Eq, Serialize)]
struct Modes {
    buffering: u64,
    interpolated: u64,
    predicted: u64,
    capped: u64,
    held: u64,
    snapped: u64,
}
impl Modes {
    fn add(&mut self, mode: Mode) {
        match mode {
            Mode::Buffering => self.buffering += 1,
            Mode::Interpolated => self.interpolated += 1,
            Mode::Predicted => self.predicted += 1,
            Mode::Capped => self.capped += 1,
            Mode::Held => self.held += 1,
            Mode::Snapped => self.snapped += 1,
        }
    }
    fn total(&self) -> u64 {
        self.buffering + self.interpolated + self.predicted + self.capped + self.held + self.snapped
    }
}

fn simulate(name: &'static str, parameters: &Parameters, blackout: bool) -> Result<Scenario> {
    simulate_clock(name, parameters, blackout, true)
}
fn simulate_clock(
    name: &'static str,
    parameters: &Parameters,
    blackout: bool,
    align_clock: bool,
) -> Result<Scenario> {
    let mut link = Link::new(parameters.conditions.clone()).map_err(anyhow::Error::msg)?;
    let duration = u64::from(parameters.packets - 1) * 1000 / u64::from(TICK_HZ);
    // Keep enough poses on both sides of the outage, including the short lab.
    let outage_start = duration / 3;
    let outage_length = (duration / 3).min(1500);
    let outage_end = outage_start + outage_length;
    let end = duration
        + u64::from(parameters.conditions.delay_ms + parameters.conditions.jitter_ms)
        + 250;
    let mut report = Scenario {
        name,
        virtual_duration_ms: end,
        generated: parameters.packets,
        blackout_start_ms: blackout.then_some(outage_start),
        blackout_duration_ms: if blackout { outage_length } else { 0 },
        blackout_loss: 0,
        network: Counters::default(),
        accepted: 0,
        duplicates_or_old: 0,
        last_sequence: None,
        rendered_frames: 0,
        stale_hidden_frames: 0,
        max_sample_age_ms: 0,
        resumed_after_blackout: false,
        final_queue: 0,
        finite_poses: true,
        modes: Modes::default(),
        max_prediction_ms: 0,
        mean_error_milli_cm: 0,
        max_error_milli_cm: 0,
        mean_error_step_milli_cm: 0,
        mean_rotation_error_millideg: 0,
        max_rotation_error_millideg: 0,
        max_rotation_norm_error_ppm: 0,
        clock_step_observed: false,
        prediction_probe50_max_error_milli_cm: None,
        prediction_probe50_samples: 0,
    };
    let config = Config {
        delay_ms: parameters.interpolation_ms,
        extrapolation_ms: parameters.extrapolation_ms,
        timebase: match parameters.timebase {
            TimebaseArg::Arrival => Timebase::Arrival,
            TimebaseArg::Server => Timebase::Server,
        },
    };
    config.validate().map_err(anyhow::Error::msg)?;
    let mut total_error = 0u64;
    let mut total_rotation_error = 0u64;
    let (mut previous_error, mut total_step, mut steps) = (None::<u64>, 0u64, 0u64);
    let mut history = ReceiveHistory::default();
    let mut seen = None;
    let mut sequence = 0;
    for now in 0..=end {
        if sequence < parameters.packets && now >= u64::from(sequence) * 1000 / u64::from(TICK_HZ) {
            if blackout && (outage_start..outage_end).contains(&now) {
                report.blackout_loss += 1;
            } else {
                let q = rotation(
                    parameters.trajectory,
                    f64::from(sequence) * 1000.0 / f64::from(TICK_HZ),
                );
                let sign = if matches!(parameters.trajectory, Trajectory::Circular)
                    && (sequence / 30) % 2 == 1
                {
                    -1.0
                } else {
                    1.0
                };
                let bytes = protocol::encode(&ServerUpdate {
                    version: VERSION,
                    player_id: 1,
                    sequence: u64::from(sequence),
                    position: truth(
                        parameters.trajectory,
                        f64::from(sequence) * 1000.0 / f64::from(TICK_HZ),
                    ),
                    rotation: Some(Rotation {
                        x: q[0] * sign,
                        y: q[1] * sign,
                        z: q[2] * sign,
                        w: q[3] * sign,
                    }),
                })
                .map_err(anyhow::Error::msg)?;
                link.submit(now, &bytes).map_err(anyhow::Error::msg)?;
            }
            sequence += 1;
        }
        while let Some(bytes) = link.pop(now) {
            let update = protocol::decode_server(&bytes)
                .ok_or_else(|| anyhow::anyhow!("generated packet failed production decoder"))?;
            // Same strict ordering rule as the worker. This lab does not invoke
            // the worker, its mutexes, sockets or engine integration.
            if report
                .last_sequence
                .is_some_and(|last| update.sequence <= last)
            {
                report.duplicates_or_old += 1;
                continue;
            }
            // Production stamping: the lab server sends at generation time on the
            // same virtual clock, so the true offset is zero plus the configured error.
            let arrival = now as f64 / 1000.0;
            let stepped =
                parameters.offset_step_ms != 0 && now >= u64::from(parameters.offset_step_at_ms);
            let offset = i64::from(parameters.offset_error_ms)
                + if stepped {
                    i64::from(parameters.offset_step_ms)
                } else {
                    0
                };
            let received_ms = update.sequence * 1000 / u64::from(TICK_HZ);
            let stamped = match config.timebase {
                Timebase::Arrival => None,
                Timebase::Server => server_sample_time(arrival, now, received_ms, offset),
            };
            let q = update.rotation.expect("own packets include rotation");
            report.clock_step_observed |= stepped && stamped.is_some();
            let sample = Sample {
                at: stamped.unwrap_or(arrival),
                position: [update.position.x, update.position.y, update.position.z],
                rotation: [q.x, q.y, q.z, q.w],
            };
            let accepted = if stamped.is_some() && align_clock {
                history.receive_server(sample, arrival, received_ms)
            } else {
                history.receive(
                    sample,
                    arrival,
                    if stamped.is_some() {
                        Timebase::Server
                    } else {
                        Timebase::Arrival
                    },
                )
            };
            ensure!(accepted, "valid packet refused by receive history");
            if matches!(parameters.trajectory, Trajectory::Linear) && history.len() >= 2 {
                let newest = history.newest().unwrap();
                let probe = history
                    .evaluate(
                        newest.at + 0.05,
                        Config {
                            delay_ms: 0,
                            extrapolation_ms: 50,
                            ..config
                        },
                    )
                    .unwrap();
                let error = ((f64::from(probe.position[0]) - f64::from(newest.position[0]) - 30.0)
                    .abs()
                    * 1000.0)
                    .round() as u64;
                report.prediction_probe50_max_error_milli_cm = Some(
                    report
                        .prediction_probe50_max_error_milli_cm
                        .unwrap_or(0)
                        .max(error),
                );
                report.prediction_probe50_samples += 1;
            }
            seen = Some(now);
            report.last_sequence = Some(update.sequence);
            report.accepted += 1;
            if blackout && update.sequence * 1000 / u64::from(TICK_HZ) >= outage_end {
                report.resumed_after_blackout = true;
            }
        }
        // Render at 60 Hz as well, sampled against the same virtual clock.
        if now * u64::from(TICK_HZ) / 1000 == now.saturating_sub(1) * u64::from(TICK_HZ) / 1000
            && now != 0
        {
            continue;
        }
        if let Some(age) = seen.map(|seen| now - seen) {
            report.max_sample_age_ms = report.max_sample_age_ms.max(age);
            if age >= STALE_AFTER_MS {
                report.stale_hidden_frames += 1;
                continue;
            }
            if let Some(pose) = history.evaluate(now as f64 / 1000.0, config) {
                report.finite_poses &= pose
                    .position
                    .into_iter()
                    .chain(pose.rotation)
                    .all(f32::is_finite);
                report.modes.add(pose.mode);
                report.max_prediction_ms = report.max_prediction_ms.max(pose.prediction_ms);
                let angle =
                    angular_error(pose.rotation, rotation(parameters.trajectory, now as f64));
                total_rotation_error += angle;
                report.max_rotation_error_millideg = report.max_rotation_error_millideg.max(angle);
                let norm = pose
                    .rotation
                    .into_iter()
                    .map(|v| f64::from(v).powi(2))
                    .sum::<f64>()
                    .sqrt();
                report.max_rotation_norm_error_ppm = report
                    .max_rotation_norm_error_ppm
                    .max(((norm - 1.0).abs() * 1_000_000.0).round() as u64);
                let actual = truth(parameters.trajectory, now as f64);
                let error = (pose
                    .position
                    .into_iter()
                    .zip([actual.x, actual.y, actual.z])
                    .map(|(a, b)| (f64::from(a) - f64::from(b)).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    * 1000.0)
                    .round() as u64;
                total_error += error;
                if let Some(previous) = previous_error {
                    total_step += error.abs_diff(previous);
                    steps += 1;
                }
                previous_error = Some(error);
                report.max_error_milli_cm = report.max_error_milli_cm.max(error);
                report.rendered_frames += 1;
            }
        }
    }
    report.network = link.counters();
    report.mean_error_milli_cm = total_error.checked_div(report.rendered_frames).unwrap_or(0);
    report.mean_rotation_error_millideg = total_rotation_error
        .checked_div(report.rendered_frames)
        .unwrap_or(0);
    report.mean_error_step_milli_cm = total_step.checked_div(steps).unwrap_or(0);
    ensure!(
        report.modes.total() == report.rendered_frames,
        "presentation mode accounting mismatch"
    );
    report.final_queue = link.pending();
    ensure!(
        report.final_queue == 0 && report.finite_poses,
        "queue did not drain or pose became invalid"
    );
    ensure!(
        report.accepted + report.duplicates_or_old == report.network.emitted,
        "receive accounting mismatch"
    );
    ensure!(
        report.network.submitted + report.blackout_loss == u64::from(report.generated),
        "generation accounting mismatch"
    );
    ensure!(
        report.network.emitted + report.network.queue_overflow
            == report.network.submitted - report.network.random_loss
                + report.network.duplicate_copies,
        "link accounting mismatch"
    );
    Ok(report)
}

#[derive(Serialize)]
struct Report<'a> {
    schema: u32,
    virtual_clock: bool,
    game_tested: bool,
    lan_tested: bool,
    scope: &'static str,
    parameters: &'a Parameters,
    clean: Scenario,
    impaired: Scenario,
    blackout: Scenario,
    /// Same synthetic network/step with the previous per-sample clock clamping.
    /// Reference only, never a selectable mode in the actual DLL.
    #[serde(skip_serializing_if = "Option::is_none")]
    clock_step_reference: Option<Scenario>,
}
fn report(parameters: &Parameters) -> Result<Report<'_>> {
    let mut clean = parameters.clone();
    clean.conditions.loss_permille = 0;
    clean.conditions.duplicate_permille = 0;
    clean.conditions.delay_ms = 0;
    clean.conditions.jitter_ms = 0;
    Ok(Report {
        schema: 5,
        virtual_clock: true,
        game_tested: false,
        lan_tested: false,
        scope: "synthetic server packets / production decoder and interpolation / simulated receive ordering",
        parameters,
        clean: simulate("clean", &clean, false)?,
        impaired: simulate("impaired", parameters, false)?,
        blackout: simulate("blackout", parameters, true)?,
        clock_step_reference: (matches!(parameters.timebase, TimebaseArg::Server)
            && parameters.offset_step_ms != 0)
            .then(|| simulate_clock("unaligned_clock_reference", parameters, false, false))
            .transpose()?,
    })
}
fn main() -> Result<()> {
    let parameters = Parameters::parse();
    let bytes = serde_json::to_vec_pretty(&report(&parameters)?)?;
    if let Some(output) = &parameters.output {
        std::fs::File::create_new(output)?.write_all(&bytes)?;
    }
    std::io::stdout().write_all(&bytes)?;
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parameters() -> Parameters {
        Parameters::try_parse_from(["network_lab", "--packets", "600"]).unwrap()
    }
    #[test]
    fn angular_measurement_uses_short_arc_and_ignores_quaternion_sign() {
        let q = rotation(Trajectory::Circular, 1234.0);
        assert_eq!(angular_error(q, q.map(|v| -v)), 0);
        assert_eq!(
            angular_error([0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.0]),
            180_000
        );
        // 0.4 rad/s for one second, independent of packet sign or norm noise.
        assert_eq!(
            angular_error(
                rotation(Trajectory::Circular, 0.0),
                rotation(Trajectory::Circular, 1000.0)
            ),
            22_918
        );
    }
    #[test]
    fn circular_rotation_survives_sign_changes_loss_and_stale_return_without_long_spins() {
        let mut p = parameters();
        p.trajectory = Trajectory::Circular;
        for timebase in [TimebaseArg::Arrival, TimebaseArg::Server] {
            p.timebase = timebase;
            let r = report(&p).unwrap();
            for scenario in [&r.clean, &r.impaired, &r.blackout] {
                assert!(scenario.finite_poses);
                assert!(scenario.max_rotation_norm_error_ppm <= 2);
                assert!(
                    scenario.max_rotation_error_millideg < 30_000,
                    "{scenario:?}"
                );
                assert!(scenario.mean_rotation_error_millideg > 0);
                assert!(scenario.max_prediction_ms <= p.extrapolation_ms);
            }
            assert!(r.blackout.stale_hidden_frames > 0 && r.blackout.resumed_after_blackout);
        }
    }
    #[test]
    fn repeatable_loss_jitter_duplication_and_reordering_use_real_codec_and_interpolation() {
        let p = parameters();
        let a = report(&p).unwrap();
        let b = report(&p).unwrap();
        assert_eq!(
            serde_json::to_vec(&a).unwrap(),
            serde_json::to_vec(&b).unwrap()
        );
        assert_eq!(a.clean.accepted, 600);
        assert_eq!(a.clean.duplicates_or_old, 0);
        assert_eq!(a.clean.network.random_loss, 0);
        assert!(a.impaired.network.random_loss > 0);
        assert!(a.impaired.network.duplicate_copies > 0);
        assert!(a.impaired.network.reordered > 0);
        assert!(a.impaired.duplicates_or_old > 0);
        assert!(a.impaired.accepted < a.clean.accepted);
        assert!(a.blackout.stale_hidden_frames > 0);
        assert!(a.blackout.resumed_after_blackout);
    }
    #[test]
    fn total_loss_never_creates_a_pose_and_small_queues_stay_bounded() {
        let mut p = parameters();
        p.conditions.loss_permille = 1000;
        let result = simulate("lost", &p, false).unwrap();
        assert_eq!(result.network.random_loss, 600);
        assert_eq!(result.accepted, 0);
        assert_eq!(result.rendered_frames, 0);
        assert_eq!(result.last_sequence, None);
        p.conditions.loss_permille = 0;
        p.conditions.duplicate_permille = 1000;
        p.conditions.delay_ms = 100;
        p.conditions.jitter_ms = 0;
        p.conditions.queue_capacity = 1;
        let result = simulate("bounded", &p, false).unwrap();
        assert!(result.network.queue_overflow > 0);
        assert_eq!(result.network.peak_queue, 1);
        assert!(result.accepted > 0);
        assert_eq!(result.final_queue, 0);
    }
    #[test]
    fn invalid_arguments_are_rejected_before_any_simulation_or_file_creation() {
        for (key, value) in [
            ("--seed", "0"),
            ("--packets", "119"),
            ("--packets", "1000001"),
            ("--loss-permille", "1001"),
            ("--duplicate-permille", "1001"),
            ("--queue-capacity", "0"),
            ("--queue-capacity", "8193"),
            ("--delay-ms", "5001"),
            ("--jitter-ms", "5001"),
            ("--interpolation-ms", "201"),
            ("--extrapolation-ms", "201"),
        ] {
            assert!(Parameters::try_parse_from(["network_lab", key, value]).is_err());
        }
    }
    #[test]
    fn smaller_delay_reduces_clean_synthetic_lag_but_uses_prediction_more() {
        let baseline = parameters();
        let delayed = report(&baseline).unwrap();
        let mut immediate = parameters();
        immediate.interpolation_ms = 0;
        immediate.extrapolation_ms = 50;
        let fast = report(&immediate).unwrap();
        assert!(fast.clean.mean_error_milli_cm < delayed.clean.mean_error_milli_cm);
        assert!(
            fast.impaired.modes.predicted + fast.impaired.modes.capped
                > delayed.impaired.modes.predicted + delayed.impaired.modes.capped
        );
        assert!(fast.impaired.max_prediction_ms <= 50);
        immediate.extrapolation_ms = 0;
        let held = report(&immediate).unwrap();
        assert_eq!(
            held.impaired.modes.predicted + held.impaired.modes.capped,
            0
        );
        assert!(held.impaired.modes.held > 0);
    }
    #[test]
    fn stop_turn_path_is_continuous_and_exercises_predictions_around_braking() {
        for at in [2000.0, 3500.0, 5500.0, 7000.0, 8000.0] {
            let a = truth(Trajectory::StopTurn, at - 0.001);
            let b = truth(Trajectory::StopTurn, at);
            assert!((a.x - b.x).abs() < 0.01 && (a.z - b.z).abs() < 0.01);
        }
        let mut p = parameters();
        p.trajectory = Trajectory::StopTurn;
        p.interpolation_ms = 0;
        p.extrapolation_ms = 50;
        let result = report(&p).unwrap();
        assert!(result.impaired.modes.predicted > 0 && result.impaired.modes.capped > 0);
        assert!(result.impaired.finite_poses && result.impaired.max_error_milli_cm > 0);
        assert!(result.impaired.max_prediction_ms <= 50);
    }
}
#[cfg(test)]
mod timebase_tests {
    use super::*;
    fn run(timebase: TimebaseArg, offset_error_ms: i16) -> Report<'static> {
        let mut p = Parameters::try_parse_from(["network_lab", "--packets", "1200"]).unwrap();
        p.timebase = timebase;
        p.offset_error_ms = offset_error_ms;
        let p = Box::leak(Box::new(p));
        report(p).unwrap()
    }
    #[test]
    fn server_timebase_removes_jitter_wobble_and_offset_error_only_shifts_lag() {
        let arrival = run(TimebaseArg::Arrival, 0);
        let server = run(TimebaseArg::Server, 0);
        assert_eq!(
            arrival.clean.mean_error_milli_cm,
            server.clean.mean_error_milli_cm
        );
        // Arrival stamps carry the link jitter into every frame; server stamps do not.
        assert!(
            arrival.impaired.mean_error_step_milli_cm
                > 10 * server.impaired.mean_error_step_milli_cm
        );
        assert!(
            server.impaired.mean_error_step_milli_cm
                <= 2 * server.clean.mean_error_step_milli_cm + 50
        );
        assert!(server.impaired.max_error_milli_cm < arrival.impaired.max_error_milli_cm);
        for error in [-100, 100] {
            let shifted = run(TimebaseArg::Server, error);
            assert!(shifted.impaired.finite_poses);
            assert!(
                shifted.impaired.mean_error_step_milli_cm
                    < arrival.impaired.mean_error_step_milli_cm / 5
            );
            // Positive error renders closer to now (more prediction), negative adds lag.
            if error > 0 {
                assert!(shifted.impaired.modes.predicted > server.impaired.modes.predicted);
            } else {
                assert!(shifted.impaired.mean_error_milli_cm > server.impaired.mean_error_milli_cm);
            }
        }
        assert!(Parameters::try_parse_from(["network_lab", "--offset-error-ms", "201"]).is_err());
    }
    #[test]
    fn clock_steps_do_not_turn_clock_correction_into_linear_speed() {
        for step in [-100, 100] {
            let mut p = Parameters::try_parse_from(["network_lab", "--packets", "1200"]).unwrap();
            p.timebase = TimebaseArg::Server;
            p.offset_step_ms = step;
            let r = report(&p).unwrap();
            let old = r.clock_step_reference.as_ref().unwrap();
            assert!(old.clock_step_observed);
            for s in [&r.clean, &r.impaired, &r.blackout] {
                assert!(s.clock_step_observed && s.finite_poses && s.final_queue == 0);
                assert!(s.prediction_probe50_samples > 0);
                // Source millisecond rounding gives up to ~1.25 cm error in
                // this fixed forecast; a clock step must not add a speed spike.
                assert!(
                    s.prediction_probe50_max_error_milli_cm.unwrap() < 1400,
                    "{s:?}"
                );
            }
            assert!(
                old.prediction_probe50_max_error_milli_cm.unwrap() > 10_000,
                "{old:?}"
            );
        }
    }
    #[test]
    fn clock_step_fallback_total_loss_and_invalid_arguments_keep_their_guards() {
        let mut p = Parameters::try_parse_from(["network_lab", "--packets", "600"]).unwrap();
        p.timebase = TimebaseArg::Server;
        p.offset_error_ms = 200;
        p.offset_step_ms = 200; // Future stamp becomes implausible: arrival fallback.
        p.offset_step_at_ms = 1000;
        let r = report(&p).unwrap();
        assert!(r.impaired.finite_poses && r.impaired.final_queue == 0);
        p.conditions.loss_permille = 1000;
        let lost = simulate("lost", &p, false).unwrap();
        assert_eq!(lost.prediction_probe50_max_error_milli_cm, None);
        assert_eq!(lost.prediction_probe50_samples, 0);
        for args in [["--offset-step-ms", "201"], ["--offset-step-at-ms", "0"]] {
            assert!(Parameters::try_parse_from(["network_lab", args[0], args[1]]).is_err());
        }
    }
}

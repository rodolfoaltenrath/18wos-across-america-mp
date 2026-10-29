//! Deterministic, virtual-clock receive-path lab. Uses the production packet
//! codec and interpolation; no sockets, engine, assets or game callbacks.
use anyhow::{Result, ensure};
use clap::Parser;
use client_dll::{
    interpolation::{History, Sample},
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
    /// A new report file; its parent directory must already exist.
    #[arg(long)]
    #[serde(skip)]
    output: Option<PathBuf>,
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
}

fn simulate(name: &'static str, parameters: &Parameters, blackout: bool) -> Result<Scenario> {
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
    };
    let mut history = History::default();
    let mut seen = None;
    let mut sequence = 0;
    for now in 0..=end {
        if sequence < parameters.packets && now >= u64::from(sequence) * 1000 / u64::from(TICK_HZ) {
            if blackout && (outage_start..outage_end).contains(&now) {
                report.blackout_loss += 1;
            } else {
                let bytes = protocol::encode(&ServerUpdate {
                    version: VERSION,
                    player_id: 1,
                    sequence: u64::from(sequence),
                    position: Position {
                        x: sequence as f32 * 10.0,
                        y: 0.0,
                        z: 100.0,
                    },
                    rotation: Some(Rotation {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: 1.0,
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
            history.push(Sample {
                at: now as f64 / 1000.0,
                position: [update.position.x, update.position.y, update.position.z],
                rotation: [0.0, 0.0, 0.0, 1.0],
            });
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
            if age >= 1000 {
                report.stale_hidden_frames += 1;
                continue;
            }
            if let Some((position, rotation)) = history.pose(now as f64 / 1000.0) {
                report.finite_poses &= position.into_iter().chain(rotation).all(f32::is_finite);
                report.rendered_frames += 1;
            }
        }
    }
    report.network = link.counters();
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
}
fn report(parameters: &Parameters) -> Result<Report<'_>> {
    let mut clean = parameters.clone();
    clean.conditions.loss_permille = 0;
    clean.conditions.duplicate_permille = 0;
    clean.conditions.delay_ms = 0;
    clean.conditions.jitter_ms = 0;
    Ok(Report {
        schema: 1,
        virtual_clock: true,
        game_tested: false,
        lan_tested: false,
        scope: "synthetic server packets / production decoder and interpolation / simulated receive ordering",
        parameters,
        clean: simulate("clean", &clean, false)?,
        impaired: simulate("impaired", parameters, false)?,
        blackout: simulate("blackout", parameters, true)?,
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
        ] {
            assert!(Parameters::try_parse_from(["network_lab", key, value]).is_err());
        }
    }
}

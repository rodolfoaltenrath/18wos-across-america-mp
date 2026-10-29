//! Deterministic, virtual-clock receive-path lab. Uses the production packet
//! codec and interpolation; no sockets, engine, assets or game callbacks.
use anyhow::{Result, ensure};
use clap::Parser;
use client_dll::interpolation::{History, Sample};
use multiplayer_server::protocol::{self, Position, Rotation, ServerUpdate, TICK_HZ, VERSION};
use serde::Serialize;
use std::{cmp::Reverse, collections::BinaryHeap, io::Write, path::PathBuf};

#[derive(Clone, Debug, Parser, Serialize)]
struct Parameters {
    /// Number of synthetic 60 Hz server poses (at least 120).
    #[arg(long, default_value_t = 3600, value_parser = clap::value_parser!(u32).range(120..=1000000))]
    packets: u32,
    /// Nonzero seed; same parameters produce the same events and JSON.
    #[arg(long, default_value_t = 42, value_parser = clap::value_parser!(u64).range(1..))]
    seed: u64,
    /// Random loss per thousand original packets.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(0..=1000))]
    loss_permille: u16,
    /// Probability of one extra copy per thousand surviving packets.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(0..=1000))]
    duplicate_permille: u16,
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u32).range(0..=5000))]
    delay_ms: u32,
    /// Independent jitter in [-jitter, +jitter]; total delay clamps at zero.
    #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u32).range(0..=5000))]
    jitter_ms: u32,
    #[arg(long, default_value_t = 2048, value_parser = clap::value_parser!(u32).range(1..=8192))]
    queue_capacity: u32,
    /// A new report file; its parent directory must already exist.
    #[arg(long)]
    #[serde(skip)]
    output: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
struct Counters {
    submitted: u64,
    random_loss: u64,
    duplicate_copies: u64,
    queue_overflow: u64,
    emitted: u64,
    reordered: u64,
    peak_queue: usize,
}
struct Link {
    parameters: Parameters,
    random: u64,
    serial: u64,
    last_emitted: u64,
    queue: BinaryHeap<Reverse<(u64, u64, Vec<u8>)>>,
    counts: Counters,
}
impl Link {
    fn new(parameters: &Parameters) -> Self {
        Self {
            parameters: parameters.clone(),
            random: parameters.seed,
            serial: 0,
            last_emitted: 0,
            queue: BinaryHeap::new(),
            counts: Counters::default(),
        }
    }
    fn random(&mut self) -> u64 {
        // xorshift64, for reproducible impairment only (not cryptography).
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.random = x;
        x
    }
    fn enqueue(&mut self, now: u64, bytes: &[u8]) {
        if self.queue.len() == self.parameters.queue_capacity as usize {
            self.counts.queue_overflow += 1;
            return;
        }
        let jitter = i64::from(self.parameters.jitter_ms);
        let offset = (self.random() % (2 * jitter + 1) as u64) as i64 - jitter;
        let delay = (i64::from(self.parameters.delay_ms) + offset).max(0) as u64;
        self.serial += 1;
        self.queue
            .push(Reverse((now + delay, self.serial, bytes.to_vec())));
        self.counts.peak_queue = self.counts.peak_queue.max(self.queue.len());
    }
    fn submit(&mut self, now: u64, bytes: &[u8]) {
        self.counts.submitted += 1;
        if self.random() % 1000 < u64::from(self.parameters.loss_permille) {
            self.counts.random_loss += 1;
            return;
        }
        self.enqueue(now, bytes);
        if self.random() % 1000 < u64::from(self.parameters.duplicate_permille) {
            self.counts.duplicate_copies += 1;
            self.enqueue(now, bytes);
        }
    }
    fn pop(&mut self, now: u64) -> Option<Vec<u8>> {
        if self.queue.peek()?.0.0 > now {
            return None;
        }
        let Reverse((_, serial, bytes)) = self.queue.pop()?;
        self.counts.emitted += 1;
        if serial < self.last_emitted {
            self.counts.reordered += 1;
        }
        self.last_emitted = self.last_emitted.max(serial);
        Some(bytes)
    }
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
    let mut link = Link::new(parameters);
    let duration = u64::from(parameters.packets - 1) * 1000 / u64::from(TICK_HZ);
    // Keep enough poses on both sides of the outage, including the short lab.
    let outage_start = duration / 3;
    let outage_length = (duration / 3).min(1500);
    let outage_end = outage_start + outage_length;
    let end = duration + u64::from(parameters.delay_ms + parameters.jitter_ms) + 250;
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
                link.submit(now, &bytes);
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
    report.network = link.counts;
    report.final_queue = link.queue.len();
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
    clean.loss_permille = 0;
    clean.duplicate_permille = 0;
    clean.delay_ms = 0;
    clean.jitter_ms = 0;
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
        p.loss_permille = 1000;
        let result = simulate("lost", &p, false).unwrap();
        assert_eq!(result.network.random_loss, 600);
        assert_eq!(result.accepted, 0);
        assert_eq!(result.rendered_frames, 0);
        assert_eq!(result.last_sequence, None);
        p.loss_permille = 0;
        p.duplicate_permille = 1000;
        p.delay_ms = 100;
        p.jitter_ms = 0;
        p.queue_capacity = 1;
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

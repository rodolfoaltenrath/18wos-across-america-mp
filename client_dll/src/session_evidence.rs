//! Bounded offline analysis of our own session reports. No process/game access.
//! Unknown fields (including paths, arguments and tokens) never reach output.
use anyhow::{Context, Result, ensure};
use multiplayer_server::{
    session_client::{Phase, Status},
    session_timing::Timing,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_INPUTS: usize = 256;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Client {
    pub status: u32,
    pub sample_state: u32,
    pub sent: u64,
    pub received: u64,
    #[serde(default)]
    pub read_failures: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Draw {
    pub draw_calls: u32,
    pub flush_calls: u32,
    pub foreign_calls: u32,
    pub flags: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Visual {
    pub item: u32,
    pub shown: u32,
    pub drawn: u32,
    pub fault: u32,
    pub skinned_parts: Option<u32>,
    pub draw_cost_avg_ns: Option<u32>,
    pub draw_cost_max_ns: Option<u32>,
    pub frame_interval_avg_us: Option<u32>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Remote {
    pub player_id: u64,
    pub age_ms: u32,
    pub has_rotation: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub client: Client,
    pub draw: Draw,
    pub visual: Visual,
    pub network_session: Option<Status>,
    pub network_timing: Option<Timing>,
    pub presentation: crate::presentation::Status,
    pub remotes: Vec<Remote>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.client.status <= 3 && self.client.sample_state <= 3,
            "invalid client state"
        );
        ensure!(
            self.visual.shown <= 1 && self.remotes.len() <= 64,
            "invalid visual/remotes"
        );
        self.presentation
            .config
            .validate()
            .map_err(anyhow::Error::msg)?;
        if let Some(s) = &self.network_session {
            ensure!(s.protocol == 2, "unsupported session protocol");
        }
        Ok(())
    }
    pub fn findings(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self
            .network_session
            .as_ref()
            .is_some_and(|s| s.phase == Phase::Connected)
        {
            if self.client.sample_state == 2 {
                out.push("pose_unavailable_while_connected");
            }
            if self.remotes.is_empty() {
                out.push("connected_without_remote_pose");
            }
        }
        if self.client.status == 2 && self.visual.item != 0 {
            out.push("stopped_worker_with_retained_visual");
        }
        if self.visual.fault != 0 {
            out.push("visual_fault_reported");
        }
        if self.draw.flags != 0 || self.draw.foreign_calls != 0 {
            out.push("draw_context_anomaly_reported");
        }
        if self.network_timing.is_some_and(|s| s.tick_gap_events > 0) {
            out.push("snapshot_tick_gaps_do_not_estimate_packet_loss");
        }
        if self.client.read_failures.is_some_and(|v| v > 0) {
            out.push("sample_unavailability_counter_is_not_memory_read_failure_count");
        }
        out
    }
}

#[derive(Deserialize, Serialize)]
struct Slot {
    actual: String,
    expected: String,
    restored: bool,
}
#[derive(Deserialize, Serialize)]
struct Files {
    unchanged: u64,
}
#[derive(Deserialize, Serialize)]
struct Stop {
    client_status: u32,
    released: bool,
    files: Option<Files>,
    slots: Vec<Slot>,
    preflight_checks: u64,
    release_attempts: u64,
}
#[derive(Debug, Serialize)]
pub struct StopSummary {
    pub worker_stopped: bool,
    pub released: bool,
    pub reported_slots: usize,
    pub matching_slots: usize,
    pub all_reported_slots_match: bool,
    pub unchanged_files_reported: Option<u64>,
    pub preflight_checks: u64,
    pub release_attempts: u64,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capture {
    Snapshot {
        input_sha256: String,
        snapshot: Box<Snapshot>,
        findings: Vec<&'static str>,
    },
    Stop {
        input_sha256: String,
        summary: StopSummary,
    },
}
pub fn parse(bytes: &[u8]) -> Result<Capture> {
    ensure!(bytes.len() <= MAX_INPUT_BYTES, "report exceeds 1 MiB");
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .context("report must be complete JSON; pass the command's stdout report")?;
    let input_sha256 = format!("{:x}", Sha256::digest(bytes));
    if value.get("client").is_some() {
        let snapshot: Snapshot = serde_json::from_value(value)?;
        snapshot.validate()?;
        let findings = snapshot.findings();
        Ok(Capture::Snapshot {
            input_sha256,
            snapshot: Box::new(snapshot),
            findings,
        })
    } else {
        let stop: Stop = serde_json::from_value(value).context("expected session-status/session-stop; identity, backups and unrelated files are not reports")?;
        ensure!(stop.slots.len() <= 64, "too many slots");
        let mut matching_slots = 0;
        for s in &stop.slots {
            let address = |v: &str| -> Result<u64> {
                ensure!(v.len() <= 18, "invalid slot address");
                Ok(u64::from_str_radix(
                    v.strip_prefix("0x").context("missing hex prefix")?,
                    16,
                )?)
            };
            if s.restored && address(&s.actual)? == address(&s.expected)? {
                matching_slots += 1;
            }
        }
        Ok(Capture::Stop {
            input_sha256,
            summary: StopSummary {
                worker_stopped: stop.client_status == 2,
                released: stop.released,
                reported_slots: stop.slots.len(),
                matching_slots,
                all_reported_slots_match: !stop.slots.is_empty()
                    && matching_slots == stop.slots.len(),
                unchanged_files_reported: stop.files.map(|f| f.unchanged),
                preflight_checks: stop.preflight_checks,
                release_attempts: stop.release_attempts,
            },
        })
    }
}

/// Export only typed report fields; never copies input paths, diagnostic text,
/// poses, save names, arbitrary JSON or credentials from a whole data folder.
pub fn sanitize(bytes: &[u8]) -> Result<Vec<u8>> {
    match parse(bytes)? {
        Capture::Snapshot { snapshot, .. } => Ok(serde_json::to_vec_pretty(&snapshot)?),
        Capture::Stop { .. } => Ok(serde_json::to_vec_pretty(&serde_json::from_slice::<Stop>(
            bytes,
        )?)?),
    }
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub before: usize,
    pub after: usize,
    pub state: &'static str,
    pub sent_delta: Option<u64>,
    pub received_delta: Option<u64>,
    pub drawn_delta: Option<u64>,
    pub source_transition: Option<&'static str>,
}
fn compare(before: usize, a: &Snapshot, after: usize, b: &Snapshot) -> Comparison {
    let mut comparison = Comparison {
        before,
        after,
        state: "epoch_unavailable",
        sent_delta: None,
        received_delta: None,
        drawn_delta: None,
        source_transition: None,
    };
    let (Some(old), Some(new)) = (&a.network_session, &b.network_session) else {
        return comparison;
    };
    if old.session == 0 || old.connection == 0 || old.player == 0 {
        return comparison;
    }
    if (old.session, old.connection, old.player) != (new.session, new.connection, new.player) {
        comparison.state = "connection_epoch_changed";
        return comparison;
    }
    let (Some(sent), Some(received), Some(drawn)) = (
        b.client.sent.checked_sub(a.client.sent),
        b.client.received.checked_sub(a.client.received),
        b.visual.drawn.checked_sub(a.visual.drawn),
    ) else {
        comparison.state = "counter_regression_or_wrap";
        return comparison;
    };
    comparison.state = "same_connection_epoch";
    comparison.sent_delta = Some(sent);
    comparison.received_delta = Some(received);
    comparison.drawn_delta = Some(u64::from(drawn));
    comparison.source_transition = match (a.client.sample_state, b.client.sample_state) {
        (1, 2) => Some("source_became_unavailable"),
        (2, 1) => Some("source_recovered"),
        (_, 0) if a.client.sample_state != 0 => Some("source_stopped"),
        _ => None,
    };
    comparison
}
#[derive(Debug, Serialize)]
pub struct Analysis {
    pub schema: u8,
    pub analysis_only: bool,
    pub focus_measured: bool,
    pub packet_loss_estimated: bool,
    pub counters_sampled_atomically: bool,
    pub captures: Vec<Capture>,
    pub comparisons: Vec<Comparison>,
    pub limitations: [&'static str; 7],
}
pub fn analyze(captures: Vec<Capture>) -> Result<Analysis> {
    ensure!(
        !captures.is_empty() && captures.len() <= MAX_INPUTS,
        "expected 1..=256 reports in explicit capture order from ONE peer"
    );
    let mut comparisons = Vec::new();
    let mut previous = None;
    for (index, capture) in captures.iter().enumerate() {
        if let Capture::Snapshot { snapshot, .. } = capture {
            if let Some((ordinal, old)) = previous {
                comparisons.push(compare(ordinal, old, index, snapshot));
            }
            previous = Some((index, snapshot.as_ref()));
        }
    }
    Ok(Analysis {
        schema: 1,
        analysis_only: true,
        focus_measured: false,
        packet_loss_estimated: false,
        counters_sampled_atomically: false,
        captures,
        comparisons,
        limitations: [
            "input_order_is_supplied_by_the_operator_not_a_synchronized_clock",
            "snapshot_gaps_do_not_mean_silence_of_all_datagrams_or_packet_loss",
            "source_unavailable_does_not_prove_os_focus_causality",
            "rtt_is_to_this_clients_server_not_visual_end_to_end_latency",
            "draw_cost_is_not_total_frame_time_or_incremental_mod_cost",
            "stop_reports_are_not_independent_live_slot_or_hash_reverification",
            "analysis_does_not_run_the_game_or_validate_automatic_load",
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn own_snapshot(sample: u32) -> Value {
        json!({"client":{"status":1,"sample_state":sample,"sent":100,"received":80,"diagnostic":"private text"},
            "draw":{"draw_calls":90,"flush_calls":360,"foreign_calls":0,"flags":0},
            "visual":{"item":123,"shown":1,"drawn":50,"fault":0},
            "network_session":{"protocol":2,"phase":"connected","session":1,"connection":2,"player":3,"reconnects":0,"accepted":1,"old":0,"invalid":0,"rtt_ms":17,"best_rtt_ms":16,"clock_offset_ms":null,"refusal":null,"leave_confirmed":false},
            "network_timing":{"rtt_samples":20,"rtt_max_ms":34,"rtt_jitter_us":1000,"snapshots":50,"arrival_jitter_us":500,"max_receive_gap_ms":29933,"tick_gap_events":4,"largest_tick_step":1796,"nonmonotonic_snapshots":0},
            "presentation":{"config":{"delay_ms":100,"extrapolation_ms":200},"revision":1,"buffering":0,"interpolated":50,"predicted":0,"capped":0,"held":0,"snapped":0,"stale":1,"empty_queries":0,"lock_misses":0},
            "remotes":[{"player_id":4,"age_ms":72,"has_rotation":true,"position":[1,2,3]}],"identity":"private identity","token":"private token"})
    }
    fn capture(v: &Value) -> Capture {
        parse(&serde_json::to_vec(v).unwrap()).unwrap()
    }
    #[test]
    fn reproduces_connected_source_unavailability_and_recovery_without_claiming_focus_or_loss() {
        let a = own_snapshot(1);
        let mut b = own_snapshot(2);
        b["client"]["sent"] = 110.into();
        let mut c = own_snapshot(1);
        c["client"]["sent"] = 120.into();
        let r = analyze(vec![capture(&a), capture(&b), capture(&c)]).unwrap();
        assert_eq!(
            r.comparisons[0].source_transition,
            Some("source_became_unavailable")
        );
        assert_eq!(r.comparisons[1].source_transition, Some("source_recovered"));
        assert!(!r.focus_measured && !r.packet_loss_estimated);
        let output = serde_json::to_string(&r).unwrap();
        assert!(output.contains("pose_unavailable_while_connected"));
        for private in [
            "private text",
            "private identity",
            "private token",
            "position",
        ] {
            assert!(!output.contains(private));
        }
        let exported = sanitize(&serde_json::to_vec(&b).unwrap()).unwrap();
        let exported_text = std::str::from_utf8(&exported).unwrap();
        assert!(!exported_text.contains("private") && !exported_text.contains("position"));
        assert!(matches!(
            parse(&exported).unwrap(),
            Capture::Snapshot { .. }
        ));
    }
    #[test]
    fn refuses_to_invent_deltas_after_epoch_change_counter_decrease_or_legacy_input() {
        let a = own_snapshot(1);
        let mut b = a.clone();
        b["network_session"]["connection"] = 9.into();
        let report = analyze(vec![capture(&a), capture(&b)]).unwrap();
        assert_eq!(report.comparisons[0].state, "connection_epoch_changed");
        assert_eq!(report.comparisons[0].sent_delta, None);
        b = a.clone();
        b["visual"]["drawn"] = 1.into();
        assert_eq!(
            analyze(vec![capture(&a), capture(&b)]).unwrap().comparisons[0].state,
            "counter_regression_or_wrap"
        );
        b["network_session"] = Value::Null;
        assert_eq!(
            analyze(vec![capture(&a), capture(&b)]).unwrap().comparisons[0].state,
            "epoch_unavailable"
        );
    }
    #[test]
    fn checks_stop_slot_values_instead_of_trusting_restored_flag() {
        let v = json!({"client_status":2,"released":true,"files":{"unchanged":8},"preflight_checks":1,"release_attempts":1,
            "slots":[{"actual":"0x10","expected":"0x20","restored":true}],"identity":"private"});
        let Capture::Stop { summary, .. } = capture(&v) else {
            panic!()
        };
        assert!(!summary.all_reported_slots_match);
        assert_eq!(summary.matching_slots, 0);
        assert_eq!(summary.unchanged_files_reported, Some(8));
    }
    #[test]
    fn rejects_unrelated_oversized_truncated_and_out_of_range_reports() {
        for v in [
            json!({"identity":"private"}),
            json!({"files":[]}),
            json!({"client":{}}),
        ] {
            assert!(parse(&serde_json::to_vec(&v).unwrap()).is_err());
        }
        assert!(parse(b"{\"client\":").is_err());
        assert!(parse(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
        let mut v = own_snapshot(1);
        v["client"]["sample_state"] = 99.into();
        assert!(parse(&serde_json::to_vec(&v).unwrap()).is_err());
        assert!(analyze(vec![]).is_err());
    }
}

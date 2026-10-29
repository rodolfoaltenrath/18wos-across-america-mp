//! Local presentation contracts with own samples only; no game/assets needed.
use client_dll::{
    api::{self, Control},
    interpolation::{Config, History, Mode, Sample},
    presentation::Status,
};
fn history() -> History {
    let mut h = History::default();
    for (at, x) in [(1.0, 0.0), (1.1, 100.0)] {
        h.push(Sample {
            at,
            position: [x, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
    }
    h
}
#[test]
fn explicit_delay_and_prediction_budgets_have_distinct_bounded_results() {
    let h = history();
    for (config, expected, mode, prediction) in [
        (Config::DEFAULT, 50.0, Mode::Interpolated, 0),
        (
            Config {
                delay_ms: 0,
                extrapolation_ms: 200,
            },
            150.0,
            Mode::Predicted,
            50,
        ),
        (
            Config {
                delay_ms: 0,
                extrapolation_ms: 20,
            },
            120.0,
            Mode::Capped,
            20,
        ),
        (
            Config {
                delay_ms: 0,
                extrapolation_ms: 0,
            },
            100.0,
            Mode::Held,
            0,
        ),
        (
            Config {
                delay_ms: 200,
                extrapolation_ms: 200,
            },
            0.0,
            Mode::Buffering,
            0,
        ),
    ] {
        let pose = h.evaluate(1.15, config).unwrap();
        assert!((pose.position[0] - expected).abs() < 0.001);
        assert_eq!(pose.mode, mode);
        assert_eq!(pose.prediction_ms, prediction);
    }
    assert_eq!(
        h.pose(1.15).unwrap().0,
        h.evaluate(1.15, Config::DEFAULT).unwrap().position
    );
    assert!(
        h.evaluate(
            1.15,
            Config {
                delay_ms: 201,
                extrapolation_ms: 200
            }
        )
        .is_none()
    );
}
#[test]
fn lower_delay_does_not_remove_teleport_burst_or_finite_guards() {
    let mut h = History::default();
    for (at, x) in [(1.0, 0.0), (1.000001, 100.0)] {
        h.push(Sample {
            at,
            position: [x, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
        });
    }
    let config = Config {
        delay_ms: 0,
        extrapolation_ms: 200,
    };
    assert!(h.evaluate(1.200001, config).unwrap().position[0] <= 1300.01);
    h.push(Sample {
        at: 1.1,
        position: [1_000_000.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
    });
    assert_eq!(h.evaluate(1.2, config).unwrap().mode, Mode::Snapped);
    assert!(h.evaluate(f64::NAN, config).is_none());
}
fn call(command: u32, json: Option<&str>) -> (u32, Control) {
    let mut block = Control::new(command);
    if let Some(json) = json {
        api::put(&mut block.message, json).unwrap();
    }
    // SAFETY: full, aligned, exclusive Control lives until the direct call returns.
    let result = unsafe { client_dll::client_control(&mut block) };
    (result, block)
}
fn query() -> Status {
    let (result, block) = call(api::PRESENTATION_STATUS, None);
    assert_eq!(result, 0);
    serde_json::from_str(api::text(&block.message).unwrap()).unwrap()
}
#[test]
fn control_configuration_is_atomic_and_bad_payload_cannot_change_it() {
    let before = query();
    let config = Config {
        delay_ms: 25,
        extrapolation_ms: 50,
    };
    assert_eq!(
        call(
            api::PRESENTATION_SET,
            Some(&serde_json::to_string(&config).unwrap())
        )
        .0,
        0
    );
    let after = query();
    assert_eq!(after.config, config);
    assert_eq!(after.revision, before.revision + 1);
    for bad in [
        r#"{"delay_ms":201,"extrapolation_ms":100}"#,
        r#"{"delay_ms":10,"extrapolation_ms":999}"#,
        r#"{"delay_ms":10,"extrapolation_ms":50,"extra":true}"#,
        "not json",
    ] {
        assert_ne!(call(api::PRESENTATION_SET, Some(bad)).0, 0);
        assert_eq!(query().config, config);
        assert_eq!(query().revision, after.revision);
    }
    call(
        api::PRESENTATION_SET,
        Some(&serde_json::to_string(&Config::DEFAULT).unwrap()),
    );
}
#[test]
fn maximum_diagnostic_values_fit_the_existing_control_message() {
    let status = Status {
        config: Config::DEFAULT,
        revision: u32::MAX,
        buffering: u64::MAX,
        interpolated: u64::MAX,
        predicted: u64::MAX,
        capped: u64::MAX,
        held: u64::MAX,
        snapped: u64::MAX,
        stale: u64::MAX,
        empty_queries: u64::MAX,
        lock_misses: u64::MAX,
    };
    let mut b = Control::new(api::PRESENTATION_STATUS);
    let json = serde_json::to_string(&status).unwrap();
    api::put(&mut b.message, &json).unwrap();
    assert_eq!(std::mem::size_of::<Control>(), 1536);
}

#[test]
fn timing_diagnostics_fit_control_and_are_null_before_a_v2_connection() {
    use multiplayer_server::session_timing::Timing;
    let timing = Timing {
        rtt_samples: u64::MAX,
        rtt_max_ms: u64::MAX,
        rtt_jitter_us: u64::MAX,
        snapshots: u64::MAX,
        arrival_jitter_us: u64::MAX,
        max_receive_gap_ms: u64::MAX,
        tick_gap_events: u64::MAX,
        largest_tick_step: u64::MAX,
        nonmonotonic_snapshots: u64::MAX,
    };
    let mut b = Control::new(api::NETWORK_TIMING);
    api::put(
        &mut b.message,
        &serde_json::to_string(&Some(timing)).unwrap(),
    )
    .unwrap();
    let (result, b) = call(api::NETWORK_TIMING, None);
    assert_eq!(result, 0);
    assert_eq!(api::text(&b.message).unwrap(), "null");
}

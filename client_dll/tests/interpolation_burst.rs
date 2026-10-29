//! Receive bursts are not a measured physics velocity. No game or clock needed.
use client_dll::interpolation::{History, MAX_EXTRAPOLATION, Sample, TELEPORT_SPEED, pose_at};
fn sample(at: f64, x: f32) -> Sample {
    Sample {
        at,
        position: [x, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
    }
}
#[test]
fn closely_spaced_receive_times_cannot_launch_unbounded_prediction() {
    let samples = [sample(1.0, 0.0), sample(1.000001, 100.0)];
    let position = pose_at(samples.into_iter(), 1.200001).unwrap().0[0];
    let bound = 100.0 + TELEPORT_SPEED * MAX_EXTRAPOLATION as f32;
    assert!(
        position <= bound + 1.0,
        "receive burst launched prediction to {position} cm (bound {bound})"
    );
    assert!((position - 1300.0).abs() < 0.1);
}
#[test]
fn equal_receive_time_keeps_latest_pose_without_a_second_velocity_sample() {
    let mut history = History::default();
    history.push(sample(1.0, 10.0));
    history.push(sample(1.0, 20.0));
    assert_eq!(history.len(), 1);
    assert_eq!(history.newest().unwrap().position[0], 20.0);
    assert_eq!(history.pose(2.0).unwrap().0[0], 20.0);
}
#[test]
fn invalid_samples_queries_and_extreme_finite_coordinates_do_not_poison_history() {
    let mut history = History::default();
    history.push(sample(1.0, 10.0));
    for bad in [
        sample(f64::NAN, 20.0),
        sample(f64::INFINITY, 20.0),
        sample(1.1, f32::INFINITY),
        Sample {
            rotation: [0.0; 4],
            ..sample(1.1, 20.0)
        },
    ] {
        history.push(bad);
    }
    assert_eq!(history.len(), 1);
    assert!(history.pose(f64::NAN).is_none());
    assert!(pose_at([sample(1.0, 10.0)].into_iter(), f64::INFINITY).is_none());
    let samples = [sample(1.0, f32::MIN), sample(1.1, f32::MAX)];
    for at in [0.0, 1.05, 1.1, 2.0] {
        assert!(
            pose_at(samples.into_iter(), at)
                .unwrap()
                .0
                .into_iter()
                .all(f32::is_finite)
        );
    }
}

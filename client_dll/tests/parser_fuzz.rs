//! Bounded deterministic robustness run of every in-memory decoder.
#[test]
fn decoders_never_panic_and_accepted_frames_round_trip() {
    for seed in [1, 2, 3] {
        let report = client_dll::parser_fuzz::run(seed, 4000);
        for t in &report.targets {
            assert_eq!(t.panics, 0, "{t:?}");
            assert_eq!(t.property_failures, 0, "{t:?}");
            assert!(t.accepted > 0 && t.rejected > 0, "{t:?}");
        }
        assert!(report.passed);
    }
}

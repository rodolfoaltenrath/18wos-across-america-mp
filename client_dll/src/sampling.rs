//! Suppress a read across an observed scene transition. This is a guard, not
//! atomic synchronization with the game's physics or every possible teardown.
pub enum Outcome<T> {
    Suspended,
    Discarded,
    Sampled(T),
}
pub fn detailed<T>(mut loading: impl FnMut() -> bool, read: impl FnOnce() -> T) -> Outcome<T> {
    if loading() {
        return Outcome::Suspended;
    }
    let result = read();
    if loading() {
        Outcome::Discarded
    } else {
        Outcome::Sampled(result)
    }
}
pub fn guarded<T>(loading: impl FnMut() -> bool, read: impl FnOnce() -> T) -> Option<T> {
    match detailed(loading, read) {
        Outcome::Sampled(value) => Some(value),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_skips_reads_and_a_transition_during_the_read_discards_the_result() {
        assert!(matches!(
            detailed(|| true, || panic!("must not read")),
            Outcome::<()>::Suspended
        ));
        let mut phase = 0;
        assert!(matches!(
            detailed(
                || {
                    phase += 1;
                    phase == 2
                },
                || 99
            ),
            Outcome::Discarded
        ));
        assert_eq!(guarded(|| true, || panic!("must not read")), None::<()>);
        let mut checks = 0;
        assert_eq!(
            guarded(
                || {
                    checks += 1;
                    checks > 1
                },
                || 123
            ),
            None
        );
        assert_eq!(guarded(|| false, || 123), Some(123));
    }
}

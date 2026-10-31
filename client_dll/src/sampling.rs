//! Suppress a read across an observed scene transition. This is a guard, not
//! atomic synchronization with the game's physics or every possible teardown.
pub fn guarded<T>(mut loading: impl FnMut() -> bool, read: impl FnOnce() -> T) -> Option<T> {
    if loading() {
        return None;
    }
    let result = read();
    (!loading()).then_some(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_skips_reads_and_a_transition_during_the_read_discards_the_result() {
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

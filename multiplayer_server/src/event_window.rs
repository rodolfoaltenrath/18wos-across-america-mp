//! Monotonic id horizon: an evicted old id cannot become a new event.
#[derive(Clone)]
pub(crate) struct Window<const N: usize> {
    highest: u64,
    seen: [bool; N],
}
impl<const N: usize> Default for Window<N> {
    fn default() -> Self {
        Self {
            highest: 0,
            seen: [false; N],
        }
    }
}
impl<const N: usize> Window<N> {
    pub(crate) fn admit(&mut self, id: u64) -> bool {
        if id == 0 || N == 0 {
            return false;
        }
        if id > self.highest {
            let advance = (id - self.highest).min(N as u64) as usize;
            self.seen.copy_within(..N - advance, advance);
            self.seen[..advance].fill(false);
            self.highest = id;
        }
        let age = self.highest - id;
        if age >= N as u64 {
            return false;
        }
        let bit = &mut self.seen[age as usize];
        let fresh = !*bit;
        *bit = true;
        fresh
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reordering_is_accepted_once_and_eviction_never_reopens_old_ids() {
        let mut w = Window::<128>::default();
        for id in [1, 3, 2, 128] {
            assert!(w.admit(id));
        }
        assert!(!w.admit(1));
        assert!(w.admit(129));
        assert!(!w.admit(1));
        assert!(w.admit(127));
        assert!(!w.admit(127));
        assert!(w.admit(u64::MAX));
        assert!(!w.admit(129));
        assert!(w.admit(u64::MAX - 127));
        assert!(!w.admit(u64::MAX - 128));
        assert!(!w.admit(0));
    }
}

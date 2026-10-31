//! Deterministic input mutator for our own parser robustness checks. No
//! dependencies, no I/O; the same seed always yields the same inputs.

/// xorshift64* generator; never zero-state.
#[derive(Clone, Debug)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}
/// Tokens that steer JSON parsers into boundary and type confusion paths.
const TOKENS: &[&[u8]] = &[
    b"0",
    b"-1",
    b"1e309",
    b"-0",
    b"18446744073709551616",
    b"4294967296",
    b"NaN",
    b"null",
    b"true",
    b"\"\"",
    b"[]",
    b"{}",
    b"\"\\u0000\"",
    b"\"\\ud800\"",
    b",",
    b":",
    b"\"x\":1",
    b"\xff",
    b"\x00",
    b"9999999999999999999999",
];
/// Upper bound for any produced input: oversized handling is tested by the
/// parsers' own limits, not by allocating unbounded inputs here.
pub const MAX_INPUT: usize = 64 * 1024;

pub fn mutate(rng: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut out = seed.to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(9) {
            0 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] ^= 1 << rng.below(8);
            }
            1 if !out.is_empty() => {
                let i = rng.below(out.len());
                out[i] = rng.next_u64() as u8;
            }
            2 => out.truncate(rng.below(out.len() + 1)),
            3 if !out.is_empty() => {
                let start = rng.below(out.len());
                let end = (start + 1 + rng.below(16)).min(out.len());
                out.drain(start..end);
            }
            4 => {
                let token = TOKENS[rng.below(TOKENS.len())];
                let at = rng.below(out.len() + 1);
                out.splice(at..at, token.iter().copied());
            }
            5 if !out.is_empty() => {
                // Duplicate a slice: repeated keys/elements and length growth.
                let start = rng.below(out.len());
                let end = (start + 1 + rng.below(64)).min(out.len());
                let copy = out[start..end].to_vec();
                let at = rng.below(out.len() + 1);
                out.splice(at..at, copy);
            }
            6 if !out.is_empty() => {
                // Replace a digit run: numeric boundaries in JSON documents.
                let i = rng.below(out.len());
                if out[i].is_ascii_digit() {
                    let token = TOKENS[rng.below(4) + 1];
                    out.splice(i..i + 1, token.iter().copied());
                }
            }
            7 if out.len() >= 4 => {
                // Little-endian word extremes for binary layouts.
                let i = rng.below(out.len() - 3);
                let value = [0u32, 1, u32::MAX, 0x8000_0000, rng.next_u64() as u32][rng.below(5)];
                out[i..i + 4].copy_from_slice(&value.to_le_bytes());
            }
            _ => {
                let len = rng.below(32);
                let at = rng.below(out.len() + 1);
                let random: Vec<u8> = (0..len).map(|_| rng.next_u64() as u8).collect();
                out.splice(at..at, random);
            }
        }
    }
    out.truncate(MAX_INPUT);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutations_are_deterministic_bounded_and_varied() {
        let seed = br#"{"version":2,"message":{"type":"ping"}}"#;
        let a: Vec<_> = (0..64)
            .scan(Rng::new(7), |r, _| Some(mutate(r, seed)))
            .collect();
        let b: Vec<_> = (0..64)
            .scan(Rng::new(7), |r, _| Some(mutate(r, seed)))
            .collect();
        assert_eq!(a, b);
        assert!(a.iter().any(|m| m.len() < seed.len()));
        assert!(a.iter().any(|m| m.len() > seed.len()));
        assert!(a.iter().all(|m| m.len() <= MAX_INPUT));
        assert_eq!(Rng::new(0).next_u64(), Rng::new(1).next_u64());
        assert!(mutate(&mut Rng::new(3), &[]).len() <= 32 * 5);
    }
}

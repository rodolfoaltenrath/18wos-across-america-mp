//! Bounded, seeded datagram scheduling shared by the virtual lab and UDP proxy.
//! It never decodes a packet or invokes the game/worker.
use clap::Args;
use serde::Serialize;
use std::{cmp::Reverse, collections::BinaryHeap};

#[derive(Clone, Debug, Args, Serialize)]
pub struct Conditions {
    #[arg(long, default_value_t = 42, value_parser = clap::value_parser!(u64).range(1..))]
    pub seed: u64,
    /// Loss per thousand original datagrams, independently in each direction.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(0..=1000))]
    pub loss_permille: u16,
    /// Chance per thousand surviving datagrams of one additional copy.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(0..=1000))]
    pub duplicate_permille: u16,
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u32).range(0..=5000))]
    pub delay_ms: u32,
    /// Uniform jitter in [-jitter, +jitter], clamping total delay at zero.
    #[arg(long, default_value_t = 80, value_parser = clap::value_parser!(u32).range(0..=5000))]
    pub jitter_ms: u32,
    #[arg(long, default_value_t = 2048, value_parser = clap::value_parser!(u32).range(1..=8192))]
    pub queue_capacity: u32,
}
impl Conditions {
    pub fn validate(&self) -> Result<(), String> {
        if self.seed == 0
            || self.loss_permille > 1000
            || self.duplicate_permille > 1000
            || self.delay_ms > 5000
            || self.jitter_ms > 5000
            || !(1..=8192).contains(&self.queue_capacity)
        {
            return Err("invalid network conditions".into());
        }
        Ok(())
    }
    /// Separate stream for server -> client; always nonzero, including wrap.
    pub fn reverse(&self) -> Self {
        let mut conditions = self.clone();
        conditions.seed = self.seed.wrapping_add(0x9e37_79b9_7f4a_7c15).max(1);
        conditions
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counters {
    pub submitted: u64,
    pub random_loss: u64,
    pub duplicate_copies: u64,
    pub queue_overflow: u64,
    pub emitted: u64,
    pub reordered: u64,
    pub peak_queue: usize,
}
pub struct Link {
    conditions: Conditions,
    random: u64,
    serial: u64,
    last_emitted: u64,
    queue: BinaryHeap<Reverse<(u64, u64, Vec<u8>)>>,
    counts: Counters,
}
impl Link {
    pub fn new(conditions: Conditions) -> Result<Self, String> {
        conditions.validate()?;
        Ok(Self {
            random: conditions.seed,
            conditions,
            serial: 0,
            last_emitted: 0,
            queue: BinaryHeap::new(),
            counts: Counters::default(),
        })
    }
    fn random(&mut self) -> u64 {
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.random = x;
        x
    }
    fn enqueue(&mut self, now: u64, bytes: &[u8]) {
        if self.queue.len() == self.conditions.queue_capacity as usize {
            self.counts.queue_overflow += 1;
            return;
        }
        let jitter = i64::from(self.conditions.jitter_ms);
        let offset = (self.random() % (2 * jitter + 1) as u64) as i64 - jitter;
        let delay = (i64::from(self.conditions.delay_ms) + offset).max(0) as u64;
        self.serial += 1;
        self.queue
            .push(Reverse((now + delay, self.serial, bytes.to_vec())));
        self.counts.peak_queue = self.counts.peak_queue.max(self.queue.len());
    }
    /// Reject over-budget data before allocating; callers count transport drops.
    pub fn submit(&mut self, now: u64, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > multiplayer_server::protocol::MAX_PACKET || now > u64::MAX - 10_000 {
            return Err("datagram or virtual time exceeds scheduler limit".into());
        }
        self.counts.submitted += 1;
        if self.random() % 1000 < u64::from(self.conditions.loss_permille) {
            self.counts.random_loss += 1;
            return Ok(());
        }
        self.enqueue(now, bytes);
        if self.random() % 1000 < u64::from(self.conditions.duplicate_permille) {
            self.counts.duplicate_copies += 1;
            self.enqueue(now, bytes);
        }
        Ok(())
    }
    pub fn pop(&mut self, now: u64) -> Option<Vec<u8>> {
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
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
    pub fn counters(&self) -> Counters {
        self.counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn conditions() -> Conditions {
        Conditions {
            seed: 1,
            loss_permille: 0,
            duplicate_permille: 0,
            delay_ms: 10,
            jitter_ms: 0,
            queue_capacity: 2,
        }
    }
    #[test]
    fn bounds_deadlines_and_fifo_ties_hold_without_a_socket_or_clock() {
        let mut link = Link::new(conditions()).unwrap();
        assert!(
            link.submit(0, &[0; multiplayer_server::protocol::MAX_PACKET + 1])
                .is_err()
        );
        assert!(link.submit(u64::MAX, b"bad time").is_err());
        assert_eq!(link.counters().submitted, 0);
        link.submit(0, b"one").unwrap();
        link.submit(0, b"two").unwrap();
        link.submit(0, b"overflow").unwrap();
        assert!(link.pop(9).is_none());
        assert_eq!(link.pop(10).unwrap(), b"one");
        assert_eq!(link.pop(10).unwrap(), b"two");
        assert!(link.pop(10).is_none());
        assert_eq!(link.counters().queue_overflow, 1);
        assert_eq!(link.pending(), 0);
        let mut invalid = conditions();
        invalid.seed = 0;
        assert!(Link::new(invalid).is_err());
    }
    #[test]
    fn reverse_seed_cannot_be_zero_after_wrapping_and_preserves_limits() {
        let mut c = conditions();
        c.seed = 0u64.wrapping_sub(0x9e37_79b9_7f4a_7c15);
        let reverse = c.reverse();
        assert_eq!(reverse.seed, 1);
        assert_ne!(c.seed, reverse.seed);
        reverse.validate().unwrap();
    }
}

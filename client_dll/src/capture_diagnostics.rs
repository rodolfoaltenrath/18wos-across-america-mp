//! Bounded reason counters, without addresses, paths, poses or diagnostic text.
//! A snapshot is independent of the other control counters; not a physics tick.
use serde::{Deserialize, Serialize};

const MAX: u64 = 999_999_999;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Ready,
    GateBusy,
    GateChanged,
    Missing,
    Changed,
    WrongType,
    Overflow,
    Invalid,
    DrawnExpired,
    Other,
    Fatal,
}
impl From<crate::pose_capture::Unavailable> for Reason {
    fn from(value: crate::pose_capture::Unavailable) -> Self {
        use crate::pose_capture::Unavailable as U;
        match value {
            U::Missing => Self::Missing,
            U::Changed => Self::Changed,
            U::Type => Self::WrongType,
            U::Overflow => Self::Overflow,
            U::Invalid => Self::Invalid,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: u8,
    pub passes: u64,
    pub accepted: u64,
    pub skipped: u64,
    pub discarded: u64,
    pub missing: u64,
    pub changed: u64,
    pub wrong_type: u64,
    pub overflow: u64,
    pub invalid: u64,
    pub drawn_expired: u64,
    pub other: u64,
    pub fatal: u64,
    pub last: Option<Reason>,
    pub truncated: bool,
}
impl Default for Report {
    fn default() -> Self {
        Self {
            schema: 1,
            passes: 0,
            accepted: 0,
            skipped: 0,
            discarded: 0,
            missing: 0,
            changed: 0,
            wrong_type: 0,
            overflow: 0,
            invalid: 0,
            drawn_expired: 0,
            other: 0,
            fatal: 0,
            last: None,
            truncated: false,
        }
    }
}
impl Report {
    fn counters(&self) -> [u64; 12] {
        [
            self.passes,
            self.accepted,
            self.skipped,
            self.discarded,
            self.missing,
            self.changed,
            self.wrong_type,
            self.overflow,
            self.invalid,
            self.drawn_expired,
            self.other,
            self.fatal,
        ]
    }
    pub(crate) fn observe(&mut self, reason: Reason) {
        self.passes = self.passes.saturating_add(1);
        let count = match reason {
            Reason::Ready => &mut self.accepted,
            Reason::GateBusy => &mut self.skipped,
            Reason::GateChanged => &mut self.discarded,
            Reason::Missing => &mut self.missing,
            Reason::Changed => &mut self.changed,
            Reason::WrongType => &mut self.wrong_type,
            Reason::Overflow => &mut self.overflow,
            Reason::Invalid => &mut self.invalid,
            Reason::DrawnExpired => &mut self.drawn_expired,
            Reason::Other => &mut self.other,
            Reason::Fatal => &mut self.fatal,
        };
        *count = count.saturating_add(1);
        self.last = Some(reason);
    }
    pub(crate) fn bounded(&self) -> Self {
        let c = |v: u64| v.min(MAX);
        Self {
            schema: 1,
            passes: c(self.passes),
            accepted: c(self.accepted),
            skipped: c(self.skipped),
            discarded: c(self.discarded),
            missing: c(self.missing),
            changed: c(self.changed),
            wrong_type: c(self.wrong_type),
            overflow: c(self.overflow),
            invalid: c(self.invalid),
            drawn_expired: c(self.drawn_expired),
            other: c(self.other),
            fatal: c(self.fatal),
            last: self.last,
            truncated: self.truncated || self.counters().iter().any(|v| *v > MAX),
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let counters = self.counters();
        if self.schema != 1
            || counters.iter().any(|v| *v > MAX)
            || (!self.truncated && counters[1..].iter().sum::<u64>() != self.passes)
            || (self.passes == 0) != self.last.is_none()
        {
            return Err("invalid capture counters");
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounts_for_rejections_and_gate_discards_without_claiming_their_cause() {
        let mut r = Report::default();
        for reason in [
            Reason::Ready,
            Reason::Changed,
            Reason::GateBusy,
            Reason::GateChanged,
            Reason::Missing,
            Reason::WrongType,
            Reason::Overflow,
            Reason::Invalid,
            Reason::DrawnExpired,
            Reason::Other,
            Reason::Fatal,
        ] {
            r.observe(reason);
        }
        assert_eq!(r.passes, 11);
        assert_eq!(
            (r.accepted, r.changed, r.skipped, r.discarded),
            (1, 1, 1, 1)
        );
        r.validate().unwrap();
        let text = serde_json::to_string(&r).unwrap();
        let decoded: Report = serde_json::from_str(&text).unwrap();
        decoded.validate().unwrap();
        assert!(!text.contains("address") && !text.contains("message"));
        r.accepted += 1;
        assert!(r.validate().is_err());
    }
    #[test]
    fn worst_case_is_bounded_for_control_and_does_not_wrap() {
        let mut r = Report::default();
        for _ in 0..2 {
            r.observe(Reason::Changed);
        }
        r.passes = u64::MAX;
        r.changed = u64::MAX;
        r.observe(Reason::Changed);
        assert_eq!(r.passes, u64::MAX);
        let bounded = r.bounded();
        bounded.validate().unwrap();
        assert!(bounded.truncated);
        assert!(serde_json::to_string(&bounded).unwrap().len() < 512);
        let mut worst = serde_json::to_value(&bounded).unwrap();
        for (name, value) in worst.as_object_mut().unwrap() {
            if name != "schema" && value.is_u64() {
                *value = MAX.into();
            }
        }
        worst["last"] = "drawn_expired".into();
        let worst: Report = serde_json::from_value(worst).unwrap();
        worst.validate().unwrap();
        assert!(serde_json::to_string(&worst).unwrap().len() < 512);
        let mut malformed = bounded;
        malformed.last = None;
        assert!(malformed.validate().is_err());
    }
}

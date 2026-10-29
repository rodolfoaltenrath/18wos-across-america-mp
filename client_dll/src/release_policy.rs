//! Release checks for the already validated outer-loop path. Not an
//! authorization to free from a command handler or to retain across reloads.
use crate::api::EngineControl;
use serde::Serialize;
use std::fmt;

pub const SETTLE_FLUSHES: u32 = 8;
const SCHEMA: u32 = 0x56525031;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    NotAllocated,
    Visible,
    WrongPhase,
    NestedCallback,
    AmbiguousFlushOrder,
    DrawsPending,
    Unreadable,
    Referenced,
    MissingBindings,
}
impl Refusal {
    /// Only these conditions may improve by waiting on the same live world.
    pub fn retryable(self) -> bool {
        matches!(self, Self::DrawsPending | Self::Referenced)
    }
}
impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAllocated => "no test visual allocated",
            Self::Visible => "hide the visual before releasing it",
            Self::WrongPhase => "release requires the installed owner outer loop",
            Self::NestedCallback => "release refused inside a nested window/draw/render callback",
            Self::AmbiguousFlushOrder => "flush order ambiguous; instance retained",
            Self::DrawsPending => "queued draws not yet flushed; retry after more frames",
            Self::Unreadable => "visual item unreadable; retained",
            Self::Referenced => "visual item still referenced; retry after more frames",
            Self::MissingBindings => "visual release bindings missing; instance retained",
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Context {
    pub current_thread: u32,
    pub owner_thread: u32,
    pub pump_installed: bool,
    pub pump_depth: u32,
    pub window_depth: u32,
    pub draw_depth: u32,
    pub render_depth: u32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub allocated: bool,
    pub shown: bool,
    pub item: u32,
    pub drawn: u32,
    pub ever_drawn: bool,
    pub flushes: u32,
    pub last_draw_flush: u32,
    pub hidden_at: u32,
    pub counters: Option<[u32; 3]>,
    pub bindings_present: bool,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub context: Context,
    pub evidence: Evidence,
    pub since_last_draw: Option<u32>,
    pub since_hidden: Option<u32>,
    pub refusal: Option<Refusal>,
    pub retryable: bool,
    /// Snapshot only: release rechecks all evidence immediately before freeing.
    pub ready: bool,
    /// The command handler is not a proven release phase; deferral is unproven.
    pub automatic_load_supported: bool,
}
fn elapsed(now: u32, then: u32) -> Option<u32> {
    let distance = now.wrapping_sub(then);
    (distance <= i32::MAX as u32).then_some(distance)
}
pub fn assess(context: Context, evidence: Evidence) -> Report {
    let since_last_draw = elapsed(evidence.flushes, evidence.last_draw_flush);
    let since_hidden = elapsed(evidence.flushes, evidence.hidden_at);
    let refusal = (|| {
        if !evidence.allocated {
            return Some(Refusal::NotAllocated);
        }
        if context.owner_thread == 0
            || context.current_thread != context.owner_thread
            || !context.pump_installed
            || context.pump_depth == 0
        {
            return Some(Refusal::WrongPhase);
        }
        if context.pump_depth != 1
            || context.window_depth != 0
            || context.draw_depth != 0
            || context.render_depth != 0
        {
            return Some(Refusal::NestedCallback);
        }
        if evidence.shown {
            return Some(Refusal::Visible);
        }
        if evidence.ever_drawn {
            let (Some(draw), Some(hidden)) = (since_last_draw, since_hidden) else {
                return Some(Refusal::AmbiguousFlushOrder);
            };
            // Compare BOTH ages. Numeric max of wrapping stamps is incorrect.
            if draw < SETTLE_FLUSHES || hidden < SETTLE_FLUSHES {
                return Some(Refusal::DrawsPending);
            }
        }
        if evidence.item == 0 || evidence.counters.is_none() {
            return Some(Refusal::Unreadable);
        }
        if evidence.counters != Some([1, 1, 0]) {
            return Some(Refusal::Referenced);
        }
        if !evidence.bindings_present {
            return Some(Refusal::MissingBindings);
        }
        None
    })();
    Report {
        context,
        evidence,
        since_last_draw,
        since_hidden,
        refusal,
        retryable: refusal.is_some_and(Refusal::retryable),
        ready: refusal.is_none(),
        automatic_load_supported: false,
    }
}
impl Report {
    /// Command 51 only, within the existing v3/664-byte diagnostic payload.
    pub fn encode(&self, b: &mut EngineControl) {
        let c = self.context;
        let e = self.evidence;
        b.frames = [0; 32];
        b.frames[..22].copy_from_slice(&[
            SCHEMA,
            c.current_thread,
            c.owner_thread,
            u32::from(c.pump_installed),
            c.pump_depth,
            c.window_depth,
            c.draw_depth,
            c.render_depth,
            u32::from(e.allocated),
            u32::from(e.shown),
            e.item,
            e.drawn,
            e.flushes,
            e.last_draw_flush,
            e.hidden_at,
            u32::from(e.counters.is_some()),
            e.counters.unwrap_or_default()[0],
            e.counters.unwrap_or_default()[1],
            e.counters.unwrap_or_default()[2],
            u32::from(e.bindings_present),
            SETTLE_FLUSHES,
            u32::from(e.ever_drawn),
        ]);
    }
    pub fn decode(b: &EngineControl) -> Result<Self, String> {
        let w = &b.frames;
        if b.command != crate::api::VISUAL_CHECK
            || w[0] != SCHEMA
            || w[20] != SETTLE_FLUSHES
            || [3, 8, 9, 15, 19, 21].iter().any(|&i| w[i] > 1)
            || w[22..].iter().any(|&v| v != 0)
            || (w[15] == 0 && w[16..19] != [0; 3])
        {
            return Err("invalid visual-check report schema".into());
        }
        Ok(assess(
            Context {
                current_thread: w[1],
                owner_thread: w[2],
                pump_installed: w[3] != 0,
                pump_depth: w[4],
                window_depth: w[5],
                draw_depth: w[6],
                render_depth: w[7],
            },
            Evidence {
                allocated: w[8] != 0,
                shown: w[9] != 0,
                item: w[10],
                drawn: w[11],
                ever_drawn: w[21] != 0,
                flushes: w[12],
                last_draw_flush: w[13],
                hidden_at: w[14],
                counters: (w[15] != 0).then_some([w[16], w[17], w[18]]),
                bindings_present: w[19] != 0,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn valid() -> (Context, Evidence) {
        (
            Context {
                current_thread: 7,
                owner_thread: 7,
                pump_installed: true,
                pump_depth: 1,
                ..Context::default()
            },
            Evidence {
                allocated: true,
                item: 99,
                drawn: 1,
                ever_drawn: true,
                flushes: 20,
                last_draw_flush: 10,
                hidden_at: 12,
                counters: Some([1, 1, 0]),
                bindings_present: true,
                ..Evidence::default()
            },
        )
    }
    #[test]
    fn wrapping_stamps_check_both_ages_and_reject_ambiguous_order() {
        let (c, mut e) = valid();
        e.last_draw_flush = u32::MAX - 10;
        e.hidden_at = 2;
        e.flushes = 5;
        assert_eq!(assess(c, e).refusal, Some(Refusal::DrawsPending));
        e.flushes = 10;
        assert!(assess(c, e).ready);
        // Diagnostic draw counts may wrap, but this never removes the guard.
        e.drawn = 0;
        e.flushes = 5;
        assert_eq!(assess(c, e).refusal, Some(Refusal::DrawsPending));
        e.flushes = 10;
        e.hidden_at = 11;
        assert_eq!(assess(c, e).refusal, Some(Refusal::AmbiguousFlushOrder));
        e.hidden_at = e.flushes.wrapping_sub(0x8000_0000);
        assert_eq!(assess(c, e).refusal, Some(Refusal::AmbiguousFlushOrder));
    }
    #[test]
    fn map_thread_alone_does_not_authorize_free_and_nested_callbacks_refuse() {
        let (c, e) = valid();
        assert!(assess(c, e).ready);
        assert!(!assess(c, e).automatic_load_supported);
        assert_eq!(
            assess(Context { pump_depth: 0, ..c }, e).refusal,
            Some(Refusal::WrongPhase)
        );
        assert_eq!(
            assess(
                Context {
                    current_thread: 8,
                    ..c
                },
                e
            )
            .refusal,
            Some(Refusal::WrongPhase)
        );
        assert_eq!(
            assess(
                Context {
                    pump_installed: false,
                    ..c
                },
                e
            )
            .refusal,
            Some(Refusal::WrongPhase)
        );
        for nested in [
            Context { pump_depth: 2, ..c },
            Context {
                window_depth: 1,
                ..c
            },
            Context { draw_depth: 1, ..c },
            Context {
                render_depth: 1,
                ..c
            },
        ] {
            assert_eq!(assess(nested, e).refusal, Some(Refusal::NestedCallback));
        }
    }
    #[test]
    fn only_pending_draws_and_held_references_are_retryable() {
        let (c, e) = valid();
        for (changed, expected) in [
            (
                Evidence {
                    allocated: false,
                    ..e
                },
                Refusal::NotAllocated,
            ),
            (Evidence { shown: true, ..e }, Refusal::Visible),
            (Evidence { flushes: 19, ..e }, Refusal::DrawsPending),
            (
                Evidence {
                    counters: None,
                    ..e
                },
                Refusal::Unreadable,
            ),
            (Evidence { item: 0, ..e }, Refusal::Unreadable),
            (
                Evidence {
                    counters: Some([2, 1, 0]),
                    ..e
                },
                Refusal::Referenced,
            ),
            (
                Evidence {
                    counters: Some([1, 2, 0]),
                    ..e
                },
                Refusal::Referenced,
            ),
            (
                Evidence {
                    counters: Some([1, 1, 1]),
                    ..e
                },
                Refusal::Referenced,
            ),
            (
                Evidence {
                    bindings_present: false,
                    ..e
                },
                Refusal::MissingBindings,
            ),
        ] {
            let report = assess(c, changed);
            assert_eq!(report.refusal, Some(expected));
            assert!(!report.ready);
            assert_eq!(
                report.retryable,
                matches!(expected, Refusal::DrawsPending | Refusal::Referenced)
            );
        }
        // Never-drawn instances have no queued draw to settle, but still need counters.
        assert!(
            assess(
                c,
                Evidence {
                    drawn: 0,
                    ever_drawn: false,
                    flushes: 0,
                    ..e
                }
            )
            .ready
        );
        assert_eq!(
            assess(
                c,
                Evidence {
                    drawn: 0,
                    ever_drawn: false,
                    counters: None,
                    ..e
                }
            )
            .refusal,
            Some(Refusal::Unreadable)
        );
    }
    #[test]
    fn v3_payload_roundtrips_and_rejects_unknown_or_partial_reports() {
        let (c, e) = valid();
        let mut b = EngineControl::new(crate::api::VISUAL_CHECK);
        assert!(Report::decode(&b).is_err());
        assess(c, e).encode(&mut b);
        let report = Report::decode(&b).unwrap();
        assert_eq!(report.context, c);
        assert_eq!(report.evidence, e);
        assert!(report.ready);
        let mut corrupt = b;
        corrupt.frames[3] = 2;
        assert!(Report::decode(&corrupt).is_err());
        assess(
            c,
            Evidence {
                counters: None,
                ..e
            },
        )
        .encode(&mut b);
        assert_eq!(
            Report::decode(&b).unwrap().refusal,
            Some(Refusal::Unreadable)
        );
        b.frames[16] = 1;
        assert!(Report::decode(&b).is_err());
        assert_eq!(std::mem::size_of::<EngineControl>(), 664);
    }
}

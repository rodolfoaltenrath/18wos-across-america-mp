//! Offline evidence analysis. Never calls the engine or declares reload safe.
use crate::transition_observation::{ACTIVE, CAPACITY, LOST, TRUNCATED};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct Capture {
    summary: Summary,
    records: Vec<Event>,
}
#[derive(Deserialize)]
struct Summary {
    state: u32,
    generation: u32,
    records: u32,
    owner_thread: u32,
    flags: u32,
    #[serde(default)]
    foreign_calls: u32,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    InitEnter,
    InitReturn,
    DoneEnter,
    DoneReturn,
    FirstDraw,
    BlockInitEnter,
    BlockInitReturn,
    DestroyRequestEnter,
    DestroyRequestReturn,
    CommandDispatch,
}
impl Kind {
    fn paired(self) -> Option<(u8, bool)> {
        Some(match self {
            Self::InitEnter => (0, true),
            Self::InitReturn => (0, false),
            Self::DoneEnter => (1, true),
            Self::DoneReturn => (1, false),
            Self::BlockInitEnter => (2, true),
            Self::BlockInitReturn => (2, false),
            Self::DestroyRequestEnter => (3, true),
            Self::DestroyRequestReturn => (3, false),
            _ => return None,
        })
    }
    fn actor(self) -> bool {
        matches!(
            self,
            Self::InitEnter
                | Self::InitReturn
                | Self::DoneEnter
                | Self::DoneReturn
                | Self::FirstDraw
        )
    }
}
#[derive(Deserialize)]
struct Event {
    kind: Kind,
    thread: u32,
    caller: u32,
    this: u32,
    argument: u32,
    depth: u32,
    flushes_seen: u32,
    scene: Option<u32>,
    physics: Option<u32>,
    model_count: Option<u32>,
    models: Option<Vec<u32>>,
    command: Option<String>,
    command_status: Option<String>,
    handler_scene: Option<u32>,
    #[serde(default)]
    observed_car: u32,
    returned: Option<bool>,
}
#[derive(Serialize)]
pub struct Issue {
    pub event: Option<usize>,
    pub code: &'static str,
}
#[derive(Default, Serialize)]
pub struct Actor {
    pub address: u32,
    pub scenes: Vec<u32>,
    pub init_phases: Vec<i32>,
    pub done_phases: Vec<i32>,
    pub draws: Vec<usize>,
}
#[derive(Serialize)]
pub struct Cleanup {
    pub enter: usize,
    pub returned: usize,
    pub actor: u32,
    pub physics_before: Option<u32>,
    pub physics_after: Option<u32>,
    pub models_empty_after: Option<bool>,
}
#[derive(Serialize)]
pub struct FailedInitialization {
    pub enter: usize,
    pub returned: usize,
    pub block: u32,
    pub processor: u32,
}
#[derive(Serialize)]
pub struct CommandWindow {
    pub event: usize,
    pub following_end_exclusive: usize,
    pub name: Option<String>,
    pub scene_before: Option<u32>,
    pub observed_car: Option<u32>,
    pub first_different_actor_init: Option<usize>,
    pub first_old_actor_done: Option<usize>,
    pub first_different_actor_draw: Option<usize>,
    pub new_init_before_old_done: Option<bool>,
    pub old_cleanup_return: Option<usize>,
    pub draw_flush_delta: Option<u32>,
}
#[derive(Serialize)]
pub struct Analysis {
    pub generation: u32,
    pub record_count: usize,
    pub capture_active: bool,
    /// Completeness of this snapshot only, not coverage of all engine paths.
    pub structurally_complete: bool,
    pub issues: Vec<Issue>,
    pub paired_calls: usize,
    pub actors: Vec<Actor>,
    pub cleanups: Vec<Cleanup>,
    /// False init_eblock returns; they do not establish complete rollback/cleanup.
    pub failed_block_initializations: Vec<FailedInitialization>,
    pub commands: Vec<CommandWindow>,
    pub automation_validated: bool,
}
pub fn analyze(bytes: &[u8]) -> Result<Analysis, String> {
    if bytes.len() > MAX_CAPTURE_BYTES {
        return Err("capture exceeds 8 MiB".into());
    }
    let capture: Capture =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid transition capture: {e}"))?;
    let summary = &capture.summary;
    let events = &capture.records;
    if events.len() > CAPACITY || summary.records as usize != events.len() {
        return Err("capture record count is inconsistent or exceeds 512".into());
    }
    if !matches!(summary.state, 1 | 2) || summary.generation == 0 || summary.owner_thread == 0 {
        return Err("capture lacks an installed observer, generation or owner".into());
    }
    let mut out = Analysis {
        generation: summary.generation,
        record_count: events.len(),
        capture_active: summary.flags & ACTIVE != 0,
        structurally_complete: false,
        issues: Vec::new(),
        paired_calls: 0,
        actors: Vec::new(),
        cleanups: Vec::new(),
        failed_block_initializations: Vec::new(),
        commands: Vec::new(),
        automation_validated: false,
    };
    for (bad, code) in [
        (summary.flags & LOST != 0, "records_lost"),
        (summary.flags & TRUNCATED != 0, "trace_truncated"),
        (
            summary.flags & !(ACTIVE | LOST | TRUNCATED) != 0,
            "unknown_trace_flags",
        ),
        (summary.foreign_calls != 0, "foreign_callbacks_observed"),
        (events.is_empty(), "empty_snapshot"),
    ] {
        if bad {
            out.issues.push(Issue { event: None, code });
        }
    }
    let mut stack: Vec<usize> = Vec::new();
    let mut actors = BTreeMap::<u32, Actor>::new();
    let mut commands = Vec::new();
    for (i, event) in events.iter().enumerate() {
        if event.models.as_ref().is_some_and(|m| {
            m.len() > 16
                || event
                    .model_count
                    .is_none_or(|count| m.len() != (count as usize).min(16))
        }) || event.command.as_ref().is_some_and(|s| s.len() > 31)
        {
            return Err(format!("event {i} has invalid bounded fields"));
        }
        if event.thread != summary.owner_thread {
            out.issues.push(Issue {
                event: Some(i),
                code: "unexpected_thread",
            });
            continue;
        }
        if event.kind.actor() {
            let actor = actors.entry(event.this).or_insert_with(|| Actor {
                address: event.this,
                ..Actor::default()
            });
            if let Some(scene) = event.scene.filter(|&v| v != 0)
                && !actor.scenes.contains(&scene)
            {
                actor.scenes.push(scene);
            }
            match event.kind {
                Kind::InitEnter => actor.init_phases.push(event.argument as i32),
                Kind::DoneEnter => actor.done_phases.push(event.argument as i32),
                Kind::FirstDraw => actor.draws.push(i),
                _ => {}
            }
        }
        if let Some((family, enter)) = event.kind.paired() {
            if enter {
                if event.depth != stack.len() as u32 + 1 {
                    out.issues.push(Issue {
                        event: Some(i),
                        code: "enter_depth_mismatch",
                    });
                }
                stack.push(i);
            } else if let Some(start) = stack.pop() {
                let before = &events[start];
                if before.kind.paired() != Some((family, true))
                    || (before.this, before.argument, before.depth, before.caller)
                        != (event.this, event.argument, event.depth, event.caller)
                {
                    out.issues.push(Issue {
                        event: Some(i),
                        code: "return_mismatch",
                    });
                    continue;
                }
                out.paired_calls += 1;
                if family >= 2 && event.returned.is_none() {
                    out.issues.push(Issue {
                        event: Some(i),
                        code: "missing_block_result",
                    });
                }
                if family == 2 && event.returned == Some(false) {
                    out.failed_block_initializations.push(FailedInitialization {
                        enter: start,
                        returned: i,
                        block: event.argument,
                        processor: event.this,
                    });
                }
                if family == 1 && event.argument == 0 {
                    let models_empty = event.models.as_ref().and_then(|models| {
                        event
                            .model_count
                            .filter(|&n| n <= 16)
                            .map(|_| models.iter().all(|&m| m == 0))
                    });
                    out.cleanups.push(Cleanup {
                        enter: start,
                        returned: i,
                        actor: event.this,
                        physics_before: before.physics,
                        physics_after: event.physics,
                        models_empty_after: models_empty,
                    });
                }
            } else {
                out.issues.push(Issue {
                    event: Some(i),
                    code: "return_without_enter",
                });
            }
        } else {
            let expected = stack.len() as u32 + u32::from(event.kind == Kind::CommandDispatch);
            if event.depth != expected {
                out.issues.push(Issue {
                    event: Some(i),
                    code: "marker_depth_mismatch",
                });
            }
            if event.kind == Kind::CommandDispatch {
                commands.push(i);
                if event.command_status.as_deref() != Some("complete") || event.command.is_none() {
                    out.issues.push(Issue {
                        event: Some(i),
                        code: "command_name_incomplete",
                    });
                }
            }
        }
    }
    for event in stack {
        out.issues.push(Issue {
            event: Some(event),
            code: "open_call_at_snapshot_end",
        });
    }
    for (n, &start) in commands.iter().enumerate() {
        let end = commands.get(n + 1).copied().unwrap_or(events.len());
        let command = &events[start];
        let old = command.observed_car;
        let find = |kind, different: bool| {
            if old == 0 {
                None
            } else {
                ((start + 1)..end).find(|&i| {
                    events[i].thread == summary.owner_thread
                        && events[i].kind == kind
                        && ((events[i].this != old) == different)
                })
            }
        };
        let init = find(Kind::InitEnter, true);
        let done = find(Kind::DoneEnter, false);
        let draw = find(Kind::FirstDraw, true);
        let cleanup = out
            .cleanups
            .iter()
            .rev()
            .find(|c| {
                c.actor == old
                    && c.enter > start
                    && c.returned < end
                    && draw.is_none_or(|d| c.returned < d)
            })
            .map(|c| c.returned);
        let delta = cleanup.zip(draw).and_then(|(c, d)| {
            let delta = events[d].flushes_seen.wrapping_sub(events[c].flushes_seen);
            (delta <= i32::MAX as u32).then_some(delta)
        });
        out.commands.push(CommandWindow {
            event: start,
            following_end_exclusive: end,
            name: command
                .command_status
                .as_deref()
                .filter(|s| *s == "complete")
                .and(command.command.clone()),
            scene_before: command.handler_scene,
            observed_car: (old != 0).then_some(old),
            first_different_actor_init: init,
            first_old_actor_done: done,
            first_different_actor_draw: draw,
            new_init_before_old_done: init.zip(done).map(|(a, b)| a < b),
            old_cleanup_return: cleanup,
            draw_flush_delta: delta,
        });
    }
    out.actors = actors.into_values().collect();
    out.structurally_complete = out.issues.is_empty();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api,
        transition_observation::{COMMAND_READ, Record},
    };
    fn row(kind: u32, this: u32, arg: u32, depth: u32, scene: u32) -> Record {
        Record {
            kind,
            this,
            argument: arg,
            depth,
            thread: 7,
            caller: 0x777,
            flushes: 10,
            readable: 27,
            values: [scene, 5, 11, 2],
            models: std::array::from_fn(|i| if i < 2 { i as u32 + 11 } else { 0 }),
            ..Record::default()
        }
    }
    fn command() -> Record {
        let mut r = row(10, 99, 64, 1, 10);
        r.context[0] = 10;
        r.context[5] = 1;
        r.readable |= COMMAND_READ | 0x100;
        api::put(&mut r.command, "load").unwrap();
        r
    }
    fn snapshot(rows: &[Record]) -> serde_json::Value {
        serde_json::json!({"summary":{"state":1,"generation":3,"records":rows.len(),"owner_thread":7,"flags":1},"records":rows.iter().map(Record::json).collect::<Vec<_>>()})
    }
    fn parse(value: &serde_json::Value) -> Analysis {
        analyze(&serde_json::to_vec(value).unwrap()).unwrap()
    }
    fn success() -> Vec<Record> {
        let mut block = row(7, 20, 200, 1, 20);
        block.returned = 1;
        let mut cleared = row(4, 1, 0, 1, 10);
        cleared.values[1] = 0;
        cleared.models.fill(0);
        let mut draw = row(5, 2, 0, 0, 20);
        draw.flushes = 13;
        vec![
            row(5, 1, 0, 0, 10),
            command(),
            row(6, 20, 200, 1, 20),
            row(1, 2, (-10i32) as u32, 2, 20),
            row(2, 2, (-10i32) as u32, 2, 20),
            row(1, 2, 0, 2, 20),
            row(2, 2, 0, 2, 20),
            block,
            row(3, 1, 0, 1, 10),
            cleared,
            draw,
        ]
    }
    #[test]
    fn correlates_signed_phases_scenes_cleanup_and_three_flushes_without_authorizing_automation() {
        let report = parse(&snapshot(&success()));
        assert!(report.structurally_complete);
        assert!(report.capture_active);
        assert_eq!(report.paired_calls, 4);
        assert_eq!(report.actors.len(), 2);
        assert_eq!(report.actors[1].init_phases, [-10, 0]);
        assert_eq!(report.actors[1].scenes, [20]);
        let command = &report.commands[0];
        assert_eq!(
            (command.scene_before, command.observed_car),
            (Some(10), Some(1))
        );
        assert_eq!(
            (
                command.first_different_actor_init,
                command.first_old_actor_done
            ),
            (Some(3), Some(8))
        );
        assert_eq!(command.new_init_before_old_done, Some(true));
        assert_eq!(
            (
                command.old_cleanup_return,
                command.first_different_actor_draw,
                command.draw_flush_delta
            ),
            (Some(9), Some(10), Some(3))
        );
        let cleanup = &report.cleanups[0];
        assert_eq!(
            (
                cleanup.physics_before,
                cleanup.physics_after,
                cleanup.models_empty_after
            ),
            (Some(5), Some(0), Some(true))
        );
        assert!(!report.automation_validated);
    }
    #[test]
    fn failed_new_block_does_not_mean_old_actor_was_destroyed() {
        let rows = [
            row(5, 1, 0, 0, 10),
            command(),
            row(6, 20, 200, 1, 20),
            row(1, 2, 0, 2, 20),
            row(2, 2, 0, 2, 20),
            row(3, 2, 0, 2, 20),
            row(4, 2, 0, 2, 20),
            row(7, 20, 200, 1, 20),
            row(5, 1, 0, 0, 10),
        ];
        let report = parse(&snapshot(&rows));
        assert!(report.structurally_complete);
        assert_eq!(report.failed_block_initializations.len(), 1);
        assert_eq!(report.failed_block_initializations[0].block, 200);
        assert_eq!(report.cleanups[0].actor, 2);
        assert_eq!(report.commands[0].old_cleanup_return, None);
        assert_eq!(report.commands[0].first_old_actor_done, None);
        assert_eq!(report.commands[0].first_different_actor_draw, None);
        assert_eq!(report.commands[0].new_init_before_old_done, None);
        assert!(!report.automation_validated);
    }
    #[test]
    fn records_truncation_loss_mismatched_pairs_and_incomplete_commands_as_issues() {
        let mut source = snapshot(&success());
        source["summary"]["flags"] = (LOST | TRUNCATED).into();
        source["summary"]["foreign_calls"] = 1.into();
        source["records"][3]["depth"] = 9.into();
        source["records"][4]["this"] = 99.into();
        source["records"][1]["command_status"] = "unreadable".into();
        let report = parse(&source);
        assert!(!report.structurally_complete);
        for code in [
            "trace_truncated",
            "records_lost",
            "foreign_callbacks_observed",
            "enter_depth_mismatch",
            "return_mismatch",
            "command_name_incomplete",
        ] {
            assert!(
                report.issues.iter().any(|i| i.code == code),
                "missing {code}"
            );
        }
        assert!(report.commands[0].name.is_none());
        let open = parse(&snapshot(&[row(1, 1, 0, 1, 10)]));
        assert_eq!(open.issues[0].code, "open_call_at_snapshot_end");
        let unmatched = parse(&snapshot(&[row(2, 1, 0, 1, 10)]));
        assert_eq!(unmatched.issues[0].code, "return_without_enter");
        let mut foreign = snapshot(&success());
        foreign["records"][2]["thread"] = 8.into();
        assert!(
            parse(&foreign)
                .issues
                .iter()
                .any(|i| i.code == "unexpected_thread")
        );
    }
    #[test]
    fn validates_input_bounds_and_keeps_missing_models_unknown() {
        assert!(analyze(&vec![b' '; MAX_CAPTURE_BYTES + 1]).is_err());
        let mut source = snapshot(&success());
        source["summary"]["records"] = 99.into();
        assert!(analyze(&serde_json::to_vec(&source).unwrap()).is_err());
        let mut source = snapshot(&success());
        source["records"][0]["models"] = serde_json::json!(vec![0u32; 17]);
        assert!(analyze(&serde_json::to_vec(&source).unwrap()).is_err());
        let mut source = snapshot(&success());
        source["records"][0]["kind"] = "future_event".into();
        assert!(analyze(&serde_json::to_vec(&source).unwrap()).is_err());
        let mut source = snapshot(&success());
        source["records"][9]["models"] = serde_json::Value::Null;
        assert_eq!(parse(&source).cleanups[0].models_empty_after, None);
        let mut source = snapshot(&success());
        source["summary"]["generation"] = 0.into();
        assert!(analyze(&serde_json::to_vec(&source).unwrap()).is_err());
    }
    #[test]
    fn old_reports_and_command_boundaries_do_not_infer_missing_load_events() {
        let mut rows = success();
        rows.remove(1);
        let mut source = snapshot(&rows);
        source["summary"]["state"] = 2.into();
        source["summary"]["flags"] = 0.into();
        let old = parse(&source);
        assert!(old.structurally_complete);
        assert!(old.commands.is_empty());
        assert!(!old.capture_active);
        assert!(!old.automation_validated);
        let mut rows = success();
        let mut next = command();
        next.depth = 2; // The second command is nested inside init_eblock.
        api::put(&mut next.command, "status").unwrap();
        rows.insert(3, next);
        let report = parse(&snapshot(&rows));
        assert!(report.structurally_complete);
        assert_eq!(report.commands.len(), 2);
        assert_eq!(report.commands[0].following_end_exclusive, 3);
        assert!(report.commands[0].first_different_actor_init.is_none());
        assert_eq!(report.commands[1].first_different_actor_init, Some(4));
    }
}

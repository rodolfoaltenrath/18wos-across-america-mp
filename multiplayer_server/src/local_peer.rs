//! Explicit local visibility exercise. Follows received poses, never reads the
//! game/process/save and never claims a second computer was tested.
use crate::{
    protocol::{Position, Rotation},
    session_client::Event,
    session_protocol::{ECHO_BASE, PlayerState, Pose},
};
use serde::Serialize;

pub const CYCLE_MS: u64 = 75_000;
const FRESH_MS: u64 = 500;
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub stage: Option<u8>,
    pub state: PlayerState,
    pub pose: Option<(Position, Rotation)>,
    pub source_available: bool,
}
#[derive(Default, Serialize)]
pub struct Summary {
    pub started: bool,
    pub elapsed_ms: u64,
    pub stages_seen: [bool; 5],
    pub poses_sent: [u64; 5],
    pub source_interruptions: u64,
    pub complete: bool,
}
pub struct Follower {
    target: u32,
    offset: [f32; 3],
    latest: Option<(Pose, u64)>,
    began: Option<u64>,
    was_available: bool,
    summary: Summary,
}
impl Follower {
    pub fn new(target: u32, offset: [f32; 3]) -> Result<Self, &'static str> {
        if target == 0
            || target >= ECHO_BASE
            || offset.iter().any(|v| !v.is_finite() || v.abs() > 10_000.)
        {
            return Err("use a real player id and finite local offset within 100 m");
        }
        Ok(Self {
            target,
            offset,
            latest: None,
            began: None,
            was_available: false,
            summary: Summary::default(),
        })
    }
    pub fn event(&mut self, event: &Event, now: u64) {
        match event {
            Event::Reset => self.latest = None,
            Event::Roster(ids) if !ids.contains(&self.target) => self.latest = None,
            Event::Pose { pose, .. } if pose.player == self.target => {
                self.latest = Some((*pose, now));
            }
            _ => (),
        }
    }
    pub fn step(&mut self, now: u64) -> Step {
        let pose = self.latest.and_then(|(p, at)| {
            if now < at || now - at > FRESH_MS {
                return None;
            }
            let rotation = p.rotation.filter(|q| q.is_valid())?;
            let position = Position {
                x: p.position.x + self.offset[0],
                y: p.position.y + self.offset[1],
                z: p.position.z + self.offset[2],
            };
            (p.position.is_valid() && position.is_valid()).then_some((position, rotation))
        });
        let available = pose.is_some();
        if self.was_available && !available {
            self.summary.source_interruptions = self.summary.source_interruptions.saturating_add(1);
        }
        self.was_available = available;
        if available && self.began.is_none() {
            self.began = Some(now);
        }
        let Some(began) = self.began else {
            return Step {
                stage: None,
                state: PlayerState::Unavailable,
                pose: None,
                source_available: false,
            };
        };
        let elapsed = now.saturating_sub(began);
        let stage = match elapsed {
            0..15_000 => 1,
            15_000..25_000 => 2,
            25_000..40_000 => 3,
            40_000..50_000 => 4,
            _ => 5,
        };
        let intended = match stage {
            2 => PlayerState::Loading,
            4 => PlayerState::Unavailable,
            _ => PlayerState::Driving,
        };
        if available {
            self.summary.stages_seen[stage as usize - 1] = true;
        }
        self.summary.started = true;
        self.summary.elapsed_ms = elapsed;
        self.summary.complete = elapsed >= CYCLE_MS
            && self.summary.stages_seen.iter().all(|v| *v)
            && [0, 2, 4].iter().all(|i| self.summary.poses_sent[*i] > 0);
        Step {
            stage: Some(stage),
            state: if available {
                intended
            } else {
                PlayerState::Unavailable
            },
            pose: if intended == PlayerState::Driving {
                pose
            } else {
                None
            },
            source_available: available,
        }
    }
    pub fn sent(&mut self, stage: u8) {
        if let Some(count) = stage
            .checked_sub(1)
            .and_then(|s| self.summary.poses_sent.get_mut(s as usize))
        {
            *count = count.saturating_add(1);
        }
    }
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
    pub fn into_summary(self) -> Summary {
        self.summary
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pose(player: u32) -> Event {
        Event::Pose {
            pose: Pose {
                player,
                generation: 1,
                sequence: 1,
                received_ms: 0,
                position: Position {
                    x: 10.,
                    y: 20.,
                    z: 30.,
                },
                rotation: Some(Rotation {
                    x: 0.,
                    y: 0.,
                    z: 0.,
                    w: 1.,
                }),
            },
            reset_history: true,
        }
    }
    #[test]
    fn waits_for_the_selected_source_and_preserves_position_rotation_and_offset() {
        let mut f = Follower::new(1, [400., 0., 0.]).unwrap();
        assert_eq!(f.step(1000).stage, None);
        f.event(&pose(2), 1000);
        assert!(!f.step(1000).source_available);
        f.event(&pose(1), 2000);
        let s = f.step(2000);
        assert_eq!(s.stage, Some(1));
        assert_eq!(s.pose.unwrap().0.x, 410.);
        assert_eq!(s.pose.unwrap().1.w, 1.);
        f.sent(1);
        assert_eq!(f.summary().poses_sent, [1, 0, 0, 0, 0]);
        assert!(Follower::new(ECHO_BASE, [0.; 3]).is_err());
        assert!(Follower::new(0, [0.; 3]).is_err());
        assert!(Follower::new(1, [f32::NAN, 0., 0.]).is_err());
    }
    #[test]
    fn finite_cycle_pauses_poses_but_keeps_explicit_states_and_requires_all_stages() {
        let mut f = Follower::new(1, [0.; 3]).unwrap();
        for (now, stage, state, sends) in [
            (0, 1, PlayerState::Driving, true),
            (14_999, 1, PlayerState::Driving, true),
            (15_000, 2, PlayerState::Loading, false),
            (24_999, 2, PlayerState::Loading, false),
            (25_000, 3, PlayerState::Driving, true),
            (40_000, 4, PlayerState::Unavailable, false),
            (50_000, 5, PlayerState::Driving, true),
            (75_000, 5, PlayerState::Driving, true),
        ] {
            f.event(&pose(1), now);
            let s = f.step(now);
            assert_eq!(
                (s.stage, s.state, s.pose.is_some()),
                (Some(stage), state, sends)
            );
            if sends {
                f.sent(stage);
            }
        }
        assert!(f.summary().complete);
        let mut skipped = Follower::new(1, [0.; 3]).unwrap();
        skipped.event(&pose(1), 0);
        skipped.step(0);
        skipped.event(&pose(1), CYCLE_MS);
        skipped.step(CYCLE_MS);
        assert!(!skipped.summary().complete);
    }
    #[test]
    fn stale_poses_reset_and_departures_never_keep_inventing_coordinates() {
        let mut f = Follower::new(1, [0.; 3]).unwrap();
        f.event(&pose(1), 0);
        assert!(f.step(0).pose.is_some());
        assert_eq!(f.step(501).state, PlayerState::Unavailable);
        assert!(f.step(1000).pose.is_none());
        assert_eq!(f.summary().source_interruptions, 1);
        f.event(&pose(1), 2000);
        assert!(f.step(2000).pose.is_some());
        f.event(&Event::Reset, 2001);
        assert!(f.step(2001).pose.is_none());
        f.event(&pose(1), 3000);
        f.step(3000);
        f.event(&Event::Roster(vec![2]), 3001);
        assert!(f.step(3001).pose.is_none());
    }
}

//! Ordered appearance-only authority. The sidecar in the supplied seed is
//! frozen; this is not the live ledger, engine clock, admission or renderer.
use crate::{
    career_entry, vehicle_appearance as va,
    world_join::{self as join, Change, Context, Delta, Receiver, Snapshot},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};

// Fits alongside a bounded career manifest in the existing 256 KiB TCP frame.
pub const MAX_SEED_JSON: usize = 128 * 1024;
pub const HISTORY: usize = 128;
pub const BATCH: usize = 32;

pub mod replica;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub career_manifest_sha256: String,
    pub content_sha256: String,
    pub catalogue_sha256: String,
    pub world_epoch: u64,
    pub authority_generation: u64,
}
impl Pin {
    pub fn new(context: &Context<'_>, epoch: u64, authority: u64) -> Result<Self, String> {
        Receiver::new(
            Context {
                career: context.career,
                content: context.content,
                catalogue: context.catalogue,
            },
            epoch,
            authority,
        )?;
        Ok(Self {
            career_manifest_sha256: career_entry::manifest_hash(context.career)?,
            content_sha256: context.content.fingerprint()?,
            catalogue_sha256: context.catalogue.fingerprint(context.content)?,
            world_epoch: epoch,
            authority_generation: authority,
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Pull { after: Option<u64> },
    Publish { change: Change },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Published,
    Duplicate,
    Stale,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Payload {
    Snapshot {
        snapshot: Box<Snapshot>,
    },
    Events {
        after: u64,
        through: u64,
        head: u64,
        deltas: Vec<Delta>,
    },
    Accepted {
        sequence: u64,
        disposition: Disposition,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub schema: u32,
    pub pin: Pin,
    pub payload: Payload,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
impl Reply {
    pub fn validate(
        &self,
        context: Context<'_>,
        expected: &Pin,
        operation: &Operation,
    ) -> Result<(), String> {
        if self.schema != 1
            || &self.pin != expected
            || Pin::new(
                &context,
                expected.world_epoch,
                expected.authority_generation,
            )? != *expected
            || self.engine_verified
            || self.world_synchronized
        {
            return Err("appearance reply context/flags differ".into());
        }
        let mut receiver =
            Receiver::new(context, expected.world_epoch, expected.authority_generation)?;
        match (&self.payload, operation) {
            (Payload::Snapshot { snapshot }, Operation::Pull { after }) => {
                bounded(snapshot)?;
                if after.is_some_and(|after| snapshot.appearance_cut < after) {
                    return Err("appearance snapshot predates requested cursor".into());
                }
                receiver.begin((**snapshot).clone())?;
                receiver.prepare()?;
            }
            (
                Payload::Events {
                    after,
                    through,
                    head,
                    deltas,
                },
                Operation::Pull {
                    after: Some(requested),
                },
            ) => {
                if after != requested || deltas.len() > BATCH || through > head {
                    return Err("appearance event page bounds differ".into());
                }
                let mut sequence = *after;
                for delta in deltas {
                    receiver.validate_delta(delta)?;
                    if sequence.checked_add(1) != Some(delta.sequence) {
                        return Err("appearance event page is not contiguous".into());
                    }
                    sequence = delta.sequence;
                }
                if sequence != *through || (deltas.is_empty() && through != head) {
                    return Err("appearance event page cursor differs".into());
                }
            }
            (
                Payload::Accepted {
                    sequence,
                    disposition,
                },
                Operation::Publish { .. },
            ) => {
                if *sequence == 0 && *disposition == Disposition::Published {
                    return Err("invalid appearance publication sequence".into());
                }
            }
            _ => return Err("unexpected appearance reply".into()),
        }
        Ok(())
    }
}
fn bounded(snapshot: &Snapshot) -> Result<(), String> {
    if serde_json::to_vec(snapshot)
        .map_err(|_| "cannot encode appearance seed")?
        .len()
        > MAX_SEED_JSON
    {
        return Err("appearance transport seed exceeds 128 KiB".into());
    }
    Ok(())
}
pub struct Authority {
    pin: Pin,
    seed: Snapshot,
    board: va::Board,
    players: BTreeSet<String>,
    history: VecDeque<Delta>,
}
impl Authority {
    pub fn new(context: Context<'_>, snapshot: Snapshot) -> Result<Self, String> {
        bounded(&snapshot)?;
        let pin = Pin::new(
            &context,
            snapshot.world_epoch,
            snapshot.authority_generation,
        )?;
        let mut r = Receiver::new(context, pin.world_epoch, pin.authority_generation)?;
        r.begin(snapshot.clone())?;
        let prepared = r.prepare()?;
        let players = snapshot
            .sidecar
            .ranking
            .iter()
            .map(|c| c.player.clone())
            .collect();
        Ok(Self {
            pin,
            seed: snapshot,
            board: prepared.appearances,
            players,
            history: VecDeque::new(),
        })
    }
    pub fn pin(&self) -> &Pin {
        &self.pin
    }
    pub fn head(&self) -> u64 {
        self.seed.appearance_cut
    }
    pub fn retained(&self) -> usize {
        self.history.len()
    }
    pub fn execute(
        &mut self,
        principal: &str,
        pin: &Pin,
        operation: &Operation,
    ) -> Result<Reply, String> {
        if pin != &self.pin || !self.players.contains(principal) {
            return Err("appearance stream context/company differs".into());
        }
        let payload = match operation {
            Operation::Pull { after } => self.pull(*after)?,
            Operation::Publish { change } => self.publish(principal, change)?,
        };
        Ok(Reply {
            schema: 1,
            pin: self.pin.clone(),
            payload,
            engine_verified: false,
            world_synchronized: false,
        })
    }
    fn pull(&self, after: Option<u64>) -> Result<Payload, String> {
        let head = self.seed.appearance_cut;
        if after.is_some_and(|after| after > head) {
            return Err("appearance cursor is ahead of this authority".into());
        }
        let retained = after.is_some_and(|after| {
            after == head
                || self
                    .history
                    .front()
                    .is_some_and(|d| after.checked_add(1).is_some_and(|next| next >= d.sequence))
        });
        if !retained {
            return Ok(Payload::Snapshot {
                snapshot: Box::new(self.seed.clone()),
            });
        }
        let after = after.unwrap();
        let deltas: Vec<_> = self
            .history
            .iter()
            .filter(|d| d.sequence > after)
            .take(BATCH)
            .cloned()
            .collect();
        let through = deltas.last().map_or(after, |d| d.sequence);
        Ok(Payload::Events {
            after,
            through,
            head,
            deltas,
        })
    }
    fn publish(&mut self, principal: &str, change: &Change) -> Result<Payload, String> {
        let (vehicles, retired) = self.board.snapshot();
        let mut next = self.board.clone();
        let disposition = match change {
            Change::Appearance { description } => {
                if description.player != principal {
                    return Err("appearance publisher differs from ticket identity".into());
                }
                match next.apply(description.clone())? {
                    va::Applied::Updated => Disposition::Published,
                    va::Applied::Duplicate => Disposition::Duplicate,
                    va::Applied::Stale => Disposition::Stale,
                }
            }
            Change::Remove {
                player,
                vehicle,
                generation,
            } => {
                if player != principal || !crate::session_protocol::identity_valid(vehicle) {
                    return Err("appearance removal publisher/vehicle differs".into());
                }
                if next
                    .get(player)
                    .is_some_and(|d| d.generation == *generation && d.vehicle != *vehicle)
                {
                    return Err("appearance removal vehicle differs".into());
                }
                next.remove(player, *generation)?;
                if next.snapshot() == (vehicles, retired) {
                    Disposition::Stale
                } else {
                    Disposition::Published
                }
            }
        };
        if disposition != Disposition::Published {
            return Ok(Payload::Accepted {
                sequence: self.seed.appearance_cut,
                disposition,
            });
        }
        let sequence = self
            .seed
            .appearance_cut
            .checked_add(1)
            .ok_or("appearance sequence exhausted")?;
        let delta = Delta {
            schema: 1,
            world_epoch: self.pin.world_epoch,
            authority_generation: self.pin.authority_generation,
            sequence,
            change: change.clone(),
        };
        let delta_bytes =
            serde_json::to_vec(&delta).map_err(|_| "cannot encode appearance delta")?;
        if delta_bytes.len() > join::MAX_DELTA_JSON {
            return Err("appearance delta exceeds transport limit".into());
        }
        let (appearances, retired) = next.snapshot();
        let mut seed = self.seed.clone();
        seed.appearance_cut = sequence;
        seed.appearances = appearances;
        seed.retired = retired;
        bounded(&seed)?;
        self.board = next;
        self.seed = seed;
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(delta);
        Ok(Payload::Accepted {
            sequence,
            disposition,
        })
    }
}

#[cfg(test)]
mod tests;

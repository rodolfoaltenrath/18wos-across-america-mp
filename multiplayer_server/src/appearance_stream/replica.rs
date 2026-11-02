//! Stateful, bounded staging of the appearance-only stream. No engine access.
//! A checkpoint is private local data, not an authenticated server statement.
use super::{Operation, Payload, Pin, Reply};
use crate::{
    career,
    world_join::{Context, Receiver, Snapshot},
};
use serde::{Deserialize, Serialize};

pub const MAX_CHECKPOINT_JSON: usize = 192 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    schema: u32,
    pin: Pin,
    snapshot: Snapshot,
    observed_head: u64,
    sha256: String,
    engine_verified: bool,
    world_synchronized: bool,
}
impl Checkpoint {
    fn digest(&self) -> Result<String, String> {
        Ok(career::hash(
            &serde_json::to_vec(&(&self.pin, &self.snapshot, self.observed_head))
                .map_err(|_| "cannot hash appearance checkpoint")?,
        ))
    }
}

/// Counts and cursors only. Reaching an observed head is not engine readiness,
/// nor a claim that the server has not advanced since its last reply.
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub through_sequence: Option<u64>,
    pub observed_head: Option<u64>,
    pub at_observed_head: bool,
    pub appearances: usize,
    pub retired: usize,
    pub seed_sidecar_frozen: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}

pub struct Replica<'a> {
    context: Context<'a>,
    pin: Pin,
    snapshot: Option<Snapshot>,
    observed_head: u64,
}
impl<'a> Replica<'a> {
    pub fn new(context: Context<'a>, expected: &Pin) -> Result<Self, String> {
        if Pin::new(
            &context,
            expected.world_epoch,
            expected.authority_generation,
        )? != *expected
        {
            return Err("appearance replica context differs".into());
        }
        Ok(Self {
            context,
            pin: expected.clone(),
            snapshot: None,
            observed_head: 0,
        })
    }
    fn context(&self) -> Context<'a> {
        Context {
            career: self.context.career,
            content: self.context.content,
            catalogue: self.context.catalogue,
        }
    }
    fn receiver(&self) -> Result<Receiver<'a>, String> {
        Receiver::new(
            self.context(),
            self.pin.world_epoch,
            self.pin.authority_generation,
        )
    }
    pub fn from_bytes(context: Context<'a>, expected: &Pin, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_CHECKPOINT_JSON {
            return Err("appearance checkpoint exceeds 192 KiB".into());
        }
        let checkpoint: Checkpoint =
            serde_json::from_slice(bytes).map_err(|_| "invalid appearance checkpoint JSON")?;
        if checkpoint.schema != 1
            || checkpoint.pin != *expected
            || checkpoint.engine_verified
            || checkpoint.world_synchronized
            || checkpoint.observed_head < checkpoint.snapshot.appearance_cut
            || checkpoint.sha256 != checkpoint.digest()?
        {
            return Err("appearance checkpoint context/hash/claims differ".into());
        }
        super::bounded(&checkpoint.snapshot)?;
        let mut replica = Self::new(context, expected)?;
        let mut receiver = replica.receiver()?;
        receiver.begin(checkpoint.snapshot.clone())?;
        receiver.prepare()?;
        replica.snapshot = Some(checkpoint.snapshot);
        replica.observed_head = checkpoint.observed_head;
        Ok(replica)
    }
    fn encode(&self, snapshot: &Snapshot, head: u64) -> Result<Vec<u8>, String> {
        super::bounded(snapshot)?;
        let mut checkpoint = Checkpoint {
            schema: 1,
            pin: self.pin.clone(),
            snapshot: snapshot.clone(),
            observed_head: head,
            sha256: String::new(),
            engine_verified: false,
            world_synchronized: false,
        };
        checkpoint.sha256 = checkpoint.digest()?;
        let bytes = serde_json::to_vec_pretty(&checkpoint)
            .map_err(|_| "cannot encode appearance checkpoint")?;
        if bytes.len() > MAX_CHECKPOINT_JSON {
            return Err("appearance checkpoint exceeds 192 KiB".into());
        }
        Ok(bytes)
    }
    pub fn checkpoint_bytes(&self) -> Result<Vec<u8>, String> {
        self.encode(
            self.snapshot
                .as_ref()
                .ok_or("appearance replica has no snapshot")?,
            self.observed_head,
        )
    }
    pub fn next_pull(&self) -> Operation {
        Operation::Pull {
            after: self.snapshot.as_ref().map(|s| s.appearance_cut),
        }
    }
    pub fn report(&self) -> Report {
        Report {
            schema: 1,
            through_sequence: self.snapshot.as_ref().map(|s| s.appearance_cut),
            observed_head: self.snapshot.as_ref().map(|_| self.observed_head),
            at_observed_head: self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.appearance_cut == self.observed_head),
            appearances: self.snapshot.as_ref().map_or(0, |s| s.appearances.len()),
            retired: self.snapshot.as_ref().map_or(0, |s| s.retired.len()),
            seed_sidecar_frozen: true,
            engine_verified: false,
            world_synchronized: false,
        }
    }
    /// Validates and prepares an entire page before publishing any local state.
    /// Errors preserve the previous state, including cut/head/checkpoint floors.
    pub fn apply(&mut self, reply: &Reply) -> Result<(), String> {
        reply.validate(self.context(), &self.pin, &self.next_pull())?;
        let head = match &reply.payload {
            Payload::Snapshot { snapshot } => snapshot.appearance_cut,
            Payload::Events { head, .. } => *head,
            Payload::Accepted { .. } => {
                return Err("appearance replica requires a pull reply".into());
            }
        };
        if head < self.observed_head {
            return Err("appearance reply predates already observed head".into());
        }
        let mut receiver = self.receiver()?;
        if let Some(previous) = &self.snapshot {
            receiver.begin(previous.clone())?;
        }
        let mut next = match &reply.payload {
            Payload::Snapshot { snapshot } => {
                if self.snapshot.is_some() {
                    receiver.prepare()?; // retain floors before replacing the base
                }
                receiver.begin((**snapshot).clone())?;
                (**snapshot).clone()
            }
            Payload::Events { deltas, .. } => {
                let previous = self
                    .snapshot
                    .as_ref()
                    .ok_or("appearance page needs a base snapshot")?;
                for delta in deltas {
                    receiver.receive(delta.clone())?;
                }
                previous.clone()
            }
            Payload::Accepted { .. } => unreachable!(),
        };
        let prepared = receiver.prepare()?;
        next.appearance_cut = prepared.through_sequence;
        next.sidecar = prepared.sidecar;
        (next.appearances, next.retired) = prepared.appearances.snapshot();
        self.encode(&next, head)?; // also bound the materialized checkpoint
        self.snapshot = Some(next);
        self.observed_head = head;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

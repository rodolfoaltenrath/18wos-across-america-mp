//! Offline staging barrier for EXISTING sidecar ledger + declared appearances.
//! Not a full world snapshot: no traffic, physics, engine-save or clock adapter.
//! Caller authenticates source and orders this appearance-only event stream.
use crate::{
    career, career_entry, career_world,
    session_protocol::identity_valid,
    vehicle_appearance::{self as va, Board, Catalogue, Description},
    world_content,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_SNAPSHOT_JSON: usize = career::MAX_METADATA;
pub const MAX_DELTA_JSON: usize = 3072;
pub const MAX_EVENT_FILE: usize = 512 * 1024;
pub const MAX_PENDING: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    SidecarAppearanceOnly,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: u32,
    pub scope: Scope,
    pub world_epoch: u64,
    pub authority_generation: u64,
    pub career_manifest_sha256: String,
    pub content_sha256: String,
    pub catalogue_sha256: String,
    /// Cut of this APPEARANCE stream, not a physics tick or ledger sequence.
    pub appearance_cut: u64,
    pub sidecar: career_world::Snapshot,
    /// Strict durable-player order. All must have an existing sidecar company.
    pub appearances: Vec<Description>,
    pub retired: Vec<va::Retired>,
}
impl Snapshot {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_SNAPSHOT_JSON {
            return Err("join snapshot exceeds 1 MiB".into());
        }
        serde_json::from_slice(bytes).map_err(|_| "invalid join snapshot JSON".into())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Appearance {
        description: Description,
    },
    Remove {
        player: String,
        vehicle: String,
        generation: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delta {
    pub schema: u32,
    pub world_epoch: u64,
    pub authority_generation: u64,
    pub sequence: u64,
    pub change: Change,
}
impl Delta {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_DELTA_JSON {
            return Err("appearance delta exceeds 3072 bytes".into());
        }
        serde_json::from_slice(bytes).map_err(|_| "invalid appearance delta JSON".into())
    }
}
pub struct Context<'a> {
    pub career: &'a career::Manifest,
    pub content: &'a world_content::Manifest,
    pub catalogue: &'a Catalogue,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    AwaitingSnapshot,
    Buffering,
    NeedsSnapshot,
    Prepared,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Received {
    Queued,
    Duplicate,
    BeforeCut,
}
pub struct Receiver<'a> {
    context: Context<'a>,
    epoch: u64,
    authority: u64,
    career_hash: String,
    content_hash: String,
    catalogue_hash: String,
    phase: Phase,
    initial: Option<Snapshot>,
    pending: Vec<Delta>,
    through: u64,
    minimum_cut: u64,
    minimum_checkpoint: u64,
    minimum_clock: u64,
    appearance_floor_hash: Option<String>,
    checkpoint_floor_hash: Option<String>,
}
pub struct Prepared {
    pub sidecar: career_world::Snapshot,
    pub appearances: Board,
    pub through_sequence: u64,
    report: Report,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub scope: Scope,
    pub phase: Phase,
    pub world_epoch: u64,
    pub authority_generation: u64,
    pub snapshot_cut: u64,
    pub through_sequence: u64,
    pub staged_events: usize,
    pub companies: usize,
    pub sidecar_checkpoint_revision: u64,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
impl Prepared {
    pub fn report(&self) -> &Report {
        &self.report
    }
}
impl<'a> Receiver<'a> {
    pub fn new(context: Context<'a>, epoch: u64, authority: u64) -> Result<Self, String> {
        if epoch == 0 || authority == 0 {
            return Err("join context must have nonzero world/authority".into());
        }
        context.career.validate()?;
        context
            .career
            .compatibility
            .compare(&context.content.legacy()?)?;
        let career_hash = career_entry::manifest_hash(context.career)?;
        let content_hash = context.content.fingerprint()?;
        let catalogue_hash = context.catalogue.fingerprint(context.content)?;
        Ok(Self {
            context,
            epoch,
            authority,
            career_hash,
            content_hash,
            catalogue_hash,
            phase: Phase::AwaitingSnapshot,
            initial: None,
            pending: Vec::new(),
            through: 0,
            minimum_cut: 0,
            minimum_checkpoint: 0,
            minimum_clock: 0,
            appearance_floor_hash: None,
            checkpoint_floor_hash: None,
        })
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    fn board(&self, s: &Snapshot) -> Result<Board, String> {
        if s.schema != 1
            || s.world_epoch != self.epoch
            || s.authority_generation != self.authority
            || s.career_manifest_sha256 != self.career_hash
            || s.content_sha256 != self.content_hash
            || s.catalogue_sha256 != self.catalogue_hash
            || s.appearances.len() > va::MAX_VEHICLES
            || s.appearances.windows(2).any(|w| w[0].player >= w[1].player)
        {
            return Err("join snapshot context/order/identity differs".into());
        }
        s.sidecar.validate(self.context.career)?;
        let players: BTreeSet<_> = s
            .sidecar
            .ranking
            .iter()
            .map(|c| c.player.as_str())
            .collect();
        for d in &s.appearances {
            if !players.contains(d.player.as_str()) {
                return Err("appearance has no company in sidecar snapshot".into());
            }
        }
        Board::from_records(
            self.epoch,
            self.context.catalogue.clone(),
            self.context.content,
            &s.appearances,
            &s.retired,
        )
    }
    pub fn begin(&mut self, snapshot: Snapshot) -> Result<(), String> {
        self.board(&snapshot)?;
        if snapshot.appearance_cut < self.minimum_cut
            || snapshot.sidecar.checkpoint_revision < self.minimum_checkpoint
            || snapshot.sidecar.clock.world_ms < self.minimum_clock
        {
            return Err("join snapshot predates already observed cut/checkpoint/clock".into());
        }
        let appearance_hash = hash_appearances(&snapshot.appearances, &snapshot.retired)?;
        let checkpoint_hash = hash_checkpoint(&snapshot.sidecar)?;
        if (snapshot.appearance_cut == self.minimum_cut
            && self
                .appearance_floor_hash
                .as_ref()
                .is_some_and(|h| h != &appearance_hash))
            || (snapshot.sidecar.checkpoint_revision == self.minimum_checkpoint
                && self
                    .checkpoint_floor_hash
                    .as_ref()
                    .is_some_and(|h| h != &checkpoint_hash))
        {
            return Err("conflicting snapshot at an already observed cut/checkpoint".into());
        }
        self.appearance_floor_hash = Some(appearance_hash);
        self.checkpoint_floor_hash = Some(checkpoint_hash);
        self.minimum_checkpoint = snapshot.sidecar.checkpoint_revision;
        self.minimum_clock = snapshot.sidecar.clock.world_ms;
        self.through = snapshot.appearance_cut;
        self.minimum_cut = self.minimum_cut.max(self.through);
        self.pending.clear();
        self.initial = Some(snapshot);
        self.phase = Phase::Buffering;
        Ok(())
    }
    fn invalidate(&mut self, sequence: u64) {
        self.minimum_cut = self.minimum_cut.max(sequence).max(self.through);
        self.appearance_floor_hash = None;
        self.pending.clear();
        self.initial = None;
        self.phase = Phase::NeedsSnapshot;
    }
    fn validate_delta(&self, delta: &Delta) -> Result<(), String> {
        if delta.schema != 1
            || delta.world_epoch != self.epoch
            || delta.authority_generation != self.authority
            || delta.sequence == 0
        {
            return Err("appearance event context/sequence differs".into());
        }
        let bytes = serde_json::to_vec(delta).map_err(|_| "cannot encode appearance event")?;
        if bytes.len() > MAX_DELTA_JSON {
            return Err("appearance event exceeds budget".into());
        }
        match &delta.change {
            Change::Appearance { description } => {
                Description::parse(
                    &serde_json::to_vec(description).map_err(|_| "cannot encode appearance")?,
                    self.context.catalogue,
                )?;
                if description.world_epoch != self.epoch {
                    return Err("appearance from another world".into());
                }
            }
            Change::Remove {
                player,
                vehicle,
                generation,
            } => {
                if !identity_valid(player) || !identity_valid(vehicle) || *generation == 0 {
                    return Err("invalid appearance removal".into());
                }
            }
        }
        Ok(())
    }
    pub fn receive(&mut self, delta: Delta) -> Result<Received, String> {
        if self.phase != Phase::Buffering {
            return Err("snapshot barrier is not buffering".into());
        }
        self.validate_delta(&delta)?;
        let cut = self.initial.as_ref().unwrap().appearance_cut;
        if delta.sequence <= cut {
            return Ok(Received::BeforeCut);
        }
        if delta.sequence <= self.through {
            if self.pending.iter().any(|d| d == &delta) {
                return Ok(Received::Duplicate);
            }
            self.invalidate(self.through);
            return Err("conflicting repeated appearance sequence; new snapshot required".into());
        }
        if self.through.checked_add(1) != Some(delta.sequence) || self.pending.len() >= MAX_PENDING
        {
            self.invalidate(delta.sequence);
            return Err("appearance gap/overflow; new snapshot required".into());
        }
        self.through = delta.sequence;
        self.pending.push(delta);
        Ok(Received::Queued)
    }
    pub fn prepare(&mut self) -> Result<Prepared, String> {
        if self.phase != Phase::Buffering {
            return Err("snapshot barrier is not ready to prepare".into());
        }
        let snapshot = self.initial.as_ref().unwrap();
        let attempt = (|| {
            let mut board = self.board(snapshot)?;
            let players: BTreeSet<_> = snapshot
                .sidecar
                .ranking
                .iter()
                .map(|c| c.player.as_str())
                .collect();
            for delta in &self.pending {
                match &delta.change {
                    Change::Appearance { description } => {
                        if !players.contains(description.player.as_str()) {
                            return Err("new company requires updated sidecar snapshot".into());
                        }
                        board.apply(description.clone())?;
                    }
                    Change::Remove {
                        player,
                        vehicle,
                        generation,
                    } => {
                        if board
                            .get(player)
                            .is_some_and(|v| v.generation == *generation && v.vehicle != *vehicle)
                        {
                            return Err("appearance removal vehicle differs".into());
                        }
                        if !players.contains(player.as_str()) {
                            return Err("retirement has no company in snapshot".into());
                        }
                        board.remove(player, *generation)?;
                    }
                }
            }
            Ok(board)
        })();
        let board = match attempt {
            Ok(board) => board,
            Err(e) => {
                self.invalidate(self.through);
                return Err(e);
            }
        };
        let (vehicles, retired) = board.snapshot();
        self.appearance_floor_hash = Some(hash_appearances(&vehicles, &retired)?);
        let snapshot = self.initial.take().unwrap();
        let count = self.pending.len();
        self.pending.clear();
        self.minimum_cut = self.through;
        self.phase = Phase::Prepared;
        let report = Report {
            schema: 1,
            scope: Scope::SidecarAppearanceOnly,
            phase: self.phase,
            world_epoch: self.epoch,
            authority_generation: self.authority,
            snapshot_cut: snapshot.appearance_cut,
            through_sequence: self.through,
            staged_events: count,
            companies: snapshot.sidecar.ranking.len(),
            sidecar_checkpoint_revision: snapshot.sidecar.checkpoint_revision,
            engine_verified: false,
            world_synchronized: false,
        };
        Ok(Prepared {
            sidecar: snapshot.sidecar,
            appearances: board,
            through_sequence: self.through,
            report,
        })
    }
}
fn hash_appearances(vehicles: &[Description], retired: &[va::Retired]) -> Result<String, String> {
    Ok(career::hash(
        &serde_json::to_vec(&(vehicles, retired)).map_err(|_| "cannot hash appearance state")?,
    ))
}
fn hash_checkpoint(s: &career_world::Snapshot) -> Result<String, String> {
    Ok(career::hash(
        &serde_json::to_vec(&(
            s.clock_checkpoint_world_ms,
            s.clock.paused,
            s.clock.scale,
            &s.ranking,
            &s.contracts,
        ))
        .map_err(|_| "cannot hash sidecar checkpoint")?,
    ))
}

#[cfg(test)]
mod tests;

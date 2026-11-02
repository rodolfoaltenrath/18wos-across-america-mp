//! Offline link from hashed candidate observations to a hash-pinned inventory.
//! Graph includes definition defaults AND candidate roots; never applies a merge
//! or claims that these resources are the engine's current selected appearance.
use crate::{
    vehicle_references::{self, Audit},
    world_content::{Input, Pinned, resource_key},
};
use anyhow::{Context, Result, ensure};
use multiplayer_server::{vehicle_appearance::observation::Observation, world_content::Manifest};
use serde::Serialize;
use std::path::Path;
const MAX_REPORT: usize = 8 * 1024 * 1024;
#[derive(Debug, Serialize)]
pub struct Slot {
    pub index: u8,
    pub definition_key: Option<String>,
    pub available: bool,
    pub sha256: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub content_sha256: String,
    pub observation_sha256: String,
    pub make_definition_key: String,
    pub make_in_content: bool,
    pub slots: Vec<Slot>,
    pub references: Option<Audit>,
    pub graph_includes_declared_defaults: bool,
    pub candidate_only: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub schema: u32,
    pub make_in_content: bool,
    pub slots: usize,
    pub unnamed_slots: usize,
    pub missing_slots: usize,
    pub definitions: usize,
    pub unparsed_or_missing_definitions: usize,
    pub ambiguous_references: usize,
    pub candidate_only: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn make_name(name: &str) -> Result<&str> {
    let make = name
        .strip_prefix("makes/truck/")
        .and_then(|s| s.strip_suffix("/description.def"))
        .context("make candidate is not a truck definition")?;
    ensure!(identifier(make), "unsupported make definition identifier");
    Ok(make)
}
fn slot_make(name: &str) -> Result<&str> {
    let pieces: Vec<_> = name.split('/').collect();
    ensure!(
        pieces.len() == 6
            && pieces[0] == "makes"
            && pieces[1] == "truck"
            && pieces[3] == "acc"
            && pieces[5] == "!desc.def"
            && identifier(pieces[2])
            && identifier(pieces[4]),
        "candidate slot is not a direct truck accessory definition"
    );
    Ok(pieces[2])
}
pub fn audit(content: &Manifest, inputs: &[Input], observation: &Observation) -> Result<Report> {
    observation.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        content.profile == observation.profile,
        "observation/content profile differs"
    );
    let observation_sha256 = observation.fingerprint().map_err(anyhow::Error::msg)?;
    let mut sources = Pinned::open(content, inputs)?;
    let make_in_content = sources.winner(&observation.make_definition_key).is_some();
    let make = if make_in_content {
        let name = sources
            .resource_name(&observation.make_definition_key)?
            .to_ascii_lowercase();
        Some(make_name(&name)?.to_owned())
    } else {
        None
    };
    let mut slots = Vec::new();
    let mut additional = Vec::new();
    for slot in &observation.slots {
        let winner = slot
            .definition_key
            .as_deref()
            .and_then(|key| sources.winner(key));
        let sha256 = winner.map(|w| w.sha256.clone());
        if sha256.is_some() {
            let name = sources
                .resource_name(slot.definition_key.as_deref().unwrap())?
                .to_ascii_lowercase();
            let candidate_make = slot_make(&name)?;
            ensure!(
                resource_key(&format!("makes/truck/{candidate_make}/description.def"))?
                    == observation.make_definition_key,
                "candidate slot belongs to another make"
            );
            // With no make root, traversal is intentionally skipped. Still
            // verify every resource we report as available, not just its name.
            if !make_in_content {
                sources.read(slot.definition_key.as_deref().unwrap(), 64 * 1024)?;
            }
            additional.push(name);
        }
        slots.push(Slot {
            index: slot.index,
            definition_key: slot.definition_key.clone(),
            available: sha256.is_some(),
            sha256,
        });
    }
    let references = if let Some(make) = make {
        Some(vehicle_references::traverse(
            content, sources, &make, additional,
        )?)
    } else {
        sources.finish()?;
        None
    };
    Ok(Report {
        schema: 1,
        content_sha256: content.fingerprint().map_err(anyhow::Error::msg)?,
        observation_sha256,
        make_definition_key: observation.make_definition_key.clone(),
        make_in_content,
        slots,
        references,
        graph_includes_declared_defaults: true,
        candidate_only: true,
        engine_verified: false,
        world_synchronized: false,
    })
}
pub fn create(
    content: &Manifest,
    inputs: &[Input],
    observation: &Observation,
    output: &Path,
) -> Result<Summary> {
    let report = audit(content, inputs, observation)?;
    let definitions = report
        .references
        .as_ref()
        .map_or(&[][..], |a| a.definitions.as_slice());
    let summary = Summary {
        schema: 1,
        make_in_content: report.make_in_content,
        slots: report.slots.len(),
        unnamed_slots: report
            .slots
            .iter()
            .filter(|s| s.definition_key.is_none())
            .count(),
        missing_slots: report
            .slots
            .iter()
            .filter(|s| s.definition_key.is_some() && !s.available)
            .count(),
        definitions: definitions.len(),
        unparsed_or_missing_definitions: definitions
            .iter()
            .filter(|d| d.status != vehicle_references::Status::LexedCandidates)
            .count(),
        ambiguous_references: definitions
            .iter()
            .flat_map(|d| &d.references)
            .filter(|r| r.resolution == vehicle_references::Resolution::Ambiguous)
            .count(),
        candidate_only: true,
        engine_verified: false,
        world_synchronized: false,
    };
    let bytes = serde_json::to_vec_pretty(&report)?;
    ensure!(
        bytes.len() <= MAX_REPORT,
        "appearance reference report exceeds 8 MiB"
    );
    multiplayer_server::career_store::write_new(output, &bytes).map_err(anyhow::Error::msg)?;
    Ok(summary)
}

#[cfg(test)]
mod tests;

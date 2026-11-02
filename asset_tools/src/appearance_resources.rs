//! Verify bytes behind candidate runtime PSM keys. No extraction, format/paint
//! inference, catalogue generation or proof of mounted/current engine content.
use crate::world_content::{Input, Pinned};
use anyhow::{Result, ensure};
use multiplayer_server::{
    vehicle_appearance::resources::ResourceObservation,
    world_content::{Manifest, Winner},
};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
const MAX_TOTAL: u64 = 256 * 1024 * 1024;
const MAX_MODEL: usize = 64 * 1024 * 1024;
const MAX_MATERIAL: usize = 4 * 1024 * 1024;
const MAX_REPORT: usize = 128 * 1024;
#[derive(Clone, Debug, Serialize)]
pub struct Resource {
    pub key: String,
    /// Present bytes checked against the declared inventory, not format-valid.
    pub verified: Option<Winner>,
}
#[derive(Debug, Serialize)]
pub struct Instance {
    pub model: Resource,
    pub materials: Vec<Resource>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub content_sha256: String,
    pub observation_sha256: String,
    pub make_definition_key: String,
    /// Same three candidate field positions; no inferred assembly graph.
    pub instances: Vec<Option<Instance>>,
    pub unique_resources_verified: usize,
    pub verified_bytes: u64,
    pub candidate_only: bool,
    pub complete_assembly: bool,
    pub resource_formats_verified: bool,
    pub mounted_content_verified: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub schema: u32,
    pub instances: usize,
    pub absent_fields: usize,
    pub material_links: usize,
    pub missing_links: usize,
    pub unique_resources_verified: usize,
    pub verified_bytes: u64,
    pub candidate_only: bool,
    pub complete_assembly: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
struct Reader {
    sources: Pinned,
    checked: BTreeMap<String, Winner>,
    remaining: u64,
    bytes: u64,
}
impl Reader {
    fn resource(&mut self, key: &str, extension: &str, limit: usize) -> Result<Resource> {
        let Some(winner) = self.sources.winner(key).cloned() else {
            return Ok(Resource {
                key: key.into(),
                verified: None,
            });
        };
        let name = self.sources.resource_name(key)?;
        ensure!(
            name.to_ascii_lowercase().ends_with(extension),
            "candidate resource has an unsupported name/type"
        );
        if !self.checked.contains_key(key) {
            ensure!(
                winner.bytes <= limit as u64,
                "candidate resource exceeds per-file read budget"
            );
            ensure!(
                winner.bytes <= self.remaining,
                "candidate resources exceed total read budget"
            );
            self.sources.read(key, limit)?;
            self.remaining -= winner.bytes;
            self.bytes += winner.bytes;
            self.checked.insert(key.into(), winner.clone());
        }
        Ok(Resource {
            key: key.into(),
            verified: Some(winner),
        })
    }
}
fn audit_inner(
    content: &Manifest,
    inputs: &[Input],
    observation: &ResourceObservation,
    budget: u64,
) -> Result<Report> {
    observation.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        content.profile == observation.names.profile,
        "PSM observation/content profile differs"
    );
    let observation_sha256 = observation.fingerprint().map_err(anyhow::Error::msg)?;
    let mut reader = Reader {
        sources: Pinned::open(content, inputs)?,
        checked: BTreeMap::new(),
        remaining: budget,
        bytes: 0,
    };
    let mut instances = Vec::new();
    for instance in &observation.instances {
        instances.push(if let Some(i) = instance {
            let model = reader.resource(&i.model_key, ".psm", MAX_MODEL)?;
            let mut materials = Vec::new();
            for key in &i.material_keys {
                materials.push(reader.resource(key, ".shd", MAX_MATERIAL)?);
            }
            Some(Instance { model, materials })
        } else {
            None
        });
    }
    let count = reader.checked.len();
    let bytes = reader.bytes;
    reader.sources.finish()?;
    Ok(Report {
        schema: 1,
        content_sha256: content.fingerprint().map_err(anyhow::Error::msg)?,
        observation_sha256,
        make_definition_key: observation.names.make_definition_key.clone(),
        instances,
        unique_resources_verified: count,
        verified_bytes: bytes,
        candidate_only: true,
        complete_assembly: false,
        resource_formats_verified: false,
        mounted_content_verified: false,
        engine_verified: false,
        world_synchronized: false,
    })
}
pub fn audit(
    content: &Manifest,
    inputs: &[Input],
    observation: &ResourceObservation,
) -> Result<Report> {
    audit_inner(content, inputs, observation, MAX_TOTAL)
}
pub fn create(
    content: &Manifest,
    inputs: &[Input],
    observation: &ResourceObservation,
    output: &Path,
) -> Result<Summary> {
    let report = audit(content, inputs, observation)?;
    let summary = Summary {
        schema: 1,
        instances: report.instances.iter().flatten().count(),
        absent_fields: report.instances.iter().filter(|i| i.is_none()).count(),
        material_links: report
            .instances
            .iter()
            .flatten()
            .map(|i| i.materials.len())
            .sum(),
        missing_links: report
            .instances
            .iter()
            .flatten()
            .flat_map(|i| std::iter::once(&i.model).chain(&i.materials))
            .filter(|r| r.verified.is_none())
            .count(),
        unique_resources_verified: report.unique_resources_verified,
        verified_bytes: report.verified_bytes,
        candidate_only: true,
        complete_assembly: false,
        engine_verified: false,
        world_synchronized: false,
    };
    let bytes = serde_json::to_vec_pretty(&report)?;
    ensure!(
        bytes.len() <= MAX_REPORT,
        "PSM resource report exceeds 128 KiB"
    );
    multiplayer_server::career_store::write_new(output, &bytes).map_err(anyhow::Error::msg)?;
    Ok(summary)
}
#[cfg(test)]
mod tests;

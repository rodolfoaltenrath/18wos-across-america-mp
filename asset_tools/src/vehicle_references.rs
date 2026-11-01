//! Hashed, private metadata audit. Candidates in definition folders/shared are
//! NOT the engine's proved lookup/inheritance rules. Never generates a catalogue
//! or copies original definitions/models/textures into the report.
use crate::{
    localization::{codec, literals},
    world_content::{Input, Pinned, resource_key},
};
use anyhow::{Result, ensure};
use multiplayer_server::{career, world_content::Manifest};
use serde::Serialize;
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

const MAX_DEFINITIONS: usize = 128;
const MAX_DEFINITION_BYTES: usize = 64 * 1024;
const MAX_REPORT: usize = 8 * 1024 * 1024;
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    LexedCandidates,
    Missing,
    DecodeRefused,
    LexingRefused,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Candidate,
    Missing,
    Ambiguous,
    Unsupported,
    LabelOnly,
}
#[derive(Debug, Serialize)]
pub struct Candidate {
    pub key: String,
    pub sha256: String,
    pub declared_layer: usize,
}
#[derive(Debug, Serialize)]
pub struct Reference {
    pub field: String,
    pub ordinal: usize,
    pub token_sha256: String,
    pub resolution: Resolution,
    pub candidates: Vec<Candidate>,
}
#[derive(Debug, Serialize)]
pub struct Definition {
    pub key: String,
    pub sha256: Option<String>,
    pub status: Status,
    pub references: Vec<Reference>,
}
#[derive(Debug, Serialize)]
pub struct Audit {
    pub schema: u32,
    pub content_sha256: String,
    pub vehicle_definition_key: String,
    pub definitions: Vec<Definition>,
    pub deduplicated_targets: usize,
    pub offset_not_interpreted: bool,
    pub inheritance_not_applied: bool,
    pub lookup_rule: &'static str,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub schema: u32,
    pub definitions: usize,
    pub unparsed_or_missing: usize,
    pub references: usize,
    pub ambiguous: usize,
    pub unsupported: usize,
    pub missing: usize,
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
fn reference(
    sources: &Pinned,
    field: &str,
    ordinal: usize,
    token: &str,
    paths: Vec<String>,
) -> Result<(Reference, Vec<String>)> {
    let mut candidates = Vec::new();
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for path in paths {
        let Ok(key) = resource_key(&path) else {
            continue;
        };
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(w) = sources.winner(&key) {
            candidates.push(Candidate {
                key,
                sha256: w.sha256.clone(),
                declared_layer: w.layer,
            });
            found.push(path);
        }
    }
    candidates.sort_by(|a, b| a.key.cmp(&b.key));
    let resolution = match candidates.len() {
        0 => Resolution::Missing,
        1 => Resolution::Candidate,
        _ => Resolution::Ambiguous,
    };
    Ok((
        Reference {
            field: field.into(),
            ordinal,
            token_sha256: career::hash(token.as_bytes()),
            resolution,
            candidates,
        },
        found,
    ))
}
fn paths(folder: &str, shared: &str, token: &str, shader: bool) -> Option<Vec<String>> {
    let path = token.strip_prefix('/').unwrap_or(token);
    resource_key(path).ok()?;
    let relative = if folder.is_empty() {
        path.to_owned()
    } else {
        format!("{folder}/{path}")
    };
    if token.starts_with('/') {
        Some(vec![path.into()])
    } else if shader {
        Some(vec![relative, format!("{shared}/{path}")])
    } else {
        Some(vec![relative])
    }
}
pub fn audit(content: &Manifest, inputs: &[Input], vehicle: &str) -> Result<Audit> {
    ensure!(identifier(vehicle), "invalid vehicle identifier");
    let root = format!(
        "makes/truck/{}/description.def",
        vehicle.to_ascii_lowercase()
    );
    let root_key = resource_key(&root)?;
    let shared = format!("makes/truck/{}/acc/shared", vehicle.to_ascii_lowercase());
    let mut sources = Pinned::open(content, inputs)?;
    let mut queued = BTreeSet::from([root_key.clone()]);
    let mut queue = VecDeque::from([root]);
    let mut definitions = Vec::new();
    let mut deduplicated_targets = 0;
    while let Some(path) = queue.pop_front() {
        let key = resource_key(&path)?;
        let Some(winner) = sources.winner(&key) else {
            definitions.push(Definition {
                key,
                sha256: None,
                status: Status::Missing,
                references: vec![],
            });
            continue;
        };
        let hash = winner.sha256.clone();
        let raw = sources.read(&key, MAX_DEFINITION_BYTES)?;
        let Ok(decoded) = codec::decode(&raw) else {
            definitions.push(Definition {
                key,
                sha256: Some(hash),
                status: Status::DecodeRefused,
                references: vec![],
            });
            continue;
        };
        let Ok(fields) = literals::reference_fields(&decoded) else {
            definitions.push(Definition {
                key,
                sha256: Some(hash),
                status: Status::LexingRefused,
                references: vec![],
            });
            continue;
        };
        let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
        let mut references = Vec::new();
        for (field, value) in fields {
            if field == "type" {
                references.push(Reference {
                    field,
                    ordinal: 0,
                    token_sha256: career::hash(value.as_bytes()),
                    resolution: Resolution::LabelOnly,
                    candidates: vec![],
                });
                continue;
            }
            let access = field
                .strip_prefix("access")
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            let tokens: Vec<_> = if field == "shader" {
                value.split(':').collect()
            } else {
                vec![value.as_str()]
            };
            ensure!(tokens.len() <= 64, "too many shader reference candidates");
            for (ordinal, token) in tokens.into_iter().enumerate() {
                let locations = if let Some(number) = access {
                    let index = number.parse::<usize>().ok();
                    if index.is_some_and(|n| n < 32 && format!("access{n}") == field)
                        && identifier(token)
                    {
                        Some(vec![format!(
                            "makes/truck/{}/acc/{token}/!desc.def",
                            vehicle.to_ascii_lowercase()
                        )])
                    } else {
                        None
                    }
                } else if field == "extdefs" {
                    paths(folder, &shared, token, false)
                } else {
                    paths(folder, &shared, token, field == "shader")
                };
                let Some(locations) = locations else {
                    references.push(Reference {
                        field: field.clone(),
                        ordinal,
                        token_sha256: career::hash(token.as_bytes()),
                        resolution: Resolution::Unsupported,
                        candidates: vec![],
                    });
                    continue;
                };
                let (entry, found) = reference(&sources, &field, ordinal, token, locations)?;
                if access.is_some() || field == "extdefs" {
                    // Follow only a unique EXISTING definition candidate. No merge,
                    // priority guessing or implicit fallback for ambiguous imports.
                    if found.len() == 1 {
                        let target = &found[0];
                        let key = resource_key(target)?;
                        if queued.insert(key) {
                            ensure!(
                                queued.len() <= MAX_DEFINITIONS,
                                "definition traversal exceeds 128 targets"
                            );
                            queue.push_back(target.clone());
                        } else {
                            deduplicated_targets += 1;
                        }
                    }
                }
                references.push(entry);
            }
        }
        definitions.push(Definition {
            key,
            sha256: Some(hash),
            status: Status::LexedCandidates,
            references,
        });
    }
    sources.finish()?;
    definitions.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(Audit {
        schema: 1,
        content_sha256: content.fingerprint().map_err(anyhow::Error::msg)?,
        vehicle_definition_key: root_key,
        definitions,
        deduplicated_targets,
        offset_not_interpreted: true,
        inheritance_not_applied: true,
        lookup_rule: "candidate_definition_folder_and_vehicle_shared_only",
        engine_verified: false,
        world_synchronized: false,
    })
}
pub fn create(
    content: &Manifest,
    inputs: &[Input],
    vehicle: &str,
    output: &Path,
) -> Result<Summary> {
    let audit = audit(content, inputs, vehicle)?;
    let references: Vec<_> = audit
        .definitions
        .iter()
        .flat_map(|d| &d.references)
        .collect();
    let summary = Summary {
        schema: 1,
        definitions: audit.definitions.len(),
        unparsed_or_missing: audit
            .definitions
            .iter()
            .filter(|d| d.status != Status::LexedCandidates)
            .count(),
        references: references.len(),
        ambiguous: references
            .iter()
            .filter(|r| r.resolution == Resolution::Ambiguous)
            .count(),
        unsupported: references
            .iter()
            .filter(|r| r.resolution == Resolution::Unsupported)
            .count(),
        missing: references
            .iter()
            .filter(|r| r.resolution == Resolution::Missing)
            .count(),
        engine_verified: false,
        world_synchronized: false,
    };
    let bytes = serde_json::to_vec_pretty(&audit)?;
    ensure!(bytes.len() <= MAX_REPORT, "reference report exceeds 8 MiB");
    multiplayer_server::career_store::write_new(output, &bytes).map_err(anyhow::Error::msg)?;
    Ok(summary)
}

#[cfg(test)]
mod tests;

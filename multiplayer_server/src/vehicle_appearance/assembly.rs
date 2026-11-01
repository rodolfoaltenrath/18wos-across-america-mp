//! Declared multipart layout. Never calls the renderer or claims metadata
//! offsets/bones are known engine coordinates. Root schema-1 hashes stay intact.
use super::{Catalogue, Model, id_valid};
use crate::{protocol::Rotation, world_content::Manifest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_PARTS: usize = 32;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    Unresolved,
    /// Original definition values: units/axes/meaning remain unverified.
    DefinitionOffset {
        value: [f32; 3],
    },
    /// Operator-declared local transform, not evidence of the engine convention.
    LocalPose {
        position_cm: [f32; 3],
        rotation: [f32; 4],
    },
    Attachment {
        anchor: String,
    },
}
impl Placement {
    fn valid(&self) -> bool {
        let position = |v: &[f32; 3]| v.iter().all(|n| n.is_finite() && n.abs() <= 100_000.0);
        match self {
            Self::Unresolved => true,
            Self::DefinitionOffset { value } => position(value),
            Self::LocalPose {
                position_cm,
                rotation: q,
            } => {
                position(position_cm)
                    && Rotation {
                        x: q[0],
                        y: q[1],
                        z: q[2],
                        w: q[3],
                    }
                    .is_valid()
            }
            Self::Attachment { anchor } => id_valid(anchor),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub id: String,
    /// Absent is root; otherwise another declared part. No implicit hierarchy.
    pub parent: Option<String>,
    pub resource: String,
    pub dependencies: Vec<String>,
    pub placement: Placement,
}
/// Stable parent-before-child order; IDs are sorted for serialization, not draw.
pub(super) fn ordered(model: &Model) -> Result<Vec<&Part>, String> {
    if model.parts.len() > MAX_PARTS || model.parts.windows(2).any(|w| w[0].id >= w[1].id) {
        return Err("invalid model part count/order".into());
    }
    let mut pending = BTreeMap::new();
    for part in &model.parts {
        if !id_valid(&part.id)
            || part.id == "root"
            || !part.placement.valid()
            || part
                .parent
                .as_ref()
                .is_some_and(|id| id == &part.id || !model.parts.iter().any(|p| &p.id == id))
        {
            return Err("invalid model part/parent/placement".into());
        }
        pending.insert(part.id.as_str(), part);
    }
    let mut ready = BTreeSet::new();
    let mut out = Vec::new();
    while !pending.is_empty() {
        let next = pending
            .iter()
            .find(|(_, p)| p.parent.as_deref().is_none_or(|id| ready.contains(id)))
            .map(|(id, _)| *id)
            .ok_or("cyclic model part hierarchy")?;
        let part = pending.remove(next).unwrap();
        ready.insert(next);
        out.push(part);
    }
    Ok(out)
}
#[derive(Clone, Debug, Serialize)]
pub struct Asset {
    pub key: String,
    pub sha256: String,
    pub bytes: u64,
    pub declared_layer: usize,
}
#[derive(Debug, Serialize)]
pub struct Slot {
    pub slot: u16,
    pub resource: String,
}
#[derive(Debug, Serialize)]
pub struct PlannedPart {
    pub id: String,
    pub parent: Option<String>,
    pub resource: String,
    pub placement: Placement,
    pub materials: Vec<Slot>,
}
#[derive(Debug, Serialize)]
pub struct Plan {
    pub schema: u32,
    pub content_sha256: String,
    pub catalogue_sha256: String,
    pub model: String,
    pub paint: String,
    pub parts: Vec<PlannedPart>,
    pub assets: Vec<Asset>,
    pub all_placements_declared_local: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
pub fn plan(
    content: &Manifest,
    catalogue: &Catalogue,
    model: &str,
    paint: &str,
) -> Result<Plan, String> {
    let catalogue_sha256 = catalogue.fingerprint(content)?;
    let model = catalogue
        .models
        .iter()
        .find(|m| m.id == model)
        .ok_or("unknown assembly model")?;
    let paint = catalogue.paint(&model.id, paint)?;
    let materials = |id: &str| {
        paint
            .materials
            .iter()
            .filter(|m| m.part.as_deref().unwrap_or("root") == id)
            .map(|m| Slot {
                slot: m.slot,
                resource: m.resource.clone(),
            })
            .collect()
    };
    let mut parts = vec![PlannedPart {
        id: "root".into(),
        parent: None,
        resource: model.resource.clone(),
        placement: Placement::LocalPose {
            position_cm: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
        },
        materials: materials("root"),
    }];
    for part in ordered(model)? {
        parts.push(PlannedPart {
            id: part.id.clone(),
            parent: Some(part.parent.clone().unwrap_or("root".into())),
            resource: part.resource.clone(),
            placement: part.placement.clone(),
            materials: materials(&part.id),
        });
    }
    let mut keys = BTreeSet::new();
    keys.insert(model.resource.clone());
    keys.extend(model.dependencies.iter().cloned());
    keys.extend(paint.dependencies.iter().cloned());
    for part in &model.parts {
        keys.insert(part.resource.clone());
        keys.extend(part.dependencies.iter().cloned());
    }
    keys.extend(paint.materials.iter().map(|m| m.resource.clone()));
    let resolved = content.resolve()?;
    let assets = keys
        .into_iter()
        .map(|key| {
            let w = &resolved[&key];
            Asset {
                key,
                sha256: w.sha256.clone(),
                bytes: w.bytes,
                declared_layer: w.layer,
            }
        })
        .collect();
    Ok(Plan {
        schema: 1,
        content_sha256: content.fingerprint()?,
        catalogue_sha256,
        model: model.id.clone(),
        paint: paint.id.clone(),
        all_placements_declared_local: parts
            .iter()
            .all(|p| matches!(p.placement, Placement::LocalPose { .. })),
        parts,
        assets,
        engine_verified: false,
        world_synchronized: false,
    })
}

#[cfg(test)]
mod tests;

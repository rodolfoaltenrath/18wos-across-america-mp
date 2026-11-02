//! Bounded offline appearance catalogue and receiver state. This validates
//! declared resource references/revisions, not model formats or engine ABI.
//! No live wire, renderer, original save, arbitrary paths or engine calls.
use crate::{career, session_protocol::identity_valid, world_content::Manifest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod assembly;
pub mod observation;
pub mod resources;

pub const MAX_CATALOGUE_JSON: usize = 4 * 1024 * 1024;
pub const MAX_DESCRIPTION_JSON: usize = 2048;
pub const MAX_VEHICLES: usize = 64;
pub const MAX_RETIRED: usize = 128;
const MAX_REFERENCES: usize = 8192;

fn id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub slot: u16,
    pub resource: String,
    /// Absent is the root model. Extra part IDs are supported only in schema 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
}
impl Material {
    fn target(&self) -> (&str, u16) {
        (self.part.as_deref().unwrap_or("root"), self.slot)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paint {
    pub id: String,
    /// Increasing slot order. Several slots may intentionally use one material.
    pub materials: Vec<Material>,
    /// Declared additional resources, such as textures; not discovered by engine.
    pub dependencies: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    pub id: String,
    pub resource: String,
    pub dependencies: Vec<String>,
    pub paints: Vec<Paint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<assembly::Part>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub schema: u32,
    pub content_sha256: String,
    /// Increasing unique IDs; no remote filesystem names.
    pub models: Vec<Model>,
}
impl Catalogue {
    pub fn parse(bytes: &[u8], content: &Manifest) -> Result<Self, String> {
        if bytes.len() > MAX_CATALOGUE_JSON {
            return Err("appearance catalogue exceeds 4 MiB".into());
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| "invalid appearance catalogue JSON")?;
        value.validate(content)?;
        Ok(value)
    }
    pub fn validate(&self, content: &Manifest) -> Result<(), String> {
        if !matches!(self.schema, 1 | 2)
            || self.content_sha256 != content.fingerprint()?
            || self.models.is_empty()
            || self.models.len() > 64
            || self.models.windows(2).any(|w| w[0].id >= w[1].id)
        {
            return Err("appearance catalogue schema/content/model order differs".into());
        }
        let resolved = content.resolve()?;
        let mut references = 0usize;
        let mut resource = |key: &str| -> Result<(), String> {
            references += 1;
            if references > MAX_REFERENCES
                || !career::hash_valid(key)
                || !resolved.contains_key(key)
            {
                return Err("unknown appearance resource or reference budget exceeded".into());
            }
            Ok(())
        };
        let dependencies = |keys: &[String]| -> Result<(), String> {
            if keys.len() > 64 || keys.windows(2).any(|w| w[0] >= w[1]) {
                return Err("invalid appearance dependency order/bounds".into());
            }
            Ok(())
        };
        for model in &self.models {
            if !id_valid(&model.id)
                || model.paints.is_empty()
                || model.paints.len() > 32
                || model.paints.windows(2).any(|w| w[0].id >= w[1].id)
            {
                return Err("invalid model/paint IDs or bounds".into());
            }
            resource(&model.resource)?;
            if self.schema == 1 && !model.parts.is_empty() {
                return Err("extra model parts require catalogue schema 2".into());
            }
            assembly::ordered(model)?;
            for part in &model.parts {
                resource(&part.resource)?;
                dependencies(&part.dependencies)?;
                for key in &part.dependencies {
                    resource(key)?;
                }
            }
            dependencies(&model.dependencies)?;
            for key in &model.dependencies {
                resource(key)?;
            }
            for paint in &model.paints {
                if !id_valid(&paint.id)
                    || paint.materials.is_empty()
                    || paint.materials.len() > if self.schema == 1 { 32 } else { 256 }
                    || paint.materials.iter().any(|m| m.slot >= 64)
                    || paint
                        .materials
                        .windows(2)
                        .any(|w| w[0].target() >= w[1].target())
                {
                    return Err("invalid paint ID/material slots".into());
                }
                dependencies(&paint.dependencies)?;
                for key in &paint.dependencies {
                    resource(key)?;
                }
                for m in &paint.materials {
                    if let Some(part) = &m.part
                        && (self.schema == 1 || !model.parts.iter().any(|p| &p.id == part))
                    {
                        return Err("paint targets unknown/unsupported model part".into());
                    }
                    resource(&m.resource)?;
                }
            }
        }
        Ok(())
    }
    pub fn fingerprint(&self, content: &Manifest) -> Result<String, String> {
        self.validate(content)?;
        self.declared_hash()
    }
    fn declared_hash(&self) -> Result<String, String> {
        Ok(career::hash(
            &serde_json::to_vec(self).map_err(|_| "cannot encode catalogue")?,
        ))
    }
    fn paint(&self, model: &str, paint: &str) -> Result<&Paint, String> {
        self.models
            .iter()
            .find(|m| m.id == model)
            .and_then(|m| m.paints.iter().find(|p| p.id == paint))
            .ok_or("unknown model/paint in pinned catalogue".into())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Description {
    pub schema: u32,
    pub catalogue_sha256: String,
    pub world_epoch: u64,
    /// Durable identity declarations, not authentication credentials/UDP IDs.
    pub player: String,
    pub vehicle: String,
    pub generation: u64,
    pub revision: u64,
    pub model: String,
    pub paint: String,
}
impl Description {
    pub fn parse(bytes: &[u8], catalogue: &Catalogue) -> Result<Self, String> {
        if bytes.len() > MAX_DESCRIPTION_JSON {
            return Err("appearance description exceeds 2048 bytes".into());
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| "invalid appearance description JSON")?;
        value.validate(catalogue, &catalogue.declared_hash()?)?;
        Ok(value)
    }
    fn validate(&self, catalogue: &Catalogue, catalogue_sha256: &str) -> Result<(), String> {
        if self.schema != 1
            || !career::hash_valid(&self.catalogue_sha256)
            || self.catalogue_sha256 != catalogue_sha256
            || self.world_epoch == 0
            || self.generation == 0
            || self.revision == 0
            || !identity_valid(&self.player)
            || !identity_valid(&self.vehicle)
            || !id_valid(&self.model)
            || !id_valid(&self.paint)
        {
            return Err("invalid appearance identity/generation/revision".into());
        }
        catalogue.paint(&self.model, &self.paint)?;
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Updated,
    Duplicate,
    Stale,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Retired {
    pub player: String,
    pub generation: u64,
}

/// Caller owns admission/authorization. Pure board never authenticates peers or
/// applies anything to the engine. A world change requires a NEW validated board.
#[derive(Clone)]
pub struct Board {
    epoch: u64,
    catalogue: std::sync::Arc<Catalogue>,
    catalogue_sha256: String,
    vehicles: BTreeMap<String, Description>,
    retired: BTreeMap<String, u64>,
}
impl Board {
    pub fn new(epoch: u64, catalogue: Catalogue, content: &Manifest) -> Result<Self, String> {
        if epoch == 0 {
            return Err("world epoch must be nonzero".into());
        }
        let catalogue_sha256 = catalogue.fingerprint(content)?;
        Ok(Self {
            epoch,
            catalogue: std::sync::Arc::new(catalogue),
            catalogue_sha256,
            vehicles: BTreeMap::new(),
            retired: BTreeMap::new(),
        })
    }
    pub fn apply(&mut self, next: Description) -> Result<Applied, String> {
        next.validate(&self.catalogue, &self.catalogue_sha256)?;
        if next.world_epoch != self.epoch {
            return Err("appearance belongs to another world epoch".into());
        }
        if self
            .retired
            .get(&next.player)
            .is_some_and(|generation| next.generation <= *generation)
        {
            return Ok(Applied::Stale);
        }
        if let Some(old) = self.vehicles.get(&next.player) {
            if next.generation < old.generation {
                return Ok(Applied::Stale);
            }
            if next.generation == old.generation {
                if next.vehicle != old.vehicle {
                    return Err("vehicle switch requires new generation".into());
                }
                if next.revision < old.revision {
                    return Ok(Applied::Stale);
                }
                if next.revision == old.revision {
                    return if &next == old {
                        Ok(Applied::Duplicate)
                    } else {
                        Err("conflicting appearance at same revision".into())
                    };
                }
            }
        } else if self.vehicles.len() >= MAX_VEHICLES {
            return Err("appearance board full".into());
        }
        if self
            .vehicles
            .values()
            .any(|v| v.player != next.player && v.vehicle == next.vehicle)
        {
            return Err("vehicle already belongs to another player".into());
        }
        self.vehicles.insert(next.player.clone(), next);
        Ok(Applied::Updated)
    }
    pub fn get(&self, player: &str) -> Option<&Description> {
        self.vehicles.get(player)
    }
    pub fn remove(&mut self, player: &str, generation: u64) -> Result<bool, String> {
        if !identity_valid(player) || generation == 0 {
            return Err("invalid retirement identity/generation".into());
        }
        if self
            .vehicles
            .get(player)
            .is_some_and(|v| v.generation > generation)
            || self
                .retired
                .get(player)
                .is_some_and(|old| *old >= generation)
        {
            return Ok(false);
        }
        if !self.retired.contains_key(player) && self.retired.len() >= MAX_RETIRED {
            return Err("retirement history full; new snapshot required".into());
        }
        self.retired.insert(player.into(), generation);
        Ok(self.vehicles.remove(player).is_some())
    }
    pub fn snapshot(&self) -> (Vec<Description>, Vec<Retired>) {
        (
            self.vehicles.values().cloned().collect(),
            self.retired
                .iter()
                .map(|(player, generation)| Retired {
                    player: player.clone(),
                    generation: *generation,
                })
                .collect(),
        )
    }
    pub fn from_records(
        epoch: u64,
        catalogue: Catalogue,
        content: &Manifest,
        vehicles: &[Description],
        retired: &[Retired],
    ) -> Result<Self, String> {
        if vehicles.len() > MAX_VEHICLES
            || retired.len() > MAX_RETIRED
            || vehicles.windows(2).any(|w| w[0].player >= w[1].player)
            || retired.windows(2).any(|w| w[0].player >= w[1].player)
        {
            return Err("invalid appearance snapshot order/count".into());
        }
        let mut board = Self::new(epoch, catalogue, content)?;
        for r in retired {
            board.remove(&r.player, r.generation)?;
        }
        for v in vehicles {
            if board.apply(v.clone())? != Applied::Updated {
                return Err("appearance snapshot resurrects retired generation".into());
            }
        }
        Ok(board)
    }
    pub fn pose_ready(
        &self,
        epoch: u64,
        player: &str,
        vehicle: &str,
        generation: u64,
        revision: u64,
    ) -> bool {
        epoch == self.epoch
            && self.vehicles.get(player).is_some_and(|v| {
                v.vehicle == vehicle && v.generation == generation && v.revision == revision
            })
    }
}
#[derive(Debug, Serialize)]
pub struct CatalogueCheck {
    pub schema: u32,
    pub content_sha256: String,
    pub catalogue_sha256: String,
    pub models: usize,
    pub paints: usize,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
pub fn inspect(content: &Manifest, catalogue: &Catalogue) -> Result<CatalogueCheck, String> {
    Ok(CatalogueCheck {
        schema: 1,
        content_sha256: content.fingerprint()?,
        catalogue_sha256: catalogue.fingerprint(content)?,
        models: catalogue.models.len(),
        paints: catalogue.models.iter().map(|m| m.paints.len()).sum(),
        engine_verified: false,
        world_synchronized: false,
    })
}
#[derive(Debug, Serialize)]
pub struct Check {
    pub schema: u32,
    pub content_sha256: String,
    pub catalogue_sha256: String,
    pub model: String,
    pub paint: String,
    pub declared_material_slots: usize,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
pub fn check(
    content: &Manifest,
    catalogue: &Catalogue,
    description: &Description,
) -> Result<Check, String> {
    let catalogue_sha256 = catalogue.fingerprint(content)?;
    description.validate(catalogue, &catalogue_sha256)?;
    Ok(Check {
        schema: 1,
        content_sha256: content.fingerprint()?,
        catalogue_sha256,
        model: description.model.clone(),
        paint: description.paint.clone(),
        declared_material_slots: catalogue
            .paint(&description.model, &description.paint)?
            .materials
            .len(),
        engine_verified: false,
        world_synchronized: false,
    })
}

#[cfg(test)]
mod tests;

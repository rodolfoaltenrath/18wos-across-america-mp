//! Offline content contract. Order is operator-declared increasing priority,
//! NOT evidence of what the commercial engine mounted. No original paths/bytes.
use crate::career::{self, Compatibility};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_JSON: usize = 32 * 1024 * 1024;
pub const MAX_LAYERS: usize = 64;
pub const MAX_RESOURCES: usize = 100_000;
pub const MAX_ARCHIVE: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_RESOURCE: u64 = 64 * 1024 * 1024;
pub const MAX_EXPANDED: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Base,
    Map,
    Mod,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// SHA-256 of the canonical lowercase relative resource name, not a path to open.
    pub key: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub role: Role,
    pub sha256: String,
    pub bytes: u64,
    /// Strict key order; case aliases/duplicates within one archive are invalid.
    pub resources: Vec<Resource>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub profile: String,
    /// Lowest to highest DECLARED priority. Last resource definition wins here.
    pub layers: Vec<Layer>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Winner {
    pub layer: usize,
    pub sha256: String,
    pub bytes: u64,
    pub shadowed: usize,
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub schema: u32,
    pub content_equal: bool,
    pub effective_resources_equal: bool,
    pub profile_equal: bool,
    pub ordered_archives_equal: bool,
    pub inventories_equal: bool,
    pub differing_resources: usize,
    /// At most 16 hashed resource names; never their private local paths.
    pub differing_keys: Vec<String>,
    pub differing_keys_truncated: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
    pub order_source: &'static str,
}
impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_JSON {
            return Err("content manifest exceeds 32 MiB".into());
        }
        let m: Self = serde_json::from_slice(bytes)
            .map_err(|_| "invalid content manifest JSON".to_string())?;
        m.validate()?;
        Ok(m)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || !crate::session_protocol::profile_valid(&self.profile)
            || self.layers.is_empty()
            || self.layers.len() > MAX_LAYERS
            || self.layers.iter().filter(|l| l.role == Role::Map).count() != 1
        {
            return Err("invalid content schema/profile/layers/map role".into());
        }
        let mut archives = BTreeSet::new();
        let (mut count, mut expanded) = (0usize, 0u64);
        for l in &self.layers {
            count = count
                .checked_add(l.resources.len())
                .ok_or("resource count overflow")?;
            if !career::hash_valid(&l.sha256)
                || !(1..=MAX_ARCHIVE).contains(&l.bytes)
                || !archives.insert(&l.sha256)
                || count > MAX_RESOURCES
                || l.resources.windows(2).any(|w| w[0].key >= w[1].key)
                || (l.role == Role::Map && l.resources.is_empty())
            {
                return Err("invalid/duplicate archive or resource inventory".into());
            }
            for r in &l.resources {
                expanded = expanded
                    .checked_add(r.bytes)
                    .ok_or("expanded size overflow")?;
                if !career::hash_valid(&r.key)
                    || !career::hash_valid(&r.sha256)
                    || r.bytes > MAX_RESOURCE
                    || expanded > MAX_EXPANDED
                {
                    return Err("invalid resource hash/size/expanded budget".into());
                }
            }
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<String, String> {
        self.validate()?;
        Ok(career::hash(
            &serde_json::to_vec(self).map_err(|_| "cannot encode content manifest")?,
        ))
    }
    pub fn resolve(&self) -> Result<BTreeMap<String, Winner>, String> {
        self.validate()?;
        let mut resolved = BTreeMap::<String, Winner>::new();
        for (layer, l) in self.layers.iter().enumerate() {
            for r in &l.resources {
                let shadowed = resolved.get(&r.key).map_or(0, |w| w.shadowed + 1);
                resolved.insert(
                    r.key.clone(),
                    Winner {
                        layer,
                        sha256: r.sha256.clone(),
                        bytes: r.bytes,
                        shadowed,
                    },
                );
            }
        }
        Ok(resolved)
    }
    /// Projection only: old schema cannot preserve bases/order/resource winners.
    /// Calling this never upgrades an existing career's admission proof.
    pub fn legacy(&self) -> Result<Compatibility, String> {
        self.validate()?;
        let mut mods_sha256: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.role == Role::Mod)
            .map(|l| l.sha256.clone())
            .collect();
        mods_sha256.sort();
        let value = Compatibility {
            profile: self.profile.clone(),
            map_sha256: self
                .layers
                .iter()
                .find(|l| l.role == Role::Map)
                .unwrap()
                .sha256
                .clone(),
            mods_sha256,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn compare(&self, peer: &Self) -> Result<Comparison, String> {
        let a = self.resolve()?;
        let b = peer.resolve()?;
        let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
        let different: Vec<_> = keys
            .into_iter()
            .filter(|k| match (a.get(*k), b.get(*k)) {
                (Some(a), Some(b)) => a.sha256 != b.sha256 || a.bytes != b.bytes,
                _ => true,
            })
            .collect();
        let ordered_archives_equal = self
            .layers
            .iter()
            .map(|l| (l.role, &l.sha256, l.bytes))
            .eq(peer.layers.iter().map(|l| (l.role, &l.sha256, l.bytes)));
        let inventories_equal = self
            .layers
            .iter()
            .map(|l| &l.resources)
            .eq(peer.layers.iter().map(|l| &l.resources));
        Ok(Comparison {
            schema: 1,
            content_equal: self == peer,
            effective_resources_equal: different.is_empty(),
            profile_equal: self.profile == peer.profile,
            ordered_archives_equal,
            inventories_equal,
            differing_resources: different.len(),
            differing_keys: different.iter().take(16).map(|k| (*k).clone()).collect(),
            differing_keys_truncated: different.len() > 16,
            engine_verified: false,
            world_synchronized: false,
            order_source: "operator_declared_low_to_high",
        })
    }
}

#[cfg(test)]
mod tests;

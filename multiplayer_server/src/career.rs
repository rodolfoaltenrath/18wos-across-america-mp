//! Multiplayer sidecar contracts. No addresses, engine callbacks or original-save edits.
use crate::{identity, session_protocol};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCHEMA: u32 = 1;
pub const MAX_SAVE: usize = 64 * 1024 * 1024;
pub const MAX_METADATA: usize = 1024 * 1024;
pub const MAX_MODS: usize = 64;

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn hash_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    pub profile: String,
    pub map_sha256: String,
    /// Sorted content hashes, not filenames or local paths.
    pub mods_sha256: Vec<String>,
}
impl Compatibility {
    pub fn validate(&self) -> Result<(), String> {
        if !session_protocol::profile_valid(&self.profile)
            || !hash_valid(&self.map_sha256)
            || self.mods_sha256.len() > MAX_MODS
            || self.mods_sha256.iter().any(|h| !hash_valid(h))
            || self.mods_sha256.windows(2).any(|w| w[0] >= w[1])
        {
            return Err("invalid profile/map/mod fingerprints".into());
        }
        Ok(())
    }
    pub fn compare(&self, other: &Self) -> Result<(), String> {
        self.validate()?;
        other.validate()?;
        if self.profile != other.profile {
            return Err("game profile differs".into());
        }
        if self.map_sha256 != other.map_sha256 {
            return Err("map/archive hash differs".into());
        }
        if self.mods_sha256 != other.mods_sha256 {
            return Err("mod content set differs".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Goal {
    Unlimited,
    /// Exact minor units in the sidecar ledger; original game win trigger is untouched.
    Financial {
        minor_units: u64,
    },
}
impl Goal {
    pub fn validate(&self) -> Result<(), String> {
        if matches!(self, Self::Financial { minor_units } if !(1..=1_000_000_000_000_000).contains(minor_units))
        {
            return Err("financial goal outside supported bounds".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveBlob {
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub career: String,
    pub scenario: String,
    pub rules_revision: u32,
    pub goal: Goal,
    pub compatibility: Compatibility,
    pub bootstrap: SaveBlob,
}
impl Manifest {
    pub fn new(goal: Goal, compatibility: Compatibility, save: &[u8]) -> Result<Self, String> {
        let result = Self {
            schema: SCHEMA,
            career: identity::random_id()?,
            scenario: "become_millionaire".into(),
            rules_revision: 1,
            goal,
            compatibility,
            bootstrap: SaveBlob {
                bytes: save.len() as u64,
                sha256: hash(save),
            },
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || !session_protocol::identity_valid(&self.career)
            || self.scenario != "become_millionaire"
            || self.rules_revision != 1
            || !(1..=MAX_SAVE as u64).contains(&self.bootstrap.bytes)
            || !hash_valid(&self.bootstrap.sha256)
        {
            return Err("invalid career manifest/version/save bounds".into());
        }
        self.goal.validate()?;
        self.compatibility.validate()
    }
    pub fn verify_save(&self, bytes: &[u8]) -> Result<(), String> {
        self.validate()?;
        if bytes.len() as u64 != self.bootstrap.bytes || hash(bytes) != self.bootstrap.sha256 {
            return Err("save length/SHA-256 does not match host manifest".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn compatible() -> Compatibility {
        Compatibility {
            profile: session_protocol::PROFILE.into(),
            map_sha256: hash(b"own map fixture"),
            mods_sha256: vec![],
        }
    }
    #[test]
    fn rules_and_compatibility_refuse_divergence() {
        let c = compatible();
        let manifest = Manifest::new(Goal::Unlimited, c.clone(), b"own save fixture").unwrap();
        manifest.verify_save(b"own save fixture").unwrap();
        assert!(manifest.verify_save(b"different save").is_err());
        let mut other = c.clone();
        other.profile.push('x');
        assert_eq!(c.compare(&other).unwrap_err(), "game profile differs");
        other = c.clone();
        other.map_sha256 = hash(b"other map");
        assert!(c.compare(&other).unwrap_err().starts_with("map"));
        other = c.clone();
        other.mods_sha256.push(hash(b"mod"));
        assert!(c.compare(&other).unwrap_err().starts_with("mod"));
        other.mods_sha256.push(other.mods_sha256[0].clone());
        assert!(other.validate().is_err());
        assert!(Goal::Financial { minor_units: 0 }.validate().is_err());
        assert!(
            Goal::Financial {
                minor_units: u64::MAX
            }
            .validate()
            .is_err()
        );
        let mut invalid = manifest;
        invalid.schema += 1;
        assert!(invalid.validate().is_err());
    }
}

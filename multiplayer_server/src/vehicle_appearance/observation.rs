//! Shared candidate-only file contract. Not a wire appearance Description, a
//! verified engine selection or evidence that the named content was mounted.
use crate::{career, session_protocol};
use serde::{Deserialize, Serialize};
pub const MAX_JSON: usize = 16 * 1024;
pub const MAX_SLOTS: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    /// Observed table order only, not an accessory/material role.
    pub index: u8,
    pub definition_key: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    ProfiledActorNameCandidates,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub schema: u32,
    pub profile: String,
    pub source: Source,
    pub make_definition_key: String,
    pub slots: Vec<Slot>,
    pub candidate_only: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
impl Observation {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1
            || self.profile != session_protocol::PROFILE
            || !career::hash_valid(&self.make_definition_key)
            || self.slots.len() > MAX_SLOTS
            || !self.candidate_only
            || self.engine_verified
            || self.world_synchronized
        {
            return Err("invalid appearance candidate observation/claims".into());
        }
        for (i, s) in self.slots.iter().enumerate() {
            if usize::from(s.index) != i
                || s.definition_key
                    .as_ref()
                    .is_some_and(|key| !career::hash_valid(key))
            {
                return Err("invalid appearance candidate slots/order".into());
            }
        }
        Ok(())
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_JSON {
            return Err("appearance observation exceeds 16 KiB".into());
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| "invalid appearance observation JSON")?;
        value.validate()?;
        Ok(value)
    }
    pub fn fingerprint(&self) -> Result<String, String> {
        self.validate()?;
        Ok(career::hash(
            &serde_json::to_vec(self).map_err(|_| "cannot encode appearance observation")?,
        ))
    }
}

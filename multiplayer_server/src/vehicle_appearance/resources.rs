//! Candidate links observed through three profiled PSM fields. Not a complete
//! assembly, active paint selection, resource format proof or engine readiness.
use super::observation::Observation;
use crate::career;
use serde::{Deserialize, Serialize};
pub const MAX_JSON: usize = 64 * 1024;
pub const MAX_MATERIALS: usize = 64;
pub const INSTANCE_COUNT: usize = 3;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub model_key: String,
    /// PSM table order, not a mapping to a catalogue piece or paint slot.
    pub material_keys: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceObservation {
    pub schema: u32,
    pub names: Observation,
    /// Fixed field order: actor+30c, +310, +314. Null field stays None.
    pub instances: Vec<Option<Instance>>,
    pub candidate_only: bool,
    pub complete_assembly: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
impl ResourceObservation {
    pub fn validate(&self) -> Result<(), String> {
        self.names.validate()?;
        if self.schema != 1
            || self.instances.len() != INSTANCE_COUNT
            || !self.candidate_only
            || self.complete_assembly
            || self.engine_verified
            || self.world_synchronized
            || self.instances.iter().flatten().any(|i| {
                !career::hash_valid(&i.model_key)
                    || i.material_keys.len() > MAX_MATERIALS
                    || i.material_keys.iter().any(|k| !career::hash_valid(k))
            })
        {
            return Err("invalid PSM candidate observation or claims".into());
        }
        Ok(())
    }
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_JSON {
            return Err("PSM candidate observation exceeds 64 KiB".into());
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| "invalid PSM candidate observation JSON")?;
        value.validate()?;
        Ok(value)
    }
    pub fn fingerprint(&self) -> Result<String, String> {
        self.validate()?;
        Ok(career::hash(
            &serde_json::to_vec(self).map_err(|_| "cannot encode PSM candidates")?,
        ))
    }
}

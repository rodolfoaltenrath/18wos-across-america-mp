//! Bounded diagnostics about the running DLL, never installation paths or tokens.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    None,
    Simulated,
    Memory,
    Game,
    Drawn,
    GamePose,
    PoseFixture,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Profile {
    #[serde(rename = "steam-aa-1.10-ccb327c2")]
    Steam,
    #[serde(rename = "own_fixture")]
    Fixture,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub schema: u8,
    pub software_version: String,
    pub control_abi: u32,
    pub engine_abi: u32,
    pub source: Source,
    /// Selection/declaration, not a claim that game hashes or state were checked.
    pub declared_profile: Option<Profile>,
    pub wire_protocol: Option<u8>,
    pub physics_tick_atomic: bool,
}
pub(crate) fn valid_version(version: &str) -> bool {
    if version.is_empty()
        || version.len() > 48
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
    {
        return false;
    }
    let mut core = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.');
    (0..3).all(|_| {
        core.next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    }) && core.next().is_none()
}
impl Info {
    pub fn selected(mode: u32, protocol: u8) -> Self {
        use crate::api;
        let source = match mode {
            api::SIMULATED => Source::Simulated,
            api::MEMORY => Source::Memory,
            api::GAME => Source::Game,
            api::DRAWN => Source::Drawn,
            api::GAME_POSE => Source::GamePose,
            api::POSE_FIXTURE => Source::PoseFixture,
            _ => Source::None,
        };
        Self {
            schema: 1,
            software_version: env!("CARGO_PKG_VERSION").into(),
            control_abi: api::ABI_VERSION,
            engine_abi: api::ENGINE_ABI_VERSION,
            source,
            declared_profile: match source {
                Source::Game | Source::Drawn | Source::GamePose => Some(Profile::Steam),
                Source::PoseFixture => Some(Profile::Fixture),
                _ => None,
            },
            wire_protocol: (source != Source::None).then_some(protocol),
            physics_tick_atomic: false,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != 1
            || !valid_version(&self.software_version)
            || self.control_abi != 1
            || self.engine_abi != 3
            || self.physics_tick_atomic
            || !matches!(self.wire_protocol, None | Some(1 | 2))
            || (self.source == Source::None) != self.wire_protocol.is_none()
        {
            return Err("unsupported runtime information");
        }
        let expected = match self.source {
            Source::Game | Source::Drawn | Source::GamePose => Some(Profile::Steam),
            Source::PoseFixture => Some(Profile::Fixture),
            _ => None,
        };
        if self.declared_profile != expected {
            return Err("runtime source/profile mismatch");
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_modes_fit_the_existing_message_without_private_configuration() {
        for mode in 0..=6 {
            for wire in [1, 2] {
                let info = Info::selected(mode, wire);
                info.validate().unwrap();
                let json = serde_json::to_string(&info).unwrap();
                crate::api::put(&mut [0; 256], &json).unwrap();
                assert_eq!(serde_json::from_str::<Info>(&json).unwrap(), info);
            }
        }
        let mut info = Info::selected(crate::api::GAME_POSE, 2);
        info.physics_tick_atomic = true;
        assert!(info.validate().is_err());
        info.physics_tick_atomic = false;
        info.declared_profile = Some(Profile::Fixture);
        assert!(info.validate().is_err());
        assert!(serde_json::from_str::<Info>(r#"{"schema":1,"path":"private"}"#).is_err());
    }
}

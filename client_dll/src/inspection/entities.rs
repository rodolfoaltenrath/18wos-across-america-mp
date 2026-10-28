use super::process::Process;
use crate::{
    engine_discovery::{self, EntityRecord},
    profile,
};
use anyhow::{Context, Result, ensure};
use std::path::Path;
/// Hash checks gate all interpretation; only reads target memory.
pub fn registered(process: &Process) -> Result<Vec<EntityRecord>> {
    ensure!(
        process.identity.pointer_bytes == 4,
        "entity registry profile requires x86"
    );
    let modules = process.modules()?;
    let engine = modules
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("p3shared.dll"))
        .context("p3shared.dll not loaded")?;
    let game = modules
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("game.dll"))
        .context("game.dll not loaded")?;
    profile::verify_engine(Path::new(&engine.path)).map_err(anyhow::Error::msg)?;
    profile::verify_module(Path::new(&game.path)).map_err(anyhow::Error::msg)?;
    let head = u32::try_from(engine.base)?
        .checked_add(profile::ENTITY_REGISTRY_HEAD)
        .context("registry address overflow")?;
    engine_discovery::registry(head, 4096, |at, out| {
        process.read(at.into(), out).map_err(|e| e.to_string())
    })
    .map_err(anyhow::Error::msg)
}

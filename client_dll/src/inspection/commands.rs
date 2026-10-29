//! Profile-checked external reads, with no injection or command execution.
use super::process::Process;
use crate::{
    command_registry::{self, Audit},
    pe_exports::Image,
    profile,
};
use anyhow::{Context, Result, ensure};
use std::path::Path;
pub fn inspect(process: &Process) -> Result<Audit> {
    ensure!(
        process.identity.pointer_bytes == 4,
        "command audit requires x86"
    );
    let before = process.modules()?;
    for (name, verify) in [
        (
            "prism3d.exe",
            profile::verify_executable as fn(&Path) -> Result<(), String>,
        ),
        ("game.dll", profile::verify_module),
    ] {
        let m = before
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("{name} not loaded"))?;
        verify(Path::new(&m.path)).map_err(anyhow::Error::msg)?;
    }
    let images = before
        .iter()
        .map(|m| {
            Ok(Image {
                name: m.name.clone(),
                base: m.base,
                size: u32::try_from(m.size)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let audit = command_registry::inspect(&images, |at, out| {
        process.read(at, out).map_err(|e| e.to_string())
    })
    .map_err(anyhow::Error::msg)?;
    process.ensure_running()?;
    let after = process.modules()?;
    for name in ["prism3d.exe", "game.dll"] {
        let identity = |rows: &[super::process::Module]| {
            rows.iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| (m.base, m.size, m.path.clone()))
        };
        ensure!(
            identity(&before) == identity(&after),
            "command modules changed during audit"
        );
    }
    Ok(audit)
}

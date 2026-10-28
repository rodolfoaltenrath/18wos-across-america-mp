//! Profile-gated reads only. No injection, registration or engine callbacks.
use super::process::Process;
use crate::{
    pe_exports::Image,
    profile,
    resource_bindings::{self, Audit},
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub fn inspect(process: &Process) -> Result<Audit> {
    ensure!(
        process.identity.pointer_bytes == 4,
        "resource audit requires x86"
    );
    let modules = process.modules()?;
    for (name, verify) in [
        (
            "prism3d.exe",
            profile::verify_executable as fn(&Path) -> Result<(), String>,
        ),
        ("p3shared.dll", profile::verify_engine),
        ("game.dll", profile::verify_module),
    ] {
        let module = modules
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("{name} not loaded"))?;
        verify(Path::new(&module.path)).map_err(anyhow::Error::msg)?;
    }
    if let Some(module) = modules
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("psm.dll"))
    {
        profile::verify_psm(Path::new(&module.path)).map_err(anyhow::Error::msg)?;
    }
    let images = modules
        .iter()
        .map(|m| {
            Ok(Image {
                name: m.name.clone(),
                base: m.base,
                size: u32::try_from(m.size)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let report = resource_bindings::inspect(&images, |at, out| {
        process.read(at, out).map_err(|e| e.to_string())
    })
    .map_err(anyhow::Error::msg)?;
    process.ensure_running()?;
    let after = process.modules()?;
    for name in ["prism3d.exe", "p3shared.dll", "game.dll", "psm.dll"] {
        let identity = |rows: &[super::process::Module]| {
            rows.iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| (m.base, m.size, m.path.clone()))
        };
        ensure!(
            identity(&modules) == identity(&after),
            "resource modules changed during capture"
        );
    }
    Ok(report)
}

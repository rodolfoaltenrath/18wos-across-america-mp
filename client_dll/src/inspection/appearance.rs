//! Profile-checked read-only appearance candidates. No DLL injection, calls,
//! patches, network or automatic catalogue selection.
use super::{
    entities,
    process::{Module, Process},
};
use crate::{
    appearance_capture::{self, Layout, Observation},
    pose_capture, profile,
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub(super) fn verified(process: &Process) -> Result<(Vec<Module>, Layout<'static>)> {
    ensure!(
        process.identity.pointer_bytes == 4,
        "appearance candidates require x86"
    );
    process.ensure_running()?;
    let before = process.modules()?;
    for (name, verify) in [
        (
            "prism3d.exe",
            profile::verify_executable as fn(&Path) -> Result<(), String>,
        ),
        ("game.dll", profile::verify_module),
        ("p3shared.dll", profile::verify_engine),
    ] {
        let module = before
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .with_context(|| format!("{name} not loaded"))?;
        verify(Path::new(&module.path)).map_err(anyhow::Error::msg)?;
    }
    let base = u32::try_from(
        before
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case("game.dll"))
            .context("game.dll missing")?
            .base,
    )?;
    let add = |offset: u32| {
        base.checked_add(offset)
            .context("appearance profile address overflow")
    };
    let record = add(profile::CAR_RECORD)?;
    let rows = entities::registered(process)?;
    let car = rows
        .iter()
        .find(|r| r.name == "actors.car")
        .context("actors.car not registered")?;
    ensure!(
        car.address == record
            && car.false_size == profile::CAR_SIZE
            && car.false_constructor == add(profile::CAR_CONSTRUCTOR)?
            && car.false_destructor == add(profile::CAR_DESTRUCTOR)?,
        "appearance actor type differs from profile"
    );
    Ok((
        before,
        Layout {
            actor: pose_capture::Layout {
                root: add(profile::COMMAND_CONTEXT)?,
                steps: &[0x14c, 0x4610, 8, 8],
                vtable: add(profile::CAR_VTABLE)?,
            },
            type_record: record,
        },
    ))
}
#[derive(Debug)]
pub struct ReadFailure(pub pose_capture::Unavailable);
impl std::fmt::Display for ReadFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            pose_capture::Unavailable::Missing => "appearance chain or names unavailable",
            pose_capture::Unavailable::Overflow => "appearance address overflow",
            pose_capture::Unavailable::Type => "appearance actor or item differs from profile",
            pose_capture::Unavailable::Changed => "appearance changed during capture; retry",
            pose_capture::Unavailable::Invalid => {
                "appearance names, resource links or counts invalid or unsupported"
            }
        })
    }
}
impl std::error::Error for ReadFailure {}
fn unavailable(reason: pose_capture::Unavailable) -> anyhow::Error {
    anyhow::Error::new(ReadFailure(reason))
}
fn finish(process: &Process, before: &[Module]) -> Result<()> {
    process.ensure_running()?;
    let after = process.modules()?;
    for name in ["prism3d.exe", "game.dll", "p3shared.dll", "psm.dll"] {
        let identity = |modules: &[super::process::Module]| {
            modules
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| (m.base, m.size, m.path.clone()))
        };
        ensure!(
            identity(before) == identity(&after),
            "appearance modules changed during read"
        );
    }
    Ok(())
}
pub fn inspect(process: &Process) -> Result<Observation> {
    let (before, layout) = verified(process)?;
    let observation = appearance_capture::capture(layout, |at, out| {
        process.read(u64::from(at), out).map_err(|_| ())
    })
    .map_err(unavailable)?;
    observation.validate().map_err(anyhow::Error::msg)?;
    finish(process, &before)?;
    Ok(observation)
}
pub fn inspect_resources(
    process: &Process,
) -> Result<crate::appearance_resources::ResourceObservation> {
    let (before, layout) = verified(process)?;
    let psm = before
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("psm.dll"))
        .context("psm.dll not loaded")?;
    profile::verify_psm(Path::new(&psm.path)).map_err(anyhow::Error::msg)?;
    let ops = u32::try_from(psm.base)?
        .checked_add(0xe568)
        .context("PSM operations overflow")?;
    let observation = crate::appearance_resources::capture(layout, ops, |at, out| {
        process.read(u64::from(at), out).map_err(|_| ())
    })
    .map_err(unavailable)?;
    finish(process, &before)?;
    Ok(observation)
}

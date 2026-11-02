//! Profile-checked read-only appearance candidates. No DLL injection, calls,
//! patches, network or automatic catalogue selection.
use super::{entities, process::Process};
use crate::{
    appearance_capture::{self, Layout, Observation},
    pose_capture, profile,
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub fn inspect(process: &Process) -> Result<Observation> {
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
    let observation = appearance_capture::capture(
        Layout {
            actor: pose_capture::Layout {
                root: add(profile::COMMAND_CONTEXT)?,
                steps: &[0x14c, 0x4610, 8, 8],
                vtable: add(profile::CAR_VTABLE)?,
            },
            type_record: record,
        },
        |at, out| process.read(u64::from(at), out).map_err(|_| ()),
    )
    .map_err(|reason| {
        anyhow::anyhow!(match reason {
            pose_capture::Unavailable::Missing => "appearance chain or names unavailable",
            pose_capture::Unavailable::Overflow => "appearance address overflow",
            pose_capture::Unavailable::Type => "appearance actor or item differs from profile",
            pose_capture::Unavailable::Changed => "appearance changed during capture; retry",
            pose_capture::Unavailable::Invalid =>
                "appearance names or count invalid or unsupported",
        })
    })?;
    observation.validate().map_err(anyhow::Error::msg)?;
    process.ensure_running()?;
    let after = process.modules()?;
    for name in ["prism3d.exe", "game.dll", "p3shared.dll"] {
        let identity = |modules: &[super::process::Module]| {
            modules
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| (m.base, m.size, m.path.clone()))
        };
        ensure!(
            identity(&before) == identity(&after),
            "appearance modules changed during read"
        );
    }
    Ok(observation)
}

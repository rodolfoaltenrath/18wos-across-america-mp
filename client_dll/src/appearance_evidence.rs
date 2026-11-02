//! Opt-in candidate metadata collection, separate from connection diagnostics.
//! Local file order is not a global clock or a claim of visual synchronization.
use crate::{
    appearance_capture::Observation, appearance_resources::ResourceObservation, local_files,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
pub const MAX_CAPTURE: usize = 96 * 1024;
const MAX_SAMPLES: usize = 64;
const MANIFEST: &str = "manifest.json";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Start,
    Status,
    Stop,
    Manual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Missing,
    Overflow,
    Type,
    Changed,
    Invalid,
    GuardRefused,
    TargetUnavailable,
}
/// Fingerprints observed by this run's kit/isolated-loader preflight. These do
/// not assert an atomic hash of files loaded in the game process.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preflight {
    pub kit_sha256: String,
    pub dll_sha256: String,
}
impl Preflight {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            multiplayer_server::career::hash_valid(&self.kit_sha256)
                && multiplayer_server::career::hash_valid(&self.dll_sha256),
            "invalid appearance preflight hashes"
        );
        Ok(())
    }
}
pub fn load_preflight(path: &Path) -> Result<Preflight> {
    let p: Preflight = serde_json::from_slice(&local_files::read(path, 1024)?)?;
    p.validate()?;
    Ok(p)
}
impl From<crate::pose_capture::Unavailable> for Refusal {
    fn from(reason: crate::pose_capture::Unavailable) -> Self {
        use crate::pose_capture::Unavailable as U;
        match reason {
            U::Missing => Self::Missing,
            U::Overflow => Self::Overflow,
            U::Type => Self::Type,
            U::Changed => Self::Changed,
            U::Invalid => Self::Invalid,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema: u8,
    pub role: Role,
    pub preflight: Option<Preflight>,
    pub names: Option<Observation>,
    pub names_refusal: Option<Refusal>,
    pub resources: Option<ResourceObservation>,
    pub resources_refusal: Option<Refusal>,
    /// Two separate reads, not a simultaneous names/resources snapshot.
    pub names_agree: Option<bool>,
    pub candidate_only: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
impl Capture {
    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.preflight {
            p.validate()?;
        }
        ensure!(
            self.schema == 1
                && self.candidate_only
                && !self.engine_verified
                && !self.world_synchronized,
            "invalid appearance capture claims"
        );
        ensure!(
            self.names.is_some() != self.names_refusal.is_some()
                && self.resources.is_some() != self.resources_refusal.is_some(),
            "appearance result/refusal inconsistent"
        );
        if let Some(v) = &self.names {
            v.validate().map_err(anyhow::Error::msg)?;
        }
        if let Some(v) = &self.resources {
            v.validate().map_err(anyhow::Error::msg)?;
        }
        ensure!(
            self.names_agree
                == self
                    .names
                    .as_ref()
                    .zip(self.resources.as_ref())
                    .map(|(a, b)| a == &b.names),
            "appearance read agreement differs"
        );
        Ok(())
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_CAPTURE,
            "appearance capture exceeds 96 KiB"
        );
        let v: Self = serde_json::from_slice(bytes)?;
        v.validate()?;
        Ok(v)
    }
}
/// Invalid/unavailable readers stay absent; no paths, reasons or exception text
/// from either backend are copied. An unavailable read is not an empty truck.
pub fn collect(
    role: Role,
    names: impl FnOnce() -> Result<Observation, Refusal>,
    resources: impl FnOnce() -> Result<ResourceObservation, Refusal>,
) -> Capture {
    let names = names().and_then(|v| v.validate().map(|()| v).map_err(|_| Refusal::Invalid));
    let resources =
        resources().and_then(|v| v.validate().map(|()| v).map_err(|_| Refusal::Invalid));
    let names_refusal = names.as_ref().err().copied();
    let resources_refusal = resources.as_ref().err().copied();
    let names = names.ok();
    let resources = resources.ok();
    let names_agree = names
        .as_ref()
        .zip(resources.as_ref())
        .map(|(a, b)| a == &b.names);
    Capture {
        schema: 1,
        role,
        preflight: None,
        names,
        names_refusal,
        resources,
        resources_refusal,
        names_agree,
        candidate_only: true,
        engine_verified: false,
        world_synchronized: false,
    }
}
#[cfg(windows)]
pub fn collect_process(role: Role, process: &crate::inspection::process::Process) -> Capture {
    fn refused(error: anyhow::Error) -> Refusal {
        error
            .downcast_ref::<crate::inspection::appearance::ReadFailure>()
            .map_or(Refusal::GuardRefused, |e| e.0.into())
    }
    let capture = collect(
        role,
        || crate::inspection::appearance::inspect(process).map_err(refused),
        || crate::inspection::appearance::inspect_resources(process).map_err(refused),
    );
    if process.ensure_running().is_err() {
        collect(
            role,
            || Err(Refusal::TargetUnavailable),
            || Err(Refusal::TargetUnavailable),
        )
    } else {
        capture
    }
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn sample(index: usize) -> String {
    format!("appearance-{:04}.json", index + 1)
}
/// Append without overwriting; a partial write remains invalid evidence. The
/// new-file guard is not a global ordering/transaction guarantee across writers.
pub fn save_next(root: &Path, capture: &Capture) -> Result<PathBuf> {
    capture.validate()?;
    let bytes = serde_json::to_vec_pretty(capture)?;
    ensure!(
        bytes.len() <= MAX_CAPTURE,
        "appearance capture exceeds 96 KiB"
    );
    match local_files::metadata(root) {
        Ok(meta) => ensure!(meta.is_dir(), "appearance destination is not a directory"),
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            fs::create_dir(root)?
        }
        Err(e) => return Err(e),
    }
    for i in 0..MAX_SAMPLES {
        let p = root.join(sample(i));
        match write(&p, &bytes) {
            Ok(()) => return Ok(p),
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    anyhow::bail!("appearance sample limit reached")
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub captures: usize,
    pub names_available: usize,
    pub resources_available: usize,
    /// Totals across captures, not unique assets or a complete assembly count.
    pub model_fields_observed: usize,
    pub material_entries_observed: usize,
    pub disagreeing_reads: usize,
    pub roles: Vec<Role>,
    pub validation_claimed: bool,
}
fn summarize(captures: &[Capture]) -> Summary {
    Summary {
        captures: captures.len(),
        names_available: captures.iter().filter(|c| c.names.is_some()).count(),
        resources_available: captures.iter().filter(|c| c.resources.is_some()).count(),
        model_fields_observed: captures
            .iter()
            .filter_map(|c| c.resources.as_ref())
            .flat_map(|r| r.instances.iter().flatten())
            .count(),
        material_entries_observed: captures
            .iter()
            .filter_map(|c| c.resources.as_ref())
            .flat_map(|r| r.instances.iter().flatten())
            .map(|i| i.material_keys.len())
            .sum(),
        disagreeing_reads: captures
            .iter()
            .filter(|c| c.names_agree == Some(false))
            .count(),
        roles: captures.iter().map(|c| c.role).collect(),
        validation_claimed: false,
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub file: String,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u8,
    pub software_version: String,
    pub summary: Summary,
    pub files: Vec<Entry>,
}
/// Input is the dedicated appearance directory, never a whole run/backup.
pub fn bundle(source: &Path, output: &Path) -> Result<Manifest> {
    ensure!(
        local_files::metadata(source)?.is_dir(),
        "appearance source missing"
    );
    let allowed: BTreeSet<_> = (0..MAX_SAMPLES).map(sample).collect();
    let mut chosen = Vec::new();
    for (i, row) in fs::read_dir(source)?.enumerate() {
        ensure!(i < MAX_SAMPLES, "too many appearance samples");
        let row = row?;
        ensure!(
            row.file_name()
                .to_str()
                .is_some_and(|s| allowed.contains(s)),
            "unexpected appearance source member"
        );
        chosen.push(row.path());
    }
    chosen.sort();
    ensure!(!chosen.is_empty(), "no appearance samples available");
    let captures = chosen
        .iter()
        .map(|p| Capture::parse(&local_files::read(p, MAX_CAPTURE)?))
        .collect::<Result<Vec<_>>>()?;
    // Validate every source before creating the destination. Re-encode typed
    // metadata; do not copy raw logs, saves or even their original file names.
    fs::create_dir(output)?;
    write(
        &output.join(".INCOMPLETE"),
        b"Appearance export not finalized.\n",
    )?;
    let mut files = Vec::new();
    for (i, capture) in captures.iter().enumerate() {
        let bytes = serde_json::to_vec_pretty(capture)?;
        let file = sample(i);
        write(&output.join(&file), &bytes)?;
        files.push(Entry {
            file,
            bytes: bytes.len(),
            sha256: multiplayer_server::career::hash(&bytes),
        });
    }
    let manifest = Manifest {
        schema: 1,
        software_version: env!("CARGO_PKG_VERSION").into(),
        summary: summarize(&captures),
        files,
    };
    write(
        &output.join(MANIFEST),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::remove_file(output.join(".INCOMPLETE"))?;
    verify(output)
}
pub fn verify(root: &Path) -> Result<Manifest> {
    ensure!(
        local_files::metadata(root)?.is_dir(),
        "appearance bundle missing"
    );
    let m: Manifest = serde_json::from_slice(&local_files::read(&root.join(MANIFEST), 32 * 1024)?)?;
    ensure!(
        m.schema == 1
            && crate::runtime_info::valid_version(&m.software_version)
            && (1..=MAX_SAMPLES).contains(&m.files.len()),
        "invalid appearance manifest"
    );
    let mut allowed = BTreeSet::from([MANIFEST.to_owned()]);
    let mut captures = Vec::new();
    for (i, e) in m.files.iter().enumerate() {
        ensure!(
            e.file == sample(i)
                && e.bytes <= MAX_CAPTURE
                && multiplayer_server::career::hash_valid(&e.sha256),
            "invalid appearance member"
        );
        let bytes = local_files::read(&root.join(&e.file), MAX_CAPTURE)?;
        ensure!(
            bytes.len() == e.bytes && multiplayer_server::career::hash(&bytes) == e.sha256,
            "appearance member changed"
        );
        let capture = Capture::parse(&bytes)?;
        ensure!(
            serde_json::to_vec_pretty(&capture)? == bytes,
            "noncanonical appearance member"
        );
        captures.push(capture);
        allowed.insert(e.file.clone());
    }
    for (i, row) in fs::read_dir(root)?.enumerate() {
        ensure!(i < allowed.len(), "unexpected appearance bundle member");
        let row = row?;
        ensure!(
            row.file_name()
                .to_str()
                .is_some_and(|n| allowed.contains(n)),
            "unexpected appearance bundle member"
        );
    }
    ensure!(
        m.summary == summarize(&captures),
        "appearance summary differs"
    );
    Ok(m)
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub same_kit_preflight: Option<bool>,
    pub same_dll_preflight: Option<bool>,
    pub names_comparable: bool,
    pub make_equal: Option<bool>,
    pub differing_name_slots: Option<usize>,
    pub resources_comparable: bool,
    pub present_fields_left: Option<usize>,
    pub present_fields_right: Option<usize>,
    pub differing_resource_fields: Option<usize>,
    pub world_synchronized: bool,
    pub engine_verified: bool,
}
/// Explicitly selected pair. These may be different players/times; equality
/// of candidate fields is never an assertion of shared rendered appearance.
pub fn compare(a: &Capture, b: &Capture) -> Result<Comparison> {
    a.validate()?;
    b.validate()?;
    let names = a.names.as_ref().zip(b.names.as_ref());
    let resources = a.resources.as_ref().zip(b.resources.as_ref());
    let slots = names.map(|(a, b)| {
        (0..a.slots.len().max(b.slots.len()))
            .filter(|&i| a.slots.get(i) != b.slots.get(i))
            .count()
    });
    Ok(Comparison {
        same_kit_preflight: a
            .preflight
            .as_ref()
            .zip(b.preflight.as_ref())
            .map(|(a, b)| a.kit_sha256 == b.kit_sha256),
        same_dll_preflight: a
            .preflight
            .as_ref()
            .zip(b.preflight.as_ref())
            .map(|(a, b)| a.dll_sha256 == b.dll_sha256),
        names_comparable: names.is_some(),
        make_equal: names.map(|(a, b)| a.make_definition_key == b.make_definition_key),
        differing_name_slots: slots,
        resources_comparable: resources.is_some(),
        present_fields_left: a
            .resources
            .as_ref()
            .map(|r| r.instances.iter().flatten().count()),
        present_fields_right: b
            .resources
            .as_ref()
            .map(|r| r.instances.iter().flatten().count()),
        differing_resource_fields: resources.map(|(a, b)| {
            a.instances
                .iter()
                .zip(&b.instances)
                .filter(|(a, b)| a != b)
                .count()
        }),
        world_synchronized: false,
        engine_verified: false,
    })
}
pub fn load(path: &Path) -> Result<Capture> {
    Capture::parse(&local_files::read(path, MAX_CAPTURE)?)
}
#[cfg(test)]
mod tests;

//! Read-only inventory of explicitly ordered local ZIP/SCS layers. No mounting,
//! extraction, game discovery, network, save writes or private paths in output.
use anyhow::{Context, Result, ensure};
use multiplayer_server::{career, world_content as contract};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

#[derive(Clone, Debug)]
pub struct Input {
    pub role: contract::Role,
    pub path: PathBuf,
}
impl std::str::FromStr for Input {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (role, path) = s
            .split_once('=')
            .ok_or("layer must be base=FILE, map=FILE or mod=FILE")?;
        let role = match role {
            "base" => contract::Role::Base,
            "map" => contract::Role::Map,
            "mod" => contract::Role::Mod,
            _ => return Err("unknown layer role".into()),
        };
        ensure_path(path)?;
        Ok(Self {
            role,
            path: path.into(),
        })
    }
}
fn ensure_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.contains('\0') {
        return Err("missing/invalid layer file".into());
    }
    Ok(())
}
#[derive(Debug, Serialize)]
pub struct Summary {
    pub schema: u32,
    pub manifest_sha256: String,
    pub layers: usize,
    pub resources: usize,
    pub effective_resources: usize,
    pub overridden_resources: usize,
    pub engine_verified: bool,
    pub world_synchronized: bool,
    pub order_source: &'static str,
}
/// Final-file links/reparse points are rejected. This is not an ancestor/race sandbox.
fn regular(path: &Path) -> Result<File> {
    let meta = fs::symlink_metadata(path).context("cannot inspect content file")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            meta.file_attributes() & 0x400 == 0,
            "reparse content file refused"
        );
    }
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "regular content file required"
    );
    let file = File::open(path).context("cannot open content file")?;
    ensure!(
        file.metadata()?.is_file(),
        "opened content must be a regular file"
    );
    Ok(file)
}
fn digest(reader: &mut impl Read, max: u64) -> Result<(String, u64)> {
    let mut hash = Sha256::new();
    let mut read = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        read = read
            .checked_add(n as u64)
            .context("content byte count overflow")?;
        ensure!(read <= max, "content exceeds byte budget");
        hash.update(&buffer[..n]);
    }
    Ok((format!("{:x}", hash.finalize()), read))
}
pub fn resource_key(name: &str) -> Result<String> {
    ensure!(
        !name.is_empty() && !name.ends_with('/') && name.len() <= 240 && name.is_ascii(),
        "unsupported resource name"
    );
    super::safe_relative(name)?;
    Ok(career::hash(name.to_ascii_lowercase().as_bytes()))
}
fn entry_key(name: &str, is_dir: bool) -> Result<String> {
    resource_key(if is_dir {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    })
}
pub fn inventory(profile: &str, inputs: &[Input]) -> Result<contract::Manifest> {
    ensure!(
        crate_profile(profile) && !inputs.is_empty() && inputs.len() <= contract::MAX_LAYERS,
        "invalid profile/layer count"
    );
    let mut manifest = contract::Manifest {
        schema: 1,
        profile: profile.into(),
        layers: Vec::new(),
    };
    let (mut entries, mut expanded) = (0usize, 0u64);
    for input in inputs {
        let mut file = regular(&input.path)?;
        let before = file.metadata()?;
        ensure!(
            (1..=contract::MAX_ARCHIVE).contains(&before.len()),
            "archive size outside budget"
        );
        let (sha256, bytes) = digest(&mut file, before.len())?;
        ensure!(bytes == before.len(), "archive changed while hashing");
        file.seek(SeekFrom::Start(0))?;
        let mut zip = ZipArchive::new(file).context("unsupported ZIP/SCS content archive")?;
        ensure!(
            zip.len() <= contract::MAX_RESOURCES,
            "too many archive entries"
        );
        let mut seen = BTreeSet::new();
        let mut resources = Vec::new();
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            let name = entry.name()?;
            let key = entry_key(&name, entry.is_dir())?;
            if let Some(mode) = entry.unix_mode() {
                ensure!(
                    matches!(mode & 0o170000, 0 | 0o040000 | 0o100000),
                    "special archive member refused"
                );
            }
            ensure!(
                seen.insert(key.clone()),
                "duplicate/case-ambiguous archive member"
            );
            if entry.is_dir() {
                continue;
            }
            ensure!(
                entry.is_file() && entry.size() <= contract::MAX_RESOURCE,
                "resource exceeds size/type budget"
            );
            entries += 1;
            ensure!(
                entries <= contract::MAX_RESOURCES,
                "resource count exceeds shared budget"
            );
            expanded = expanded
                .checked_add(entry.size())
                .context("expanded count overflow")?;
            ensure!(
                expanded <= contract::MAX_EXPANDED,
                "expanded resources exceed shared budget"
            );
            let declared = entry.size();
            let (sha256, bytes) = digest(&mut entry, declared)?;
            ensure!(bytes == declared, "resource expanded size mismatch");
            resources.push(contract::Resource { key, sha256, bytes });
        }
        // Same handle is checked and hashed again after decoding. Detects ordinary
        // changes, not a malicious concurrent writer restoring the original bytes.
        let mut file = zip.into_inner();
        let after = file.metadata()?;
        ensure!(
            before.len() == after.len() && before.modified()? == after.modified()?,
            "archive changed during inventory"
        );
        file.seek(SeekFrom::Start(0))?;
        let (again, length) = digest(&mut file, before.len())?;
        ensure!(
            again == sha256 && length == bytes,
            "archive changed between inventory passes"
        );
        resources.sort_by(|a, b| a.key.cmp(&b.key));
        manifest.layers.push(contract::Layer {
            role: input.role,
            sha256,
            bytes,
            resources,
        });
    }
    manifest.validate().map_err(anyhow::Error::msg)?;
    Ok(manifest)
}
fn crate_profile(profile: &str) -> bool {
    multiplayer_server::session_protocol::profile_valid(profile)
}
pub fn create(profile: &str, inputs: &[Input], output: &Path) -> Result<Summary> {
    let manifest = inventory(profile, inputs)?;
    let resolved = manifest.resolve().map_err(anyhow::Error::msg)?;
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    ensure!(
        bytes.len() <= contract::MAX_JSON,
        "content JSON exceeds limit"
    );
    multiplayer_server::career_store::write_new(output, &bytes).map_err(anyhow::Error::msg)?;
    Ok(Summary {
        schema: 1,
        manifest_sha256: manifest.fingerprint().map_err(anyhow::Error::msg)?,
        layers: manifest.layers.len(),
        resources: manifest.layers.iter().map(|l| l.resources.len()).sum(),
        effective_resources: resolved.len(),
        overridden_resources: resolved.values().filter(|w| w.shadowed > 0).count(),
        engine_verified: false,
        world_synchronized: false,
        order_source: "operator_declared_low_to_high",
    })
}
pub fn load(path: &Path) -> Result<contract::Manifest> {
    let mut file = regular(path)?;
    ensure!(
        file.metadata()?.len() <= contract::MAX_JSON as u64,
        "content JSON exceeds limit"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(contract::MAX_JSON as u64 + 1)
        .read_to_end(&mut bytes)?;
    contract::Manifest::parse(&bytes).map_err(anyhow::Error::msg)
}
pub fn compare(host: &Path, peer: &Path) -> Result<contract::Comparison> {
    load(host)?
        .compare(&load(peer)?)
        .map_err(anyhow::Error::msg)
}

struct PinnedLayer {
    archive: ZipArchive<File>,
    indices: BTreeMap<String, usize>,
    bytes: u64,
    sha256: String,
    modified: std::time::SystemTime,
}
/// Keeps verified archive handles for a bounded batch of reads. Call finish
/// BEFORE publishing output. Not a sandbox or atomic snapshot against writers.
pub struct Pinned {
    layers: Vec<PinnedLayer>,
    resolved: BTreeMap<String, contract::Winner>,
}
impl Pinned {
    pub fn open(manifest: &contract::Manifest, inputs: &[Input]) -> Result<Self> {
        let resolved = manifest.resolve().map_err(anyhow::Error::msg)?;
        ensure!(
            inputs.len() == manifest.layers.len(),
            "content layer bindings differ"
        );
        let mut layers = Vec::new();
        for (input, expected) in inputs.iter().zip(&manifest.layers) {
            ensure!(
                input.role == expected.role,
                "content layer roles/order differ"
            );
            let mut file = regular(&input.path)?;
            let before = file.metadata()?;
            ensure!(
                before.len() == expected.bytes,
                "archive length differs from manifest"
            );
            let (hash, bytes) = digest(&mut file, expected.bytes)?;
            ensure!(
                hash == expected.sha256 && bytes == expected.bytes,
                "archive hash differs from manifest"
            );
            file.seek(SeekFrom::Start(0))?;
            let mut archive = ZipArchive::new(file)?;
            ensure!(
                archive.len() <= contract::MAX_RESOURCES,
                "archive entry count exceeds budget"
            );
            let mut indices = BTreeMap::new();
            let mut seen = BTreeSet::new();
            for index in 0..archive.len() {
                let entry = archive.by_index(index)?;
                let key = entry_key(&entry.name()?, entry.is_dir())?;
                ensure!(seen.insert(key.clone()), "ambiguous archive resource");
                if let Some(mode) = entry.unix_mode() {
                    ensure!(
                        matches!(mode & 0o170000, 0 | 0o040000 | 0o100000),
                        "special archive member refused"
                    );
                }
                if entry.is_dir() {
                    continue;
                }
                let r = expected
                    .resources
                    .binary_search_by(|r| r.key.cmp(&key))
                    .ok()
                    .map(|i| &expected.resources[i])
                    .context("resource absent from pinned inventory")?;
                ensure!(
                    entry.is_file() && entry.size() == r.bytes,
                    "resource size/type differs from inventory"
                );
                indices.insert(key, index);
            }
            ensure!(
                indices.len() == expected.resources.len(),
                "resource inventory member count differs"
            );
            layers.push(PinnedLayer {
                archive,
                indices,
                bytes: expected.bytes,
                sha256: expected.sha256.clone(),
                modified: before.modified()?,
            });
        }
        Ok(Self { layers, resolved })
    }
    pub fn winner(&self, key: &str) -> Option<&contract::Winner> {
        self.resolved.get(key)
    }
    /// Private VFS name of the declared winner, never a filesystem extraction path.
    pub fn resource_name(&mut self, key: &str) -> Result<String> {
        let winner = self
            .resolved
            .get(key)
            .context("resource absent from pinned content")?;
        let layer = &mut self.layers[winner.layer];
        let entry = layer.archive.by_index(layer.indices[key])?;
        let name = entry.name()?;
        ensure!(resource_key(&name)? == key, "pinned resource name differs");
        Ok(name.into_owned())
    }
    pub fn read(&mut self, key: &str, limit: usize) -> Result<Vec<u8>> {
        ensure!(
            limit > 0 && limit <= contract::MAX_RESOURCE as usize,
            "invalid pinned read budget"
        );
        let expected = self
            .resolved
            .get(key)
            .context("resource absent from pinned content")?;
        ensure!(
            expected.bytes <= limit as u64,
            "pinned resource exceeds read budget"
        );
        let layer = &mut self.layers[expected.layer];
        let mut entry = layer.archive.by_index(layer.indices[key])?;
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= limit
                && bytes.len() as u64 == expected.bytes
                && career::hash(&bytes) == expected.sha256,
            "pinned resource hash/length differs"
        );
        Ok(bytes)
    }
    pub fn finish(self) -> Result<()> {
        for layer in self.layers {
            let mut file = layer.archive.into_inner();
            let after = file.metadata()?;
            ensure!(
                after.len() == layer.bytes && after.modified()? == layer.modified,
                "archive changed during pinned reads"
            );
            file.seek(SeekFrom::Start(0))?;
            let (hash, bytes) = digest(&mut file, layer.bytes)?;
            ensure!(
                hash == layer.sha256 && bytes == layer.bytes,
                "archive changed between pinned passes"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

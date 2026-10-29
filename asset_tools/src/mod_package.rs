//! Reproducible data-only overlays. Staging does not mount anything in the game.
//! Origin is a declaration, not proof of authorship or a package signature.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Write},
    path::{Component, Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_FILE: usize = 16 * 1024 * 1024;
const MAX_TOTAL: usize = 32 * 1024 * 1024;
const MAX_JSON: usize = 1024 * 1024;
const MAX_FILES: usize = 1024;
const MAX_ARCHIVE: usize = MAX_TOTAL + MAX_JSON;
const MANIFEST: &str = "manifest.json";
const PAYLOAD: &str = "overlay.scs";
const KIND: &str = "18wos-aa-data-overlay";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Owned,
    DerivedLocal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub source: String,
    pub path: String,
    pub origin: Origin,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema: u8,
    pub id: String,
    pub version: String,
    pub files: Vec<Input>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub input: Input,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u8,
    pub kind: String,
    pub id: String,
    pub version: String,
    pub distributable: bool,
    pub payload_sha256: String,
    pub files: Vec<Entry>,
}

#[derive(Debug, Serialize)]
pub struct BuildReport {
    pub reused: bool,
    pub manifest: Manifest,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
}

fn label(value: &str) -> Result<()> {
    ensure!(
        (1..=64).contains(&value.len())
            && value
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"-_.".contains(&c))
            && value.as_bytes()[0].is_ascii_alphanumeric(),
        "id/version must be 1..64 lowercase ASCII letters, digits, dots, hyphens or underscores"
    );
    Ok(())
}

fn relative(value: &str) -> Result<PathBuf> {
    ensure!(
        value.is_ascii() && value.len() <= 240 && !value.ends_with('/'),
        "invalid overlay path"
    );
    super::safe_relative(value)
}

fn asset_path(value: &str) -> Result<()> {
    let path = relative(value)?;
    let ext = super::extension(&path);
    ensure!(
        matches!(
            ext.as_str(),
            "def" | "shd" | "mat" | "txt" | "tga" | "png" | "dds" | "bmp" | "jpg" | "jpeg"
        ),
        "unsupported data-only asset extension: {value}"
    );
    ensure!(
        !value.split('/').any(|c| matches!(
            c.to_ascii_lowercase().as_str(),
            "save" | "saves" | ".git" | ".cargo" | "target" | "backups"
        )),
        "private/generated path in overlay"
    );
    Ok(())
}

/// Check every existing ancestor, including Windows junctions. This is for
/// a local tree with no concurrent writers, not a handle-relative sandbox.
fn checked(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for part in absolute.components() {
        ensure!(
            !matches!(part, Component::ParentDir),
            "parent components are unsupported"
        );
        current.push(part);
        // A verbatim disk/UNC prefix is not a complete Windows path until its
        // root component has been appended (metadata("\\\\?\\C:") fails).
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "links are unsupported: {}",
                    current.display()
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        meta.file_attributes() & 0x400 == 0,
                        "reparse points are unsupported"
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("checking {}", current.display())),
        }
    }
    Ok(absolute)
}

fn read(path: &Path, cap: usize) -> Result<Vec<u8>> {
    let path = checked(path)?;
    let meta = fs::metadata(&path)?;
    ensure!(
        meta.is_file() && meta.len() <= cap as u64,
        "nonregular/oversized file: {}",
        path.display()
    );
    let mut bytes = Vec::new();
    File::open(&path)?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= cap && bytes.len() as u64 == meta.len(),
        "file changed size or exceeded limit"
    );
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn fresh_dir(path: &Path) -> Result<PathBuf> {
    let path = checked(path)?;
    ensure!(
        path.parent().is_some_and(Path::is_dir),
        "destination parent must already exist"
    );
    fs::create_dir(&path).context("destination must be a NEW directory")?;
    Ok(path)
}

fn contents(path: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
    let path = checked(path)?;
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(&path)? {
        names.insert(entry?.file_name());
        ensure!(names.len() <= 2, "unexpected extra files in package");
    }
    ensure!(
        names == BTreeSet::from([MANIFEST.into(), PAYLOAD.into()]),
        "package must contain ONLY manifest.json and overlay.scs"
    );
    Ok((
        read(&path.join(MANIFEST), MAX_JSON)?,
        read(&path.join(PAYLOAD), MAX_ARCHIVE)?,
    ))
}

fn graph(files: &[Input]) -> Result<()> {
    ensure!(
        (1..=MAX_FILES).contains(&files.len()),
        "package requires 1..=1024 files"
    );
    let mut names = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for entry in files {
        asset_path(&entry.path)?;
        relative(&entry.source)?;
        let prefix = match entry.origin {
            Origin::Owned => "inputs/owned/",
            Origin::DerivedLocal => "inputs/local/",
        };
        ensure!(
            entry.source.starts_with(prefix),
            "source must be inside {prefix}"
        );
        ensure!(
            names.insert(entry.path.to_ascii_lowercase()),
            "duplicate/case-colliding asset path"
        );
        paths.insert(entry.path.as_str());
    }
    for entry in files {
        let mut parent = Path::new(&entry.path).parent();
        while let Some(p) = parent {
            ensure!(
                !names.contains(&p.to_string_lossy().to_ascii_lowercase()),
                "file/directory path collision"
            );
            parent = p.parent();
        }
        let mut seen = BTreeSet::new();
        for dep in &entry.depends_on {
            ensure!(
                paths.contains(dep.as_str()),
                "missing/case-mismatched dependency: {dep}"
            );
            ensure!(seen.insert(dep), "duplicate dependency");
        }
    }
    // Iterative topological pass; untrusted depth cannot overflow the stack.
    let mut done = BTreeSet::new();
    loop {
        let previous = done.len();
        for entry in files {
            if entry.depends_on.iter().all(|d| done.contains(d.as_str())) {
                done.insert(entry.path.as_str());
            }
        }
        if done.len() == files.len() {
            return Ok(());
        }
        ensure!(done.len() != previous, "cyclic asset dependencies");
    }
}

fn data_only(path: &str, bytes: &[u8]) -> Result<()> {
    ensure!(
        ![
            b"MZ".as_slice(),
            b"\x7fELF",
            b"PK\x03\x04",
            b"PK\x05\x06",
            b"PK\x07\x08"
        ]
        .iter()
        .any(|sig| bytes.starts_with(sig)),
        "executable/archive signature disguised as asset: {path}"
    );
    if matches!(
        super::extension(Path::new(path)).as_str(),
        "def" | "shd" | "mat" | "txt"
    ) {
        let text = std::str::from_utf8(bytes).context("text asset must be UTF-8")?;
        ensure!(
            !text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t')),
            "control character in text asset"
        );
    }
    Ok(())
}

fn archive(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for (name, bytes) in files {
        zip.start_file(name, options)?;
        zip.write_all(bytes)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn validate(json: &[u8], payload: &[u8], distributable: bool) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(json).context("invalid overlay manifest")?;
    ensure!(
        manifest.schema == 1 && manifest.kind == KIND,
        "unsupported overlay manifest"
    );
    label(&manifest.id)?;
    label(&manifest.version)?;
    graph(
        &manifest
            .files
            .iter()
            .map(|f| f.input.clone())
            .collect::<Vec<_>>(),
    )?;
    ensure!(
        manifest.distributable
            == manifest
                .files
                .iter()
                .all(|e| e.input.origin == Origin::Owned),
        "inconsistent origin/distribution declaration"
    );
    ensure!(
        !distributable || manifest.distributable,
        "derived_local assets cannot enter a distributable package"
    );
    ensure!(
        valid_hash(&manifest.payload_sha256) && hash(payload) == manifest.payload_sha256,
        "overlay hash mismatch"
    );
    ensure!(
        serde_json::to_vec_pretty(&manifest)? == json,
        "manifest is not canonical"
    );
    let mut zip = ZipArchive::new(Cursor::new(payload))?;
    ensure!(
        zip.len() == manifest.files.len(),
        "archive/manifest entry count mismatch"
    );
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    let mut last = None;
    for (index, entry) in manifest.files.iter().enumerate() {
        let name = &entry.input.path;
        ensure!(
            last.is_none_or(|previous: &String| previous < name),
            "manifest entries must be sorted"
        );
        last = Some(name);
        ensure!(
            entry.bytes <= MAX_FILE && valid_hash(&entry.sha256),
            "invalid asset size/hash"
        );
        total = total.checked_add(entry.bytes).context("size overflow")?;
        ensure!(total <= MAX_TOTAL, "overlay exceeds 32 MiB");
        let mut file = zip.by_index(index)?;
        ensure!(
            file.name()?.as_ref() == name && file.size() == entry.bytes as u64,
            "archive path/size mismatch"
        );
        ensure!(
            file.is_file()
                && file.compression() == CompressionMethod::Stored
                && file.unix_mode().is_none_or(|m| m & 0o170000 == 0o100000),
            "nonregular/compressed overlay entry"
        );
        let mut bytes = Vec::new();
        (&mut file)
            .take(entry.bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() == entry.bytes && hash(&bytes) == entry.sha256,
            "asset hash mismatch: {name}"
        );
        data_only(name, &bytes)?;
        files.insert(name.clone(), bytes);
    }
    // Reject appended data, comments, extra fields and hidden/unlisted entries.
    // Canonical Stored output is independent of source mtimes and host paths.
    ensure!(archive(&files)? == payload, "noncanonical overlay archive");
    Ok(manifest)
}

pub fn verify(package: &Path, distributable: bool) -> Result<Manifest> {
    let (json, payload) = contents(package)?;
    validate(&json, &payload, distributable)
}

/// Generate only our own tiny PNG/text example, without commercial inputs.
pub fn example(output: &Path) -> Result<()> {
    let root = fresh_dir(output)?;
    fs::create_dir(root.join("inputs"))?;
    fs::create_dir(root.join("inputs/owned"))?;
    let image = image::RgbaImage::from_fn(2, 2, |x, y| {
        if x == y {
            image::Rgba([255, 128, 0, 255])
        } else {
            image::Rgba([0, 128, 255, 64])
        }
    });
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image).write_to(&mut encoded, image::ImageFormat::Png)?;
    write_new(&root.join("inputs/owned/checker.png"), encoded.get_ref())?;
    write_new(
        &root.join("inputs/owned/notes.txt"),
        b"Own pipeline fixture. Not a game translation or material.\n",
    )?;
    let project = Project {
        schema: 1,
        id: "pipeline-example".into(),
        version: "0.1.0".into(),
        files: vec![
            Input {
                source: "inputs/owned/checker.png".into(),
                path: "textures/mp/checker.png".into(),
                origin: Origin::Owned,
                depends_on: vec![],
            },
            Input {
                source: "inputs/owned/notes.txt".into(),
                path: "locale/mp/notes.txt".into(),
                origin: Origin::Owned,
                depends_on: vec!["textures/mp/checker.png".into()],
            },
        ],
    };
    write_new(
        &root.join("project.json"),
        &serde_json::to_vec_pretty(&project)?,
    )?;
    Ok(())
}

/// Build new immutable output, or reuse only an exact verified match. Changed
/// input requires a new output directory; existing packages are never replaced.
pub fn build(project: &Path, output: &Path, distributable: bool) -> Result<BuildReport> {
    let project_path = checked(project)?;
    let project: Project = serde_json::from_slice(&read(&project_path, MAX_JSON)?)?;
    ensure!(project.schema == 1, "unsupported project schema");
    label(&project.id)?;
    label(&project.version)?;
    graph(&project.files)?;
    let owned = project.files.iter().all(|e| e.origin == Origin::Owned);
    ensure!(
        !distributable || owned,
        "derived_local assets cannot enter a distributable package"
    );
    let root = project_path
        .parent()
        .context("project parent missing")?
        .canonicalize()?;
    let output_path = checked(output)?;
    let output_parent = output_path
        .parent()
        .context("output parent missing")?
        .canonicalize()?;
    ensure!(
        !output_parent.starts_with(root.join("inputs").canonicalize()?),
        "output must be outside inputs"
    );
    let mut entries = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for mut input in project.files {
        let bytes = read(&root.join(&input.source), MAX_FILE)?;
        total += bytes.len();
        ensure!(total <= MAX_TOTAL, "overlay exceeds 32 MiB");
        data_only(&input.path, &bytes)?;
        input.depends_on.sort();
        let entry = Entry {
            input: input.clone(),
            bytes: bytes.len(),
            sha256: hash(&bytes),
        };
        entries.insert(input.path.clone(), entry);
        files.insert(input.path, bytes);
    }
    let payload = archive(&files)?;
    ensure!(payload.len() <= MAX_ARCHIVE, "archive exceeds limit");
    let manifest = Manifest {
        schema: 1,
        kind: KIND.into(),
        id: project.id,
        version: project.version,
        distributable: owned,
        payload_sha256: hash(&payload),
        files: entries.into_values().collect(),
    };
    let json = serde_json::to_vec_pretty(&manifest)?;
    ensure!(json.len() <= MAX_JSON, "manifest exceeds limit");
    validate(&json, &payload, distributable)?;
    let reused = match fs::symlink_metadata(&output_path) {
        Ok(_) => {
            let (old_json, old_payload) = contents(&output_path)?;
            validate(&old_json, &old_payload, distributable)?;
            ensure!(
                old_json == json && old_payload == payload,
                "output differs: choose a NEW output directory"
            );
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            store(&output_path, &json, &payload)?;
            false
        }
        Err(e) => return Err(e.into()),
    };
    Ok(BuildReport { reused, manifest })
}

fn store(output: &Path, json: &[u8], payload: &[u8]) -> Result<()> {
    let output = fresh_dir(output)?;
    // A failed write leaves a partial directory for inspection. Never reuse it
    // as a valid package or delete a tree that another process may have changed.
    write_new(&output.join(PAYLOAD), payload)?;
    write_new(&output.join(MANIFEST), json)?;
    Ok(())
}

/// Prepare an independent package directory. No init.cfg/Steam/save changes.
pub fn stage(package: &Path, destination: &Path, distributable: bool) -> Result<Manifest> {
    let (json, payload) = contents(package)?;
    let manifest = validate(&json, &payload, distributable)?;
    let source = checked(package)?.canonicalize()?;
    let dest = checked(destination)?;
    ensure!(
        !dest
            .parent()
            .context("destination parent missing")?
            .canonicalize()?
            .starts_with(source),
        "destination cannot be inside package"
    );
    store(&dest, &json, &payload)?;
    verify(&dest, distributable)?;
    Ok(manifest)
}

/// Backup first, then remove only the two unchanged files. Not a transaction
/// across power loss/concurrent writers; a verified backup survives partial exit.
pub fn unstage(package: &Path, backup: &Path) -> Result<Manifest> {
    let (json, payload) = contents(package)?;
    let manifest = validate(&json, &payload, false)?;
    stage(package, backup, false)?;
    let path = checked(package)?;
    ensure!(
        contents(&path)? == (json.clone(), payload.clone()),
        "package changed during backup; no removal performed"
    );
    // Check each exact file again immediately before removing it.
    ensure!(
        read(&path.join(PAYLOAD), MAX_ARCHIVE)? == payload,
        "overlay changed; backup preserved"
    );
    fs::remove_file(path.join(PAYLOAD)).context("removal incomplete; verified backup preserved")?;
    ensure!(
        read(&path.join(MANIFEST), MAX_JSON)? == json,
        "manifest changed; backup preserved"
    );
    fs::remove_file(path.join(MANIFEST))
        .context("removal incomplete; verified backup preserved")?;
    fs::remove_dir(path).context("directory retained; verified backup preserved")?;
    Ok(manifest)
}

#[cfg(test)]
mod tests;

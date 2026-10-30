//! Allowlisted source inventories and deterministic, source-only handoff archives.
//! This catches accidental game/private-file inclusion; it does not establish
//! authorship, scan Git history, or authenticate a manifest supplied by others.
use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_FILE: usize = 2 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;
const MAX_LIST: usize = 8 * 1024 * 1024;
const MAX_FILES: usize = 10_000;
const MANIFEST: &str = "SOURCE_MANIFEST.json";
const KIND: &str = "18wos-aa-own-sources";

#[derive(Clone, Copy, Debug, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Tracked files plus nonignored untracked files, read from disk.
    Worktree,
    /// Exact staged blobs, including staged changes absent from disk.
    Index,
    /// Exact committed HEAD tree, independent of later worktree/index edits.
    Head,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Audit {
    pub schema: u8,
    pub scope: Scope,
    pub commit: Option<String>,
    pub dirty: bool,
    pub total_bytes: usize,
    pub files: Vec<SourceFile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u8,
    pub kind: String,
    pub commit: String,
    pub files: Vec<SourceFile>,
}

struct Input {
    path: String,
    oid: Option<String>,
}

/// The default-deny list is intentionally independent of Git's ignore settings:
/// `git add -f` must not bypass the artifact gate.
fn allowed(path: &str) -> Result<()> {
    crate::safe_relative(path)?;
    ensure!(
        path.len() <= 240 && path.is_ascii(),
        "unsupported source path"
    );
    ensure!(
        path.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c)),
        "unsupported source path: {path}"
    );
    ensure!(
        !path.split('/').any(|part| matches!(
            part.to_ascii_lowercase().as_str(),
            "target" | "extracted" | "generated" | "backups" | ".git" | ".cargo"
        )),
        "generated/private source path: {path}"
    );
    let fixed = matches!(
        path,
        ".gitignore"
            | ".gitattributes"
            | "Cargo.toml"
            | "Cargo.lock"
            | "README.md"
            | "ROADMAP.MD"
            | "AGENTS.md"
            | "LICENSE"
            | "rust-toolchain.toml"
            | "docs/NETWORK_PROTOCOL.md"
            | "docs/GAME_VALIDATION.md"
            | "docs/UI_DESIGN.md"
            | "docs/LOCALIZATION.md"
            | "translations/pt-BR/local_def.json"
            | "asset_tools/Cargo.toml"
            | "multiplayer_server/Cargo.toml"
            | "client_dll/Cargo.toml"
            | "mod_ui/Cargo.toml"
    );
    let rust = path.ends_with(".rs")
        && [
            "asset_tools/src/",
            "multiplayer_server/src/",
            "client_dll/src/",
            "client_dll/tests/",
            "mod_ui/src/",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix));
    ensure!(
        fixed || rust,
        "path is outside the own-source allowlist: {path}"
    );
    Ok(())
}

fn text_only(path: &str, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_FILE, "source exceeds 2 MiB: {path}");
    ensure!(
        ![
            b"MZ".as_slice(),
            b"PK\x03\x04",
            b"DDS ",
            b"\x89PNG",
            b"\x7fELF"
        ]
        .iter()
        .any(|magic| bytes.starts_with(magic)),
        "binary signature in source: {path}"
    );
    let text =
        std::str::from_utf8(bytes).with_context(|| format!("source is not UTF-8: {path}"))?;
    ensure!(
        !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n')),
        "binary/control bytes in source: {path}"
    );
    Ok(())
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|c| c.is_ascii_hexdigit())
}

fn git(root: &Path, args: &[&str], max: usize) -> Result<Vec<u8>> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("starting Git for source audit")?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .context("Git stdout")?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > max {
        let _ = child.kill();
        let _ = child.wait();
        read?;
        bail!("Git output exceeded the source-audit limit");
    }
    ensure!(child.wait()?.success(), "Git source-audit operation failed");
    Ok(bytes)
}

fn regular_path(root: &Path, path: &str) -> Result<()> {
    let mut next = root.to_path_buf();
    let parts: Vec<_> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        next.push(part);
        let meta =
            fs::symlink_metadata(&next).with_context(|| format!("reading source: {path}"))?;
        ensure!(
            !meta.file_type().is_symlink(),
            "symlink in source path: {path}"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                meta.file_attributes() & 0x400 == 0,
                "reparse point in source path: {path}"
            );
        }
        ensure!(
            if i + 1 == parts.len() {
                meta.is_file()
            } else {
                meta.is_dir()
            },
            "source path is not a regular file/directory: {path}"
        );
    }
    Ok(())
}

type Contents = Vec<(String, Vec<u8>)>;

fn collect(root: &Path, scope: Scope) -> Result<(Audit, Contents)> {
    let root = fs::canonicalize(root).context("source root must exist")?;
    let git_root = git(&root, &["rev-parse", "--show-toplevel"], 4096)?;
    let git_root = std::str::from_utf8(&git_root)?.trim_end_matches(['\r', '\n']);
    ensure!(
        fs::canonicalize(git_root)? == root,
        "select the repository root, not a subdirectory"
    );
    let commit = git(&root, &["rev-parse", "--verify", "HEAD"], 128)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|v| v.trim().to_string());
    ensure!(
        commit.as_deref().is_none_or(oid),
        "invalid Git commit identifier"
    );
    let dirty = !git(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
        MAX_LIST,
    )?
    .is_empty();
    let mut inputs = Vec::new();
    let list = match scope {
        Scope::Head => git(
            &root,
            &[
                "ls-tree",
                "-r",
                "-z",
                "--full-tree",
                commit.as_deref().context("HEAD is not committed")?,
            ],
            MAX_LIST,
        )?,
        _ => git(&root, &["ls-files", "--stage", "-z"], MAX_LIST)?,
    };
    for entry in list.split(|c| *c == 0).filter(|v| !v.is_empty()) {
        let entry = std::str::from_utf8(entry).context("Git source names must be UTF-8")?;
        let (header, path) = entry.split_once('\t').context("invalid Git entry")?;
        let fields: Vec<_> = header.split(' ').collect();
        ensure!(fields.len() == 3, "invalid Git entry header");
        ensure!(
            matches!(fields[0], "100644" | "100755"),
            "symlink/submodule/nonregular Git entry: {path}"
        );
        let blob = if matches!(scope, Scope::Head) {
            ensure!(fields[1] == "blob", "Git entry is not a blob: {path}");
            fields[2]
        } else {
            ensure!(fields[2] == "0", "unmerged index entry: {path}");
            fields[1]
        };
        ensure!(oid(blob), "invalid Git blob identifier");
        inputs.push(Input {
            path: path.into(),
            oid: (!matches!(scope, Scope::Worktree)).then(|| blob.into()),
        });
    }
    if matches!(scope, Scope::Worktree) {
        let list = git(
            &root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            MAX_LIST,
        )?;
        for path in list.split(|c| *c == 0).filter(|p| !p.is_empty()) {
            inputs.push(Input {
                path: std::str::from_utf8(path)?.into(),
                oid: None,
            });
        }
    }
    ensure!(
        !inputs.is_empty() && inputs.len() <= MAX_FILES,
        "source inventory must have 1..10000 files"
    );
    inputs.sort_by(|a, b| a.path.cmp(&b.path));
    let mut seen = BTreeSet::new();
    // Validate the entire path inventory before reading any file contents.
    for input in &inputs {
        allowed(&input.path)?;
        ensure!(
            seen.insert(input.path.to_ascii_lowercase()),
            "duplicate/Windows case collision: {}",
            input.path
        );
    }
    let mut report = Audit {
        schema: 1,
        scope,
        commit,
        dirty,
        total_bytes: 0,
        files: Vec::new(),
    };
    let mut contents = Vec::new();
    for input in inputs {
        let data = if let Some(blob) = input.oid {
            git(&root, &["cat-file", "blob", &blob], MAX_FILE)?
        } else {
            regular_path(&root, &input.path)?;
            let mut bytes = Vec::new();
            File::open(root.join(&input.path))?
                .take(MAX_FILE as u64 + 1)
                .read_to_end(&mut bytes)?;
            bytes
        };
        text_only(&input.path, &data)?;
        report.total_bytes += data.len();
        ensure!(
            report.total_bytes <= MAX_TOTAL,
            "source inventory exceeds 64 MiB"
        );
        report.files.push(SourceFile {
            path: input.path.clone(),
            bytes: data.len(),
            sha256: hash(&data),
        });
        contents.push((input.path, data));
    }
    Ok((report, contents))
}

pub fn audit(root: &Path, scope: Scope) -> Result<Audit> {
    collect(root, scope).map(|(report, _)| report)
}

/// Only committed HEAD blobs are exported. Never recurse over a Steam folder,
/// follow .gitignore exceptions, run a build, or copy a local configuration.
pub fn bundle(root: &Path, output: &Path) -> Result<Manifest> {
    let (report, contents) = collect(root, Scope::Head)?;
    let manifest = Manifest {
        schema: 1,
        kind: KIND.into(),
        commit: report.commit.context("HEAD is not committed")?,
        files: report.files,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    ensure!(bytes.len() <= MAX_FILE, "source manifest exceeds limit");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .context("source bundle output must be a new file in an existing directory")?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    zip.start_file(MANIFEST, options)?;
    zip.write_all(&bytes)?;
    for (path, bytes) in contents {
        zip.start_file(path, options)?;
        zip.write_all(&bytes)?;
    }
    zip.finish()?.sync_all()?;
    Ok(manifest)
}

/// Read-only: no extraction and no execution. Hash agreement is integrity within
/// this archive, not a signature proving who produced it.
pub fn verify_bundle(input: &Path) -> Result<Manifest> {
    let file = File::open(input)?;
    ensure!(
        file.metadata()?.len() <= (MAX_TOTAL + 4 * MAX_FILE) as u64,
        "source archive exceeds limit"
    );
    let mut zip = ZipArchive::new(file).context("opening source bundle")?;
    ensure!(
        (2..=MAX_FILES + 1).contains(&zip.len()),
        "source archive entry count exceeds limit"
    );
    let mut names = BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = entry.name()?;
        ensure!(
            !entry.is_dir() && entry.size() <= MAX_FILE as u64,
            "invalid/oversized source entry"
        );
        ensure!(
            entry
                .unix_mode()
                .is_none_or(|m| matches!(m & 0o170000, 0 | 0o100000)),
            "nonregular source entry"
        );
        ensure!(
            names.insert(name.to_ascii_lowercase()),
            "duplicate/case-colliding source archive name"
        );
        if name != MANIFEST {
            allowed(&name)?;
        }
    }
    let bytes = read_entry(&mut zip, MANIFEST)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).context("invalid source manifest")?;
    ensure!(
        manifest.schema == 1 && manifest.kind == KIND && oid(&manifest.commit),
        "unsupported source manifest"
    );
    ensure!(
        manifest.files.len() + 1 == zip.len(),
        "source manifest count mismatch"
    );
    let mut seen = BTreeSet::new();
    let mut total = 0;
    for source in &manifest.files {
        allowed(&source.path)?;
        ensure!(
            seen.insert(source.path.to_ascii_lowercase()),
            "duplicate manifest path"
        );
        ensure!(
            source.bytes <= MAX_FILE && source.sha256.len() == 64,
            "invalid source manifest entry"
        );
        total += source.bytes;
        ensure!(total <= MAX_TOTAL, "source bundle exceeds 64 MiB");
        let bytes = read_entry(&mut zip, &source.path)?;
        text_only(&source.path, &bytes)?;
        ensure!(
            bytes.len() == source.bytes && hash(&bytes) == source.sha256,
            "source hash/size mismatch: {}",
            source.path
        );
    }
    Ok(manifest)
}

fn read_entry(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    zip.by_name(name)?
        .take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_FILE,
        "source archive entry exceeds limit"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests;

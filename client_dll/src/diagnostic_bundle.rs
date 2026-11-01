//! Share only re-sanitized status/stop reports, never a raw run or its backups.
use crate::{local_files, session_evidence};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
const SAMPLE_CAP: usize = 64 * 1024;
const ANALYSIS_CAP: usize = 8 * 1024 * 1024;
const MANIFEST: &str = "manifest.json";
const ANALYSIS: &str = "analysis.json";
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
    pub validation_claimed: bool,
    pub files: Vec<Entry>,
}
fn name(index: usize) -> String {
    format!("report-{:04}.json", index + 1)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn key(value: &str) -> Option<(u128, u8)> {
    let (tick, role) = value.strip_suffix(".json")?.split_once('-')?;
    if tick.is_empty() || tick.len() > 39 || !tick.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((
        tick.parse().ok()?,
        match role {
            "start" => 0,
            "status" => 1,
            "stop" => 2,
            _ => return None,
        },
    ))
}
pub fn create(source: &Path, output: &Path) -> Result<Manifest> {
    let input = source.join("diagnosticos");
    ensure!(
        local_files::metadata(&input)?.is_dir(),
        "diagnostics directory missing"
    );
    let mut chosen = Vec::new();
    for (i, row) in fs::read_dir(&input)?.enumerate() {
        ensure!(i < 1024, "too many directory entries");
        let row = row?;
        if let Some(order) = row.file_name().to_str().and_then(key) {
            chosen.push((order, row.path()));
            ensure!(
                chosen.len() <= session_evidence::MAX_INPUTS,
                "too many diagnostic reports"
            );
        }
    }
    ensure!(!chosen.is_empty(), "no status/stop diagnostics available");
    chosen.sort_by_key(|row| row.0);
    from_reports(
        &chosen.into_iter().map(|row| row.1).collect::<Vec<_>>(),
        output,
    )
}
/// Explicit input order from ONE peer, including historical raw status/stop.
/// Each report is filtered again; original paths/names never enter the bundle.
pub fn from_reports(paths: &[PathBuf], output: &Path) -> Result<Manifest> {
    ensure!(
        !paths.is_empty() && paths.len() <= session_evidence::MAX_INPUTS,
        "invalid diagnostic report count"
    );
    let mut reports = Vec::new();
    let mut captures = Vec::new();
    for path in paths {
        let bytes = session_evidence::sanitize(&local_files::read(path, SAMPLE_CAP)?)?;
        ensure!(bytes.len() <= SAMPLE_CAP, "sanitized report exceeds limit");
        captures.push(session_evidence::parse(&bytes)?);
        reports.push(bytes);
    }
    let analysis = serde_json::to_vec_pretty(&session_evidence::analyze(captures)?)?;
    ensure!(analysis.len() <= ANALYSIS_CAP, "analysis exceeds limit");
    // Validate every input before creating output; never overwrite any folder.
    fs::create_dir(output)?;
    write(
        &output.join(".INCOMPLETE"),
        b"Export not finalized. Do not share this directory yet.\n",
    )?;
    let mut files = Vec::new();
    for (i, bytes) in reports.iter().enumerate() {
        let file = name(i);
        write(&output.join(&file), bytes)?;
        files.push(Entry {
            file,
            bytes: bytes.len(),
            sha256: digest(bytes),
        });
    }
    write(&output.join(ANALYSIS), &analysis)?;
    files.push(Entry {
        file: ANALYSIS.into(),
        bytes: analysis.len(),
        sha256: digest(&analysis),
    });
    let manifest = Manifest {
        schema: 1,
        software_version: env!("CARGO_PKG_VERSION").into(),
        validation_claimed: false,
        files,
    };
    write(
        &output.join(MANIFEST),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    fs::remove_file(output.join(".INCOMPLETE"))?;
    verify(output)
}
/// Independently check membership/hashes, canonical filtering and regeneration
/// of analysis. A valid bundle is still not human/gameplay validation.
pub fn verify(root: &Path) -> Result<Manifest> {
    ensure!(
        local_files::metadata(root)?.is_dir(),
        "bundle must be a directory"
    );
    let manifest: Manifest =
        serde_json::from_slice(&local_files::read(&root.join(MANIFEST), SAMPLE_CAP)?)?;
    ensure!(
        manifest.schema == 1
            && crate::runtime_info::valid_version(&manifest.software_version)
            && !manifest.validation_claimed
            && (2..=session_evidence::MAX_INPUTS + 1).contains(&manifest.files.len()),
        "invalid diagnostic manifest"
    );
    let count = manifest.files.len() - 1;
    let mut allowed = BTreeSet::from([MANIFEST.to_owned()]);
    let mut captures = Vec::new();
    let mut expected_analysis = None;
    for (i, entry) in manifest.files.iter().enumerate() {
        let expected = if i == count {
            ANALYSIS.to_owned()
        } else {
            name(i)
        };
        ensure!(
            entry.file == expected && allowed.insert(expected.clone()),
            "invalid/duplicate diagnostic member"
        );
        let cap = if i == count { ANALYSIS_CAP } else { SAMPLE_CAP };
        ensure!(
            entry.bytes <= cap && entry.sha256.len() == 64,
            "invalid diagnostic entry size/hash"
        );
        let bytes = local_files::read(&root.join(expected), cap)?;
        ensure!(
            bytes.len() == entry.bytes && digest(&bytes) == entry.sha256,
            "diagnostic member changed"
        );
        if i == count {
            expected_analysis = Some(bytes);
        } else {
            ensure!(
                session_evidence::sanitize(&bytes)? == bytes,
                "diagnostic report contains noncanonical/private fields"
            );
            captures.push(session_evidence::parse(&bytes)?);
        }
    }
    for (i, row) in fs::read_dir(root)?.enumerate() {
        ensure!(i < allowed.len(), "unexpected diagnostic member");
        let row = row?;
        ensure!(
            row.file_name()
                .to_str()
                .is_some_and(|n| allowed.contains(n)),
            "unexpected diagnostic member"
        );
    }
    ensure!(
        expected_analysis.as_deref()
            == Some(serde_json::to_vec_pretty(&session_evidence::analyze(captures)?)?.as_slice()),
        "diagnostic analysis differs"
    );
    Ok(manifest)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn status(sent: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"client":{"status":1,"sample_state":1,"sent":sent,"received":0,"diagnostic":"PRIVATE"},"draw":{"draw_calls":0,"flush_calls":0,"foreign_calls":0,"flags":0},"visual":{"item":0,"shown":0,"drawn":0,"fault":0},"presentation":crate::presentation::status(),"remotes":[],"token":"PRIVATE","path":"PRIVATE"})).unwrap()
    }
    #[test]
    fn export_filters_unknown_fields_ignores_raw_files_and_preserves_inputs() {
        let t = tempfile::tempdir().unwrap();
        let input = t.path().join("diagnosticos");
        fs::create_dir(&input).unwrap();
        fs::write(input.join("10-status.json"), status(20)).unwrap();
        fs::write(input.join("2-start.json"), status(10)).unwrap();
        fs::write(input.join("identity.json"), b"PRIVATE").unwrap();
        fs::write(t.path().join("own-save.sav"), b"PRIVATE").unwrap();
        let out = t.path().join("share");
        let manifest = create(t.path(), &out).unwrap();
        assert_eq!(manifest.files.len(), 3);
        verify(&out).unwrap();
        for e in &manifest.files {
            assert!(
                !String::from_utf8(local_files::read(&out.join(&e.file), ANALYSIS_CAP).unwrap())
                    .unwrap()
                    .contains("PRIVATE")
            );
        }
        let first = local_files::read(&out.join(name(0)), SAMPLE_CAP).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&first).unwrap()["client"]["sent"],
            10
        );
        assert!(
            fs::read(input.join("2-start.json"))
                .unwrap()
                .windows(7)
                .any(|b| b == b"PRIVATE")
        );
        assert!(create(t.path(), &out).is_err());
        fs::write(out.join("backup.sav"), b"PRIVATE").unwrap();
        assert!(verify(&out).is_err());
    }
    #[test]
    fn corrupt_reports_and_oversized_files_fail_before_creating_output() {
        let t = tempfile::tempdir().unwrap();
        let input = t.path().join("diagnosticos");
        fs::create_dir(&input).unwrap();
        let out = t.path().join("share");
        fs::write(input.join("1-status.json"), b"{broken").unwrap();
        assert!(create(t.path(), &out).is_err());
        assert!(!out.exists());
        fs::write(input.join("1-status.json"), vec![b' '; SAMPLE_CAP + 1]).unwrap();
        assert!(create(t.path(), &out).is_err());
        assert!(!out.exists());
        fs::write(input.join("1-status.json"), status(1)).unwrap();
        create(t.path(), &out).unwrap();
        fs::write(out.join(name(0)), status(2)).unwrap();
        assert!(verify(&out).is_err());
    }
}

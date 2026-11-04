//! Refuses a release artifact holding byte-identical game files. Indexes every
//! entry of the local originals by SHA-256 and looks for those bytes in the
//! artifact, including inside ZIP/SCS members and decoded 3nK envelopes.
//! Exact copies only: an edited or re-encoded original is not detected, and
//! `derived_local` content is rejected by `mod-build --distributable`, not here.
//! The report names artifact paths and hashes, never original content.
use crate::localization::codec;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Very short files repeat by chance (one-line texture objects, empty defs).
pub const MIN_BYTES: u64 = 64;
const MAX_ENTRY: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_FILES: usize = 20_000;
const MAX_TOTAL: u64 = 16 * 1024 * 1024 * 1024;

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}
fn read_bounded(mut reader: impl Read, size: u64, budget: &mut u64) -> Result<Vec<u8>> {
    ensure!(size <= MAX_ENTRY, "member exceeds audit limit");
    *budget = budget
        .checked_sub(size)
        .context("audit read budget exhausted")?;
    let mut bytes = Vec::with_capacity(size as usize);
    reader
        .by_ref()
        .take(MAX_ENTRY + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 == size, "member size mismatch");
    Ok(bytes)
}

#[derive(Default)]
pub struct Index {
    hashes: BTreeSet<[u8; 32]>,
    pub entries: usize,
}
impl Index {
    /// Whole archives and all their members, raw and with the 3nK envelope removed.
    pub fn build(originals: &[PathBuf]) -> Result<Self> {
        ensure!(
            (1..=64).contains(&originals.len()),
            "supply 1..=64 original files"
        );
        let mut index = Self::default();
        let mut budget = MAX_TOTAL;
        for path in originals {
            let size = std::fs::metadata(path)?.len();
            let bytes = read_bounded(File::open(path)?, size, &mut budget)?;
            index.add(&bytes);
            if !is_zip(&bytes) {
                continue;
            }
            let mut zip = zip::ZipArchive::new(Cursor::new(&bytes))
                .with_context(|| format!("{} is not a readable archive", path.display()))?;
            ensure!(zip.len() <= MAX_ENTRIES, "archive entry limit");
            for i in 0..zip.len() {
                let entry = zip.by_index(i)?;
                if !entry.is_file() {
                    continue;
                }
                let size = entry.size();
                let member = read_bounded(entry, size, &mut budget)?;
                index.add(&member);
                if let Ok(decoded) = codec::decode(&member) {
                    index.add(&decoded);
                }
            }
        }
        Ok(index)
    }
    fn add(&mut self, bytes: &[u8]) {
        if bytes.len() as u64 >= MIN_BYTES && self.hashes.insert(digest(bytes)) {
            self.entries += 1;
        }
    }
    fn contains(&self, bytes: &[u8]) -> bool {
        bytes.len() as u64 >= MIN_BYTES && self.hashes.contains(&digest(bytes))
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Finding {
    pub path: String,
    /// Member inside an archive, when the copy is not the file itself.
    pub member: Option<String>,
    pub sha256: String,
}
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub schema: u32,
    pub original_entries: usize,
    pub files: usize,
    pub members: usize,
    pub findings: Vec<Finding>,
    /// Links and special files are not followed, so their targets stay unaudited.
    pub not_regular: Vec<String>,
    pub clean: bool,
    pub exact_copies_only: bool,
}
fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn audit(root: &Path, index: &Index) -> Result<Report> {
    let mut report = Report {
        schema: 1,
        original_entries: index.entries,
        exact_copies_only: true,
        ..Report::default()
    };
    let mut budget = MAX_TOTAL;
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type().is_dir() {
            continue;
        }
        if !entry.file_type().is_file() {
            report.not_regular.push(relative);
            continue;
        }
        report.files += 1;
        ensure!(report.files <= MAX_FILES, "artifact file limit");
        let size = entry.metadata()?.len();
        let bytes = read_bounded(File::open(entry.path())?, size, &mut budget)?;
        if index.contains(&bytes) {
            report.findings.push(Finding {
                path: relative.clone(),
                member: None,
                sha256: hex(&bytes),
            });
        }
        if !is_zip(&bytes) {
            continue;
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(&bytes))
            .with_context(|| format!("{relative} looks like an archive but cannot be read"))?;
        ensure!(zip.len() <= MAX_ENTRIES, "archive entry limit");
        for i in 0..zip.len() {
            let member = zip.by_index(i)?;
            if !member.is_file() {
                continue;
            }
            report.members += 1;
            let name = member.name()?.into_owned();
            let size = member.size();
            let content = read_bounded(member, size, &mut budget)?;
            if index.contains(&content) {
                report.findings.push(Finding {
                    path: relative.clone(),
                    member: Some(name),
                    sha256: hex(&content),
                });
            }
        }
    }
    report.clean = report.findings.is_empty() && report.not_regular.is_empty();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};
    const OWN_ORIGINAL: &[u8] =
        b"own original fixture long enough to count as a real game resource here";
    fn zip_with(path: &Path, files: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, bytes) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(bytes).unwrap();
        }
        z.finish().unwrap();
    }
    fn envelope(plain: &[u8], seed: u8) -> Vec<u8> {
        let mut out = vec![b'3', b'n', b'K', 1, 0, seed];
        for (i, &b) in plain.iter().enumerate() {
            let n = (usize::from(seed) + i) & 255;
            out.push(b ^ ((((n << 2) ^ !n) << 3) ^ n) as u8);
        }
        out
    }
    fn index(t: &Path) -> Index {
        let archive = t.join("own.scs");
        zip_with(
            &archive,
            &[
                ("defs/own.def", OWN_ORIGINAL),
                ("scripts/secret.def", &envelope(&OWN_ORIGINAL.repeat(2), 7)),
                ("tiny.tobj", b"map 2d own.tga"),
            ],
        );
        Index::build(&[archive]).unwrap()
    }
    #[test]
    fn renamed_nested_and_decoded_copies_are_found_but_own_files_pass() {
        let t = tempfile::tempdir().unwrap();
        let index = index(t.path());
        let kit = t.path().join("kit");
        fs::create_dir_all(kit.join("sub")).unwrap();
        fs::write(kit.join("sub/renamed.txt"), OWN_ORIGINAL).unwrap();
        fs::write(kit.join("decoded.def"), OWN_ORIGINAL.repeat(2)).unwrap();
        fs::write(
            kit.join("mine.txt"),
            b"own text written for this kit, not from the game at all",
        )
        .unwrap();
        fs::write(kit.join("tiny.tobj"), b"map 2d own.tga").unwrap();
        zip_with(
            &kit.join("pack.scs"),
            &[("x/y.def", OWN_ORIGINAL), ("mine", b"own")],
        );
        let r = audit(&kit, &index).unwrap();
        assert!(!r.clean);
        assert_eq!((r.files, r.members), (5, 2));
        let found: Vec<_> = r
            .findings
            .iter()
            .map(|f| (f.path.as_str(), f.member.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                ("decoded.def", None),
                ("pack.scs", Some("x/y.def")),
                ("sub/renamed.txt", None)
            ]
        );
    }
    #[test]
    fn a_clean_artifact_passes_and_the_whole_archive_counts_as_a_copy() {
        let t = tempfile::tempdir().unwrap();
        let index = index(t.path());
        let kit = t.path().join("kit");
        fs::create_dir(&kit).unwrap();
        fs::write(
            kit.join("mine.txt"),
            b"own text written for this kit, not from the game at all",
        )
        .unwrap();
        let r = audit(&kit, &index).unwrap();
        assert!(r.clean && r.exact_copies_only, "{r:?}");
        fs::copy(t.path().join("own.scs"), kit.join("copy.bin")).unwrap();
        let r = audit(&kit, &index).unwrap();
        assert!(!r.clean);
        assert!(
            r.findings
                .iter()
                .any(|f| f.path == "copy.bin" && f.member.is_none())
        );
        assert!(serde_json::to_string(&r).unwrap().len() < 4096);
    }
    #[test]
    fn unreadable_inputs_fail_instead_of_passing() {
        let t = tempfile::tempdir().unwrap();
        let index = index(t.path());
        assert!(Index::build(&[]).is_err());
        assert!(Index::build(&[t.path().join("missing.scs")]).is_err());
        let kit = t.path().join("kit");
        fs::create_dir(&kit).unwrap();
        fs::write(kit.join("broken.scs"), b"PK\x03\x04 not really an archive").unwrap();
        assert!(audit(&kit, &index).is_err());
        assert!(audit(&t.path().join("absent"), &index).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn links_are_reported_not_followed() {
        let t = tempfile::tempdir().unwrap();
        let index = index(t.path());
        let kit = t.path().join("kit");
        fs::create_dir(&kit).unwrap();
        std::os::unix::fs::symlink(t.path().join("own.scs"), kit.join("link")).unwrap();
        let r = audit(&kit, &index).unwrap();
        assert!(!r.clean && r.findings.is_empty());
        assert_eq!(r.not_regular, ["link"]);
    }
}

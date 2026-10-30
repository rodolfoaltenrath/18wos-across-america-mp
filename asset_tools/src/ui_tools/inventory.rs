//! Metadata/key inventory only. Legacy font bytes and original prose stay local.
use super::hash;
use anyhow::{Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::PathBuf,
};
#[derive(Serialize)]
pub struct Resource {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub utf8: bool,
    pub codepage_hint: Option<u32>,
    pub keys: Vec<String>,
}
#[derive(Serialize)]
pub struct Archive {
    pub sha256: String,
    pub entries: usize,
    pub resources: Vec<Resource>,
    pub malformed_markers: usize,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: u32,
    pub archives: Vec<Archive>,
    pub keys: BTreeMap<String, usize>,
    pub translation_complete: bool,
    pub native_font_coverage_verified: bool,
    pub binaries_and_images_audited: bool,
}
fn keys(bytes: &[u8]) -> (Vec<String>, usize) {
    let mut at = 0;
    let mut found = BTreeSet::new();
    let mut malformed = 0;
    while at + 1 < bytes.len() {
        if &bytes[at..at + 2] != b"@@" {
            at += 1;
            continue;
        }
        let begin = at + 2;
        let end = (begin..bytes.len().min(begin + 130))
            .find(|&p| p + 1 < bytes.len() && &bytes[p..p + 2] == b"@@");
        if let Some(end) = end {
            let token = &bytes[begin..end];
            if !token.is_empty()
                && token.len() <= 128
                && token
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(b))
            {
                found.insert(String::from_utf8_lossy(token).into_owned());
            } else {
                malformed += 1;
            }
            at = end + 2;
        } else {
            malformed += 1;
            at = begin;
        }
    }
    (found.into_iter().collect(), malformed)
}
pub fn scan(paths: &[PathBuf]) -> Result<Report> {
    ensure!(
        !paths.is_empty() && paths.len() <= 16,
        "supply 1..=16 archives"
    );
    let mut report = Report {
        schema: 1,
        archives: vec![],
        keys: BTreeMap::new(),
        translation_complete: false,
        native_font_coverage_verified: false,
        binaries_and_images_audited: false,
    };
    let mut total = 0usize;
    let mut resource_count = 0usize;
    for path in paths {
        let mut f = File::open(path)?;
        let before = f.metadata()?;
        ensure!(
            before.is_file() && before.len() <= 4 * 1024 * 1024 * 1024u64,
            "archive bounds"
        );
        let mut h = Sha256::new();
        let mut buf = [0; 65536];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        let digest = format!("{:x}", h.finalize());
        let mut zip = zip::ZipArchive::new(File::open(path)?)?;
        ensure!(zip.len() <= 100_000, "too many entries");
        let mut result = Archive {
            sha256: digest,
            entries: zip.len(),
            resources: vec![],
            malformed_markers: 0,
        };
        let mut names = BTreeSet::new();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            let name = file.name()?.into_owned();
            if file.is_dir() {
                continue;
            }
            let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            if !matches!(
                ext.as_str(),
                "esc" | "def" | "font" | "ini" | "cfg" | "tpl" | "shd"
            ) {
                continue;
            }
            crate::safe_relative(&name)?;
            resource_count += 1;
            ensure!(
                resource_count <= 8192 && name.len() <= 240,
                "inventory resource/path limit"
            );
            ensure!(
                names.insert(name.to_ascii_lowercase()),
                "duplicate/case-colliding text resource"
            );
            ensure!(file.size() <= 1024 * 1024, "text resource exceeds 1 MiB");
            let size = file.size();
            let mut bytes = Vec::new();
            file.by_ref()
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() <= 1024 * 1024 && bytes.len() as u64 == size,
                "resource size mismatch"
            );
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| anyhow::anyhow!("inventory size overflow"))?;
            ensure!(total <= 32 * 1024 * 1024, "text inventory exceeds 32 MiB");
            let (tokens, bad) = keys(&bytes);
            result.malformed_markers += bad;
            for token in &tokens {
                *report.keys.entry(token.clone()).or_default() += 1;
            }
            ensure!(report.keys.len() <= 16384, "too many localization keys");
            let utf = std::str::from_utf8(&bytes);
            let codepage = utf.as_ref().ok().and_then(|s| {
                s.lines().find_map(|line| {
                    line.trim()
                        .strip_prefix("codepage:")
                        .and_then(|v| v.split('#').next())
                        .and_then(|v| v.trim().parse::<u32>().ok())
                })
            });
            result.resources.push(Resource {
                path: name,
                bytes: size,
                sha256: hash(&bytes),
                utf8: utf.is_ok(),
                codepage_hint: codepage,
                keys: tokens,
            });
        }
        let after = std::fs::metadata(path)?;
        ensure!(
            before.len() == after.len() && before.modified()? == after.modified()?,
            "archive changed during inventory"
        );
        result.resources.sort_by(|a, b| a.path.cmp(&b.path));
        report.archives.push(result);
    }
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn own_archive_keys_and_legacy_encoding_are_inventoried_without_extracting_prose() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("own.scs");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        for (name, data) in [
            (
                "ui/own.esc",
                b"@@ui_one@@ @@ui_one@@ @@ui_two@@ @@bad key@@".as_slice(),
            ),
            ("fonts/own.font", &[0xff, 0xfe]),
            ("fonts/own.ini", b"codepage:1250 # fixture"),
        ] {
            z.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        let r = scan(&[p]).unwrap();
        assert_eq!(r.keys.len(), 2);
        assert_eq!(r.keys["ui_one"], 1);
        assert_eq!(r.archives[0].malformed_markers, 1);
        assert!(
            !r.archives[0]
                .resources
                .iter()
                .find(|r| r.path.ends_with(".font"))
                .unwrap()
                .utf8
        );
        assert!(!r.translation_complete);
        assert_eq!(
            r.archives[0]
                .resources
                .iter()
                .find(|r| r.path.ends_with(".ini"))
                .unwrap()
                .codepage_hint,
            Some(1250)
        );
    }
}

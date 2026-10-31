//! Finite, sanitized session captures. Recording is evidence collection, not validation.
use crate::session_evidence::{self, Capture};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_SAMPLE_BYTES: usize = 64 * 1024;
const MAX_ANALYSIS_BYTES: usize = 16 * 1024 * 1024;
const MARKER: &str = ".INCOMPLETE";
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub seconds: u64,
    pub interval_ms: u64,
}
impl Plan {
    pub fn validate(self) -> Result<()> {
        ensure!(
            (1..=1800).contains(&self.seconds),
            "recording duration must be 1..=1800 seconds"
        );
        ensure!(
            (1000..=60000).contains(&self.interval_ms),
            "recording interval must be 1000..=60000 ms"
        );
        ensure!(
            (self.seconds * 1000).div_ceil(self.interval_ms) < session_evidence::MAX_INPUTS as u64,
            "recording would exceed 256 samples; increase the interval"
        );
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum End {
    DurationReached,
    Interrupted,
    CollectionFailed,
    TargetChanged,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub file: String,
    pub elapsed_ms: u64,
    pub sha256: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub schema: u8,
    pub software_version: String,
    pub plan: Plan,
    pub end: End,
    pub elapsed_ms: u64,
    pub requested_window_complete: bool,
    pub validation_claimed: bool,
    pub samples: Vec<Sample>,
    pub analysis_sha256: Option<String>,
}
pub struct Recorder {
    directory: PathBuf,
    plan: Plan,
    samples: Vec<Sample>,
    captures: Vec<Capture>,
    write_failed: bool,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn sample_name(index: usize) -> String {
    format!("snapshot-{:04}.json", index + 1)
}
impl Recorder {
    pub fn create(directory: &Path, plan: Plan) -> Result<Self> {
        plan.validate()?;
        fs::create_dir(directory)
            .context("recording requires a NEW directory and an existing parent")?;
        write_new(
            &directory.join(MARKER),
            b"Collection has not been finalized. Do not infer completion.\n",
        )?;
        Ok(Self {
            directory: directory.into(),
            plan,
            samples: Vec::new(),
            captures: Vec::new(),
            write_failed: false,
        })
    }
    pub fn push(&mut self, elapsed_ms: u64, raw: &[u8]) -> Result<()> {
        ensure!(
            !self.write_failed,
            "previous write failed; keep the incomplete recording"
        );
        ensure!(
            self.samples.len() < session_evidence::MAX_INPUTS,
            "too many samples"
        );
        ensure!(
            self.samples
                .last()
                .is_none_or(|s| elapsed_ms > s.elapsed_ms),
            "sample time must increase"
        );
        ensure!(raw.len() <= MAX_SAMPLE_BYTES, "sample exceeds 64 KiB");
        let sanitized = session_evidence::sanitize(raw)?;
        let capture = session_evidence::parse(&sanitized)?;
        ensure!(
            matches!(capture, Capture::Snapshot { .. }),
            "recorder expects session-status, not a stop report"
        );
        let file = sample_name(self.samples.len());
        if let Err(error) = write_new(&self.directory.join(&file), &sanitized) {
            self.write_failed = true;
            return Err(error);
        }
        self.samples.push(Sample {
            file,
            elapsed_ms,
            sha256: digest(&sanitized),
        });
        self.captures.push(capture);
        Ok(())
    }
    pub fn finish(self, end: End, elapsed_ms: u64) -> Result<Summary> {
        ensure!(
            !self.write_failed,
            "write failure leaves .INCOMPLETE; do not overwrite or resume it"
        );
        ensure!(
            self.samples
                .last()
                .is_none_or(|s| elapsed_ms >= s.elapsed_ms),
            "finish precedes last sample"
        );
        ensure!(
            end != End::DurationReached
                || (elapsed_ms >= self.plan.seconds * 1000 && !self.samples.is_empty()),
            "requested duration was not reached"
        );
        let analysis_sha256 = if self.captures.is_empty() {
            None
        } else {
            let bytes = serde_json::to_vec_pretty(&session_evidence::analyze(self.captures)?)?;
            ensure!(bytes.len() <= MAX_ANALYSIS_BYTES, "analysis exceeds 16 MiB");
            write_new(&self.directory.join("analysis.json"), &bytes)?;
            Some(digest(&bytes))
        };
        let summary = Summary {
            schema: 1,
            software_version: env!("CARGO_PKG_VERSION").into(),
            plan: self.plan,
            end,
            elapsed_ms,
            requested_window_complete: end == End::DurationReached,
            validation_claimed: false,
            samples: self.samples,
            analysis_sha256,
        };
        write_new(
            &self.directory.join("summary.json"),
            &serde_json::to_vec_pretty(&summary)?,
        )?;
        fs::remove_file(self.directory.join(MARKER))?;
        Ok(summary)
    }
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "expected a regular recording file"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "recording reparse point refused"
        );
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "recording file exceeds limit");
    Ok(bytes)
}
/// Rechecks exact sanitized bytes, ordered names and derived analysis offline.
/// Hashes detect damage, not a forged recording or a genuine gameplay result.
pub fn verify(directory: &Path) -> Result<Summary> {
    let metadata = fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "expected a recording directory"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes() & 0x400 == 0,
            "recording directory reparse point refused"
        );
    }
    ensure!(
        fs::symlink_metadata(directory.join(MARKER))
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "recording is incomplete or marker cannot be checked"
    );
    let summary: Summary = serde_json::from_slice(&read_bounded(
        &directory.join("summary.json"),
        MAX_SAMPLE_BYTES,
    )?)?;
    summary.plan.validate()?;
    ensure!(
        summary.schema == 1
            && !summary.validation_claimed
            && summary.samples.len() <= session_evidence::MAX_INPUTS,
        "invalid recording summary"
    );
    ensure!(
        summary.requested_window_complete == (summary.end == End::DurationReached),
        "inconsistent recording outcome"
    );
    ensure!(
        summary.end != End::DurationReached
            || (summary.elapsed_ms >= summary.plan.seconds * 1000 && !summary.samples.is_empty()),
        "duration was not recorded"
    );
    let mut captures = Vec::new();
    let mut previous = None;
    for (index, sample) in summary.samples.iter().enumerate() {
        ensure!(
            sample.file == sample_name(index)
                && previous.is_none_or(|ms| sample.elapsed_ms > ms)
                && sample.elapsed_ms <= summary.elapsed_ms,
            "invalid sample order/name/time"
        );
        previous = Some(sample.elapsed_ms);
        let bytes = read_bounded(&directory.join(&sample.file), MAX_SAMPLE_BYTES)?;
        ensure!(
            digest(&bytes) == sample.sha256 && session_evidence::sanitize(&bytes)? == bytes,
            "sample hash or sanitization mismatch"
        );
        let capture = session_evidence::parse(&bytes)?;
        ensure!(
            matches!(capture, Capture::Snapshot { .. }),
            "invalid recorded sample"
        );
        captures.push(capture);
    }
    ensure!(
        summary.analysis_sha256.is_some() != captures.is_empty(),
        "missing or unexpected analysis"
    );
    if let Some(hash) = &summary.analysis_sha256 {
        let bytes = read_bounded(&directory.join("analysis.json"), MAX_ANALYSIS_BYTES)?;
        ensure!(
            digest(&bytes) == *hash
                && bytes == serde_json::to_vec_pretty(&session_evidence::analyze(captures)?)?,
            "derived analysis mismatch"
        );
    }
    let expected_files = summary.samples.len() + 1 + usize::from(summary.analysis_sha256.is_some());
    let mut count = 0;
    for entry in fs::read_dir(directory)? {
        entry?;
        count += 1;
        ensure!(count <= expected_files, "unexpected recording entries");
    }
    ensure!(count == expected_files, "missing recording entries");
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn own_status() -> Vec<u8> {
        serde_json::to_vec(&json!({"client":{"status":1,"sample_state":1,"sent":10,"received":8,"diagnostic":"private"},
            "draw":{"draw_calls":2,"flush_calls":8,"foreign_calls":0,"flags":0},
            "visual":{"item":123,"shown":1,"drawn":2,"fault":0},
            "network_session":null,"network_timing":null,
            "presentation":{"config":{"delay_ms":100,"extrapolation_ms":200},"revision":1,"buffering":0,"interpolated":0,"predicted":0,"capped":0,"held":0,"snapped":0,"stale":0,"empty_queries":0,"lock_misses":0},
            "remotes":[],"ticket":"private","path":"private","position":[1,2,3]})).unwrap()
    }
    fn plan() -> Plan {
        Plan {
            seconds: 1,
            interval_ms: 1000,
        }
    }
    #[test]
    fn finalized_recording_is_bounded_sanitized_and_damage_is_detected_offline() {
        let parent = tempfile::tempdir().unwrap();
        let directory = parent.path().join("recording");
        let mut recorder = Recorder::create(&directory, plan()).unwrap();
        assert!(verify(&directory).is_err());
        recorder.push(0, &own_status()).unwrap();
        assert!(recorder.push(0, &own_status()).is_err());
        recorder.push(1000, &own_status()).unwrap();
        let summary = recorder.finish(End::DurationReached, 1000).unwrap();
        assert!(summary.requested_window_complete && !summary.validation_claimed);
        assert_eq!(verify(&directory).unwrap().samples.len(), 2);
        for name in ["summary.json", "analysis.json", "snapshot-0001.json"] {
            let text = fs::read_to_string(directory.join(name)).unwrap();
            assert!(
                !text.contains("private") && !text.contains("ticket") && !text.contains("position")
            );
        }
        assert!(Recorder::create(&directory, plan()).is_err());
        fs::write(directory.join("snapshot-0001.json"), b"{}").unwrap();
        assert!(verify(&directory).is_err());
        assert!(
            Plan {
                seconds: 1800,
                interval_ms: 1000
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn abort_or_collision_cannot_be_reported_as_complete_or_overwrite_existing_files() {
        let parent = tempfile::tempdir().unwrap();
        let directory = parent.path().join("aborted");
        let recorder = Recorder::create(&directory, plan()).unwrap();
        let summary = recorder.finish(End::CollectionFailed, 0).unwrap();
        assert!(!summary.requested_window_complete && summary.analysis_sha256.is_none());
        verify(&directory).unwrap();
        let directory = parent.path().join("collision");
        let mut recorder = Recorder::create(&directory, plan()).unwrap();
        fs::write(directory.join("snapshot-0001.json"), b"preserve").unwrap();
        assert!(recorder.push(0, &own_status()).is_err());
        assert!(recorder.finish(End::CollectionFailed, 1).is_err());
        assert_eq!(
            fs::read(directory.join("snapshot-0001.json")).unwrap(),
            b"preserve"
        );
        assert!(directory.join(MARKER).exists());
        assert!(!directory.join("summary.json").exists());
    }
    #[test]
    fn manipulation_of_names_analysis_and_extra_files_is_refused() {
        let parent = tempfile::tempdir().unwrap();
        let directory = parent.path().join("recording");
        let mut recorder = Recorder::create(&directory, plan()).unwrap();
        recorder.push(0, &own_status()).unwrap();
        recorder.finish(End::Interrupted, 200).unwrap();
        verify(&directory).unwrap();
        fs::write(directory.join("extra"), b"x").unwrap();
        assert!(verify(&directory).is_err());
        fs::remove_file(directory.join("extra")).unwrap();
        let path = directory.join("summary.json");
        let mut summary: Summary = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        summary.samples[0].file = "../outside.json".into();
        fs::write(&path, serde_json::to_vec(&summary).unwrap()).unwrap();
        assert!(verify(&directory).is_err());
        summary.samples[0].file = sample_name(0);
        fs::write(&path, serde_json::to_vec(&summary).unwrap()).unwrap();
        fs::write(directory.join("analysis.json"), b"{}").unwrap();
        assert!(verify(&directory).is_err());
    }
}

use super::process::Identity;
use anyhow::{Context, Result, ensure};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"AAMEM01\0";
pub const MAX_CANDIDATES: usize = 8_000_000;
const MAX_HEADER: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ValueEnum)]
pub enum Kind {
    F32,
    F64,
}

impl Kind {
    pub fn width(self) -> usize {
        match self {
            Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    pub fn decode(self, bytes: &[u8]) -> f64 {
        match self {
            Self::F32 => f64::from(f32::from_le_bytes(
                bytes[..4].try_into().expect("f32-sized buffer"),
            )),
            Self::F64 => f64::from_le_bytes(bytes[..8].try_into().expect("f64-sized buffer")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub identity: Identity,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub address: u64,
    pub value: f64,
}

pub struct Snapshot {
    pub header: Header,
    pub candidates: Vec<Candidate>,
}

impl Snapshot {
    fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.header.identity.pointer_bytes, 4 | 8),
            "invalid pointer size"
        );
        ensure!(
            self.candidates.len() <= MAX_CANDIDATES,
            "too many candidates"
        );
        let mut previous = 0;
        let max = if self.header.identity.pointer_bytes == 4 {
            u64::from(u32::MAX)
        } else {
            u64::MAX
        };
        for candidate in &self.candidates {
            ensure!(
                candidate.address > previous && candidate.address <= max,
                "invalid, duplicate or unsorted candidate address"
            );
            ensure!(candidate.value.is_finite(), "candidate is NaN/infinite");
            previous = candidate.address;
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        ensure!(
            path.extension().is_some_and(|ext| ext == "aamem"),
            "scan files must end in .aamem so they stay out of Git"
        );
        self.validate()?;
        let header = serde_json::to_vec(&self.header)?;
        ensure!(header.len() <= MAX_HEADER, "snapshot header too large");
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("scan output must be a new file: {}", path.display()))?;
        let mut writer = BufWriter::new(file);
        writer.write_all(MAGIC)?;
        writer.write_all(&(header.len() as u32).to_le_bytes())?;
        writer.write_all(&header)?;
        writer.write_all(&(self.candidates.len() as u64).to_le_bytes())?;
        for candidate in &self.candidates {
            writer.write_all(&candidate.address.to_le_bytes())?;
            writer.write_all(&candidate.value.to_le_bytes())?;
        }
        writer.flush()?;
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let mut reader = BufReader::new(file);
        let mut magic = [0; 8];
        reader.read_exact(&mut magic)?;
        ensure!(&magic == MAGIC, "not a supported .aamem snapshot");
        let mut length = [0; 4];
        reader.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        ensure!(length <= MAX_HEADER, "snapshot header exceeds limit");
        let mut header = vec![0; length];
        reader.read_exact(&mut header)?;
        let header = serde_json::from_slice(&header)?;
        let mut count = [0; 8];
        reader.read_exact(&mut count)?;
        let count = u64::from_le_bytes(count);
        ensure!(
            count <= MAX_CANDIDATES as u64,
            "snapshot exceeds candidate limit"
        );
        ensure!(
            size == 20 + length as u64 + count * 16,
            "snapshot length does not match its record count"
        );
        let mut candidates = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let mut entry = [0; 16];
            reader.read_exact(&mut entry)?;
            candidates.push(Candidate {
                address: u64::from_le_bytes(entry[..8].try_into()?),
                value: f64::from_le_bytes(entry[8..].try_into()?),
            });
        }
        let result = Self { header, candidates };
        result.validate()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_format_roundtrip_and_rejects_corruption_and_overwrite() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("own.aamem");
        let snapshot = Snapshot {
            header: Header {
                identity: Identity {
                    pid: 42,
                    created: 99,
                    executable: "own-fixture.exe".into(),
                    pointer_bytes: 4,
                },
                kind: Kind::F64,
            },
            candidates: vec![Candidate {
                address: 0x1234,
                value: 19.25,
            }],
        };
        snapshot.save(&path).unwrap();
        let loaded = Snapshot::load(&path).unwrap();
        assert_eq!(loaded.header.kind, Kind::F64);
        assert_eq!(loaded.candidates, snapshot.candidates);
        assert!(snapshot.save(&path).is_err());
        assert!(snapshot.save(&temporary.path().join("leak.rs")).is_err());
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"extra")
            .unwrap();
        assert!(Snapshot::load(&path).is_err());
        let bad = temporary.path().join("bad.aamem");
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&bad, bytes).unwrap();
        assert!(Snapshot::load(&bad).is_err());
    }
}

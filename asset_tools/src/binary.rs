//! Bounded, read-only inspection of proprietary game and save files.
use anyhow::{Result, ensure};
use clap::ValueEnum;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum NumberKind {
    F32,
    F64,
    I32,
}

impl NumberKind {
    fn width(self) -> usize {
        match self {
            Self::F64 => 8,
            _ => 4,
        }
    }
    fn decode(self, bytes: &[u8]) -> f64 {
        match self {
            Self::F32 => f64::from(f32::from_le_bytes(
                bytes[..4].try_into().expect("four-byte window"),
            )),
            Self::F64 => f64::from_le_bytes(bytes[..8].try_into().expect("eight-byte window")),
            Self::I32 => f64::from(i32::from_le_bytes(
                bytes[..4].try_into().expect("four-byte window"),
            )),
        }
    }
}

fn patterns(
    bytes: &[u8],
    kind: NumberKind,
    minimum: f64,
    maximum: f64,
    small_max: f64,
) -> impl Iterator<Item = (usize, [f64; 3])> + '_ {
    let width = kind.width();
    bytes
        .windows(width * 3)
        .enumerate()
        .filter_map(move |(at, window)| {
            let values = std::array::from_fn(|index| {
                kind.decode(&window[index * width..(index + 1) * width])
            });
            let [a, b, c] = values;
            (values.iter().all(|v| v.is_finite())
                && a.abs() >= minimum
                && a.abs() <= maximum
                && b.abs() <= small_max
                && c.abs() >= minimum
                && c.abs() <= maximum)
                .then_some((at, values))
        })
}

/// Find plausible packed numeric triples without assuming a save-file schema.
/// The middle component is treated only as a small-component hypothesis.
pub fn vectors(
    path: &Path,
    minimum: f64,
    maximum: f64,
    small_max: f64,
    limit: usize,
    kind: NumberKind,
) -> Result<()> {
    ensure!(
        minimum.is_finite()
            && maximum.is_finite()
            && small_max.is_finite()
            && minimum >= 0.0
            && maximum >= minimum
            && small_max >= 0.0,
        "invalid vector search bounds"
    );
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= 16 * 1024 * 1024,
        "vector inspection is limited to regular files up to 16 MiB"
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "input grew beyond the inspection limit"
    );
    let mut hits = 0;
    for (at, [a, b, c]) in patterns(&bytes, kind, minimum, maximum, small_max) {
        if hits < limit {
            println!("0x{at:08x} ({at})  ({a:.9}, {b:.9}, {c:.9})");
        }
        hits += 1;
    }
    println!(
        "{hits} candidate {kind:?} triples; these are numeric patterns, not confirmed coordinates"
    );
    Ok(())
}

pub fn offset(text: &str) -> Result<u64, String> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16),
        None => text.parse(),
    }
    .map_err(|_| "expected a decimal offset or 0x-prefixed hexadecimal offset".into())
}

pub fn inspect(path: &Path, offset: u64, length: usize, numbers: bool) -> Result<()> {
    ensure!(
        (1..=4096).contains(&length),
        "inspection length must be 1..=4096 bytes"
    );
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "input must be a regular file");
    ensure!(offset < metadata.len(), "offset must be inside the file");
    let length = (length as u64).min(metadata.len() - offset) as usize;
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    println!(
        "{}: {} bytes total, inspecting {} bytes at 0x{offset:x} ({offset})",
        path.display(),
        metadata.len(),
        length
    );
    if numbers {
        for at in (0..length.saturating_sub(3)).step_by(4) {
            let four: [u8; 4] = bytes[at..at + 4].try_into()?;
            let double = if at + 8 <= length {
                format!("{:.10e}", f64::from_le_bytes(bytes[at..at + 8].try_into()?))
            } else {
                "-".into()
            };
            println!(
                "0x{:08x} ({:>8}) u32={:>10} i32={:>11} f32={:>17.7} f64={double}",
                offset + at as u64,
                offset + at as u64,
                u32::from_le_bytes(four),
                i32::from_le_bytes(four),
                f32::from_le_bytes(four)
            );
        }
    } else {
        for (index, chunk) in bytes.chunks(16).enumerate() {
            let hex = chunk
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            let text: String = chunk
                .iter()
                .map(|byte| {
                    if byte.is_ascii_graphic() || *byte == b' ' {
                        char::from(*byte)
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("0x{:08x}  {hex:<47}  {text}", offset + (index * 16) as u64);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unaligned_own_double_and_integer_records_without_assuming_schema() {
        let mut bytes = b"own:".to_vec();
        bytes.push(0);
        for value in [12345.25f64, 12.5, -23456.75] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let found: Vec<_> = patterns(&bytes, NumberKind::F64, 1000.0, 100000.0, 100.0).collect();
        assert!(found.contains(&(5, [12345.25, 12.5, -23456.75])));
        let mut bytes = vec![0xff];
        for value in [12345i32, 12, -23456] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert!(
            patterns(&bytes, NumberKind::I32, 1000.0, 100000.0, 100.0)
                .any(|item| item == (1, [12345.0, 12.0, -23456.0]))
        );
    }
}

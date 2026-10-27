use super::{
    process::{MemoryScope, Process},
    snapshot::{Candidate, Header, Kind, MAX_CANDIDATES, Snapshot},
};
use anyhow::{Result, ensure};
use clap::ValueEnum;

#[derive(Clone, Copy)]
pub struct Options {
    pub start: u64,
    pub end: Option<u64>,
    pub kind: Kind,
    pub alignment: u64,
    pub min: f64,
    pub max: f64,
    pub min_abs: f64,
    pub all_readable: bool,
    pub memory: MemoryScope,
    pub max_bytes: u64,
    pub max_candidates: usize,
}

#[derive(Default, Debug)]
pub struct Report {
    pub bytes_read: u64,
    pub unreadable: usize,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Change {
    Changed,
    Unchanged,
    Increased,
    Decreased,
}

pub fn matches(change: Change, old: f64, new: f64, epsilon: f64, max_delta: Option<f64>) -> bool {
    let delta = new - old;
    new.is_finite()
        && delta.is_finite()
        && max_delta.is_none_or(|max| delta.abs() <= max)
        && match change {
            Change::Changed => delta.abs() > epsilon,
            Change::Unchanged => delta.abs() <= epsilon,
            Change::Increased => delta > epsilon,
            Change::Decreased => delta < -epsilon,
        }
}

pub fn scan(process: &Process, options: Options) -> Result<(Snapshot, Report)> {
    process.ensure_running()?;
    ensure!(
        options.min.is_finite() && options.max.is_finite() && options.min <= options.max,
        "invalid finite value range"
    );
    ensure!(
        options.min_abs.is_finite() && options.min_abs >= 0.0,
        "min-abs must be finite and nonnegative"
    );
    ensure!(
        matches!(options.alignment, 1 | 4 | 8),
        "alignment must be 1, 4 or 8"
    );
    ensure!(
        (1..=MAX_CANDIDATES).contains(&options.max_candidates),
        "candidate limit must be 1..={MAX_CANDIDATES}"
    );
    ensure!(
        options.end.is_none_or(|end| end > options.start),
        "end must be greater than start"
    );
    ensure!(
        options.start <= process.max_address(),
        "start exceeds target address space"
    );
    let regions: Vec<_> = process
        .region_details(options.start, options.end, options.all_readable)?
        .into_iter()
        .filter(|region| options.memory.accepts(region.kind))
        .collect();
    let selected_bytes: u64 = regions.iter().map(|region| region.end - region.start).sum();
    ensure!(
        selected_bytes <= options.max_bytes,
        "selected readable regions total {} MiB, above limit {} MiB; narrow --start/--end or raise --max-mib",
        selected_bytes / 1048576,
        options.max_bytes / 1048576
    );
    let mut candidates = Vec::new();
    let mut report = Report::default();
    let width = options.kind.width();
    let mut buffer = [0u8; 4096 + 8];
    for region in regions {
        let (from, to) = (region.start, region.end);
        let mut at = from;
        while at < to {
            let advance = 4096.min(to - at);
            let mut len = (advance + width as u64 - 1).min(to - at) as usize;
            if process.read(at, &mut buffer[..len]).is_err() {
                // A page may have changed protection since VirtualQueryEx.
                len = advance as usize;
                if process.read(at, &mut buffer[..len]).is_err() {
                    report.unreadable += 1;
                    at += advance;
                    continue;
                }
            }
            report.bytes_read += len as u64;
            let first = (options.alignment - at % options.alignment) % options.alignment;
            for offset in (first as usize..advance as usize).step_by(options.alignment as usize) {
                if offset + width > len {
                    break;
                }
                let value = options.kind.decode(&buffer[offset..offset + width]);
                if value.is_finite()
                    && value >= options.min
                    && value <= options.max
                    && value.abs() >= options.min_abs
                {
                    ensure!(
                        candidates.len() < options.max_candidates,
                        "candidate limit reached; narrow the address/value range; no truncated snapshot was saved"
                    );
                    candidates.push(Candidate {
                        address: at + offset as u64,
                        value,
                    });
                }
            }
            at += advance;
        }
    }
    process.ensure_running()?;
    Ok((
        Snapshot {
            header: Header {
                identity: process.identity.clone(),
                kind: options.kind,
            },
            candidates,
        },
        report,
    ))
}

pub fn refine(
    process: &Process,
    previous: &Snapshot,
    change: Change,
    epsilon: f64,
    max_delta: Option<f64>,
) -> Result<(Snapshot, Report)> {
    process.ensure_running()?;
    process.same_session(&previous.header.identity)?;
    ensure!(
        epsilon.is_finite() && epsilon >= 0.0,
        "epsilon must be finite and nonnegative"
    );
    ensure!(
        max_delta.is_none_or(|max| max.is_finite() && max > 0.0),
        "max-delta must be finite and positive"
    );
    let mut output = Vec::new();
    let mut report = Report::default();
    let width = previous.header.kind.width();
    let mut start = 0;
    // Group sorted candidates by page: one bulk read per group, with per-value
    // fallback when page protection changes or a value crosses a page boundary.
    while start < previous.candidates.len() {
        let first = previous.candidates[start].address;
        let mut end = start + 1;
        while end < previous.candidates.len()
            && previous.candidates[end].address / 4096 == first / 4096
        {
            end += 1;
        }
        let length = (previous.candidates[end - 1].address - first) as usize + width;
        let mut buffer = vec![0; length];
        let bulk = process.read(first, &mut buffer).is_ok();
        if bulk {
            report.bytes_read += length as u64;
        }
        for old in &previous.candidates[start..end] {
            let mut value_buffer = [0; 8];
            let bytes = if bulk {
                let offset = (old.address - first) as usize;
                &buffer[offset..offset + width]
            } else {
                if process
                    .read(old.address, &mut value_buffer[..width])
                    .is_err()
                {
                    report.unreadable += 1;
                    continue;
                }
                report.bytes_read += width as u64;
                &value_buffer[..width]
            };
            let current = previous.header.kind.decode(bytes);
            if matches(change, old.value, current, epsilon, max_delta) {
                output.push(Candidate {
                    address: old.address,
                    value: current,
                });
            }
        }
        start = end;
    }
    process.ensure_running()?;
    Ok((
        Snapshot {
            header: previous.header.clone(),
            candidates: output,
        },
        report,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_filters_respect_noise_direction_and_jump_limits() {
        assert!(matches(Change::Unchanged, 10.0, 10.00001, 0.001, None));
        assert!(!matches(Change::Changed, 10.0, 10.00001, 0.001, None));
        assert!(matches(Change::Increased, 10.0, 12.0, 0.001, Some(5.0)));
        assert!(matches(Change::Decreased, 10.0, 8.0, 0.001, None));
        assert!(!matches(Change::Changed, 10.0, 1000.0, 0.001, Some(5.0)));
        assert!(!matches(Change::Changed, 10.0, f64::NAN, 0.001, None));
        assert_eq!(Kind::F64.decode(&12345.125f64.to_le_bytes()), 12345.125);
    }
}

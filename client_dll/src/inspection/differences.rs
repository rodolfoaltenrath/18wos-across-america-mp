use super::snapshot::Snapshot;
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug)]
pub struct Difference {
    pub address: u64,
    pub before: f64,
    pub after: f64,
}

impl Difference {
    pub fn delta(self) -> f64 {
        self.after - self.before
    }
}

/// Compare only addresses present in both files, retaining exact numeric
/// changes. No live process reads are performed.
pub fn differences(before: &Snapshot, after: &Snapshot) -> Result<Vec<Difference>> {
    ensure!(
        before.header.identity == after.header.identity,
        "snapshots belong to different process sessions"
    );
    ensure!(
        before.header.kind == after.header.kind,
        "snapshots use different numeric kinds"
    );
    let mut output = Vec::new();
    let (mut left, mut right) = (0, 0);
    while left < before.candidates.len() && right < after.candidates.len() {
        let old = before.candidates[left];
        let new = after.candidates[right];
        match old.address.cmp(&new.address) {
            std::cmp::Ordering::Less => left += 1,
            std::cmp::Ordering::Greater => right += 1,
            std::cmp::Ordering::Equal => {
                let delta = new.value - old.value;
                if delta.is_finite() && delta != 0.0 {
                    output.push(Difference {
                        address: old.address,
                        before: old.value,
                        after: new.value,
                    });
                }
                left += 1;
                right += 1;
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspection::{
        process::Identity,
        snapshot::{Candidate, Header, Kind},
    };

    #[test]
    fn compares_sparse_intersection_and_rejects_incompatible_captures() {
        let header = Header {
            identity: Identity {
                pid: 1,
                created: 99,
                executable: "own.exe".into(),
                pointer_bytes: 4,
            },
            kind: Kind::F32,
        };
        let before = Snapshot {
            header: header.clone(),
            candidates: vec![
                Candidate {
                    address: 4,
                    value: 10.0,
                },
                Candidate {
                    address: 8,
                    value: 20.0,
                },
                Candidate {
                    address: 16,
                    value: 30.0,
                },
            ],
        };
        let mut after = Snapshot {
            header,
            candidates: vec![
                Candidate {
                    address: 8,
                    value: 22.5,
                },
                Candidate {
                    address: 12,
                    value: 77.0,
                },
                Candidate {
                    address: 16,
                    value: 30.0,
                },
            ],
        };
        let changes = differences(&before, &after).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, 8);
        assert_eq!(changes[0].delta(), 2.5);
        after.header.kind = Kind::F64;
        assert!(differences(&before, &after).is_err());
        after.header.kind = Kind::F32;
        after.header.identity.created += 1;
        assert!(differences(&before, &after).is_err());
    }
}

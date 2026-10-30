//! Authoritative sidecar ledger. Local host administration only; no client pose
//! is accepted as delivery evidence and no commercial-game balance is modified.
use crate::{
    career::{self, Manifest},
    career_store::{self, Journal},
    protocol::{Position, Rotation},
    session_protocol::identity_valid,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const MAX_COMPANIES: usize = 64;
const MAX_CONTRACTS: usize = 128;
const MAX_OPERATIONS: usize = 512;
const MAX_MONEY: u64 = 1_000_000_000_000_000;
fn label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 64 && !value.chars().any(char::is_control)
}
fn place(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clock {
    pub world_ms: u64,
    pub paused: bool,
    pub scale: u32,
}
impl Clock {
    fn advance(&mut self, real_ms: u64) -> Result<(), String> {
        if self.paused {
            return Ok(());
        }
        self.world_ms = real_ms
            .checked_mul(u64::from(self.scale))
            .and_then(|n| self.world_ms.checked_add(n))
            .ok_or("world clock overflow")?;
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub sequence: u64,
    pub vehicle: String,
    pub position: Position,
    pub rotation: Rotation,
    pub contract: Option<String>,
    pub cargo: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Company {
    pub name: String,
    pub balance_minor: u64,
    pub checkpoint: Option<Checkpoint>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Progress {
    Offered,
    Accepted {
        player: String,
        cargo: String,
    },
    Delivered {
        player: String,
        cargo: String,
        host_attestation_sha256: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub origin: String,
    pub destination: String,
    pub reward_minor: u64,
    pub progress: Progress,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Register {
        player: String,
        name: String,
    },
    Offer {
        contract: String,
        origin: String,
        destination: String,
        reward_minor: u64,
    },
    Accept {
        player: String,
        contract: String,
    },
    Checkpoint {
        player: String,
        vehicle: String,
        position: Position,
        rotation: Rotation,
    },
    /// Explicit host attestation, NOT an untrusted automatic client delivery.
    Deliver {
        player: String,
        contract: String,
        host_attestation_sha256: String,
    },
    ConfigureClock {
        paused: bool,
        scale: u32,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Registered,
    Offered,
    Accepted {
        cargo: String,
    },
    Checkpointed {
        sequence: u64,
    },
    Paid {
        reward_minor: u64,
        balance_minor: u64,
    },
    ClockConfigured,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recorded {
    request_sha256: String,
    outcome: Outcome,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    schema: u32,
    career: String,
    manifest_sha256: String,
    pub clock: Clock,
    pub companies: BTreeMap<String, Company>,
    pub contracts: BTreeMap<String, Contract>,
    operations: BTreeMap<String, Recorded>,
}
impl State {
    pub fn new(manifest: &Manifest) -> Result<Self, String> {
        manifest.validate()?;
        Ok(Self {
            schema: 1,
            career: manifest.career.clone(),
            manifest_sha256: career::hash(&career_store::json(manifest)?),
            clock: Clock {
                world_ms: 0,
                paused: false,
                scale: 1,
            },
            companies: BTreeMap::new(),
            contracts: BTreeMap::new(),
            operations: BTreeMap::new(),
        })
    }
    pub fn validate(&self, manifest: &Manifest) -> Result<(), String> {
        manifest.validate()?;
        if self.schema != 1
            || self.career != manifest.career
            || self.manifest_sha256 != career::hash(&career_store::json(manifest)?)
            || !(1..=3600).contains(&self.clock.scale)
            || self.companies.len() > MAX_COMPANIES
            || self.contracts.len() > MAX_CONTRACTS
            || self.operations.len() > MAX_OPERATIONS
        {
            return Err("invalid/bounded career state or manifest binding".into());
        }
        let mut earned: BTreeMap<&str, u64> =
            self.companies.keys().map(|k| (k.as_str(), 0)).collect();
        let mut active = BTreeMap::new();
        let mut cargos = std::collections::BTreeSet::new();
        for (id, c) in &self.contracts {
            if !identity_valid(id)
                || !place(&c.origin)
                || !place(&c.destination)
                || c.origin == c.destination
                || !(1..=MAX_MONEY).contains(&c.reward_minor)
            {
                return Err("invalid contract".into());
            }
            let assignment = match &c.progress {
                Progress::Offered => None,
                Progress::Accepted { player, cargo } => {
                    if active
                        .insert(player.as_str(), (id.as_str(), cargo.as_str()))
                        .is_some()
                    {
                        return Err("player has multiple active contracts".into());
                    }
                    Some((player, cargo))
                }
                Progress::Delivered {
                    player,
                    cargo,
                    host_attestation_sha256,
                } => {
                    if !career::hash_valid(host_attestation_sha256) {
                        return Err("invalid host attestation".into());
                    }
                    let balance = earned
                        .get_mut(player.as_str())
                        .ok_or("delivery to missing company")?;
                    *balance = balance
                        .checked_add(c.reward_minor)
                        .filter(|n| *n <= MAX_MONEY)
                        .ok_or("balance overflow")?;
                    Some((player, cargo))
                }
            };
            if let Some((player, cargo)) = assignment
                && (!self.companies.contains_key(player)
                    || !identity_valid(cargo)
                    || !cargos.insert(cargo))
            {
                return Err("invalid cargo/owner identity".into());
            }
        }
        for (id, c) in &self.companies {
            if !identity_valid(id) || !label(&c.name) || c.balance_minor != earned[id.as_str()] {
                return Err("invalid company or inconsistent credit ledger".into());
            }
            if let Some(cp) = &c.checkpoint {
                if cp.sequence == 0
                    || !place(&cp.vehicle)
                    || !cp.position.is_valid()
                    || !cp.rotation.is_valid()
                {
                    return Err("invalid vehicle checkpoint".into());
                }
                let expected = active.get(id.as_str()).copied();
                let actual = match (&cp.contract, &cp.cargo) {
                    (Some(contract), Some(cargo)) => Some((contract.as_str(), cargo.as_str())),
                    (None, None) => None,
                    _ => return Err("partial cargo checkpoint".into()),
                };
                if actual != expected {
                    return Err("checkpoint/contract ownership mismatch".into());
                }
            }
        }
        for (id, op) in &self.operations {
            if !identity_valid(id) || !career::hash_valid(&op.request_sha256) {
                return Err("invalid operation history".into());
            }
        }
        Ok(())
    }
    fn apply(&mut self, operation_id: &str, operation: &Operation) -> Result<Outcome, String> {
        if !identity_valid(operation_id) {
            return Err("operation ID must be 128-bit lowercase hex".into());
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err("operation history full; deduplication cannot be evicted safely".into());
        }
        let outcome = match operation {
            Operation::Register { player, name } => {
                if !identity_valid(player)
                    || !label(name)
                    || self.companies.contains_key(player)
                    || self.companies.len() >= MAX_COMPANIES
                {
                    return Err("invalid/duplicate company or company limit".into());
                }
                self.companies.insert(
                    player.clone(),
                    Company {
                        name: name.clone(),
                        balance_minor: 0,
                        checkpoint: None,
                    },
                );
                Outcome::Registered
            }
            Operation::Offer {
                contract,
                origin,
                destination,
                reward_minor,
            } => {
                if !identity_valid(contract)
                    || !place(origin)
                    || !place(destination)
                    || origin == destination
                    || !(1..=MAX_MONEY).contains(reward_minor)
                    || self.contracts.contains_key(contract)
                    || self.contracts.len() >= MAX_CONTRACTS
                {
                    return Err("invalid/duplicate contract or contract limit".into());
                }
                self.contracts.insert(
                    contract.clone(),
                    Contract {
                        origin: origin.clone(),
                        destination: destination.clone(),
                        reward_minor: *reward_minor,
                        progress: Progress::Offered,
                    },
                );
                Outcome::Offered
            }
            Operation::Accept { player, contract } => {
                if !self.companies.contains_key(player) {
                    return Err("unknown company".into());
                }
                if self.contracts.values().any(|c| matches!(&c.progress,Progress::Accepted { player: owner, .. } if owner == player)) { return Err("company already carries an active contract".into()); }
                let c = self.contracts.get_mut(contract).ok_or("unknown contract")?;
                if !matches!(c.progress, Progress::Offered) {
                    return Err("contract already assigned/paid".into());
                }
                let cargo = operation_id.to_owned();
                c.progress = Progress::Accepted {
                    player: player.clone(),
                    cargo: cargo.clone(),
                };
                if let Some(cp) = &mut self.companies.get_mut(player).unwrap().checkpoint {
                    cp.contract = Some(contract.clone());
                    cp.cargo = Some(cargo.clone());
                }
                Outcome::Accepted { cargo }
            }
            Operation::Checkpoint {
                player,
                vehicle,
                position,
                rotation,
            } => {
                if !place(vehicle) || !position.is_valid() || !rotation.is_valid() {
                    return Err("invalid vehicle/pose".into());
                }
                let active = self.contracts.iter().find_map(|(id, c)| match &c.progress {
                    Progress::Accepted { player: p, cargo } if p == player => {
                        Some((id.clone(), cargo.clone()))
                    }
                    _ => None,
                });
                let company = self.companies.get_mut(player).ok_or("unknown company")?;
                let sequence = company.checkpoint.as_ref().map_or(Ok(1), |cp| {
                    cp.sequence
                        .checked_add(1)
                        .ok_or("checkpoint sequence exhausted")
                })?;
                let (contract, cargo) = match active {
                    Some((id, cargo)) => (Some(id), Some(cargo)),
                    None => (None, None),
                };
                company.checkpoint = Some(Checkpoint {
                    sequence,
                    vehicle: vehicle.clone(),
                    position: *position,
                    rotation: *rotation,
                    contract,
                    cargo,
                });
                Outcome::Checkpointed { sequence }
            }
            Operation::Deliver {
                player,
                contract,
                host_attestation_sha256,
            } => {
                if !career::hash_valid(host_attestation_sha256) {
                    return Err("explicit host attestation hash required".into());
                }
                let c = self.contracts.get_mut(contract).ok_or("unknown contract")?;
                let cargo = match &c.progress {
                    Progress::Accepted {
                        player: owner,
                        cargo,
                    } if owner == player => cargo.clone(),
                    _ => {
                        return Err(
                            "contract is not assigned to this company or already paid".into()
                        );
                    }
                };
                let company = self.companies.get_mut(player).ok_or("unknown company")?;
                let cp = company
                    .checkpoint
                    .as_mut()
                    .ok_or("transport checkpoint required before host delivery confirmation")?;
                if cp.contract.as_deref() != Some(contract) || cp.cargo.as_deref() != Some(&cargo) {
                    return Err("cargo checkpoint mismatch".into());
                }
                company.balance_minor = company
                    .balance_minor
                    .checked_add(c.reward_minor)
                    .filter(|n| *n <= MAX_MONEY)
                    .ok_or("balance overflow")?;
                cp.contract = None;
                cp.cargo = None;
                c.progress = Progress::Delivered {
                    player: player.clone(),
                    cargo,
                    host_attestation_sha256: host_attestation_sha256.clone(),
                };
                Outcome::Paid {
                    reward_minor: c.reward_minor,
                    balance_minor: company.balance_minor,
                }
            }
            Operation::ConfigureClock { paused, scale } => {
                if !(1..=3600).contains(scale) {
                    return Err("clock scale outside 1..3600".into());
                }
                self.clock.paused = *paused;
                self.clock.scale = *scale;
                Outcome::ClockConfigured
            }
        };
        self.operations.insert(
            operation_id.into(),
            Recorded {
                request_sha256: career::hash(&career_store::json(operation)?),
                outcome: outcome.clone(),
            },
        );
        Ok(outcome)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyView {
    pub player: String,
    pub company: Company,
    pub ledger_goal_reached: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: u32,
    pub career: String,
    pub checkpoint_revision: u64,
    pub clock: Clock,
    pub clock_checkpoint_world_ms: u64,
    pub ranking: Vec<CompanyView>,
    pub contracts: BTreeMap<String, Contract>,
    pub engine_adapters: bool,
    pub original_save_updated: bool,
}
impl Snapshot {
    pub fn validate(&self, manifest: &Manifest) -> Result<(), String> {
        if self.schema != 1
            || self.career != manifest.career
            || self.engine_adapters
            || self.original_save_updated
            || self.clock_checkpoint_world_ms > self.clock.world_ms
            || self.ranking.len() > MAX_COMPANIES
        {
            return Err("invalid or unsupported world snapshot".into());
        }
        let mut state = State::new(manifest)?;
        state.clock = self.clock.clone();
        state.contracts = self.contracts.clone();
        for company in &self.ranking {
            let goal = match manifest.goal {
                career::Goal::Unlimited => false,
                career::Goal::Financial { minor_units } => {
                    company.company.balance_minor >= minor_units
                }
            };
            if company.ledger_goal_reached != goal
                || state
                    .companies
                    .insert(company.player.clone(), company.company.clone())
                    .is_some()
            {
                return Err("invalid company/goal in world snapshot".into());
            }
        }
        if self.ranking.windows(2).any(|w| {
            w[0].company.balance_minor < w[1].company.balance_minor
                || (w[0].company.balance_minor == w[1].company.balance_minor
                    && w[0].player >= w[1].player)
        }) {
            return Err("invalid ranking order".into());
        }
        state.validate(manifest)
    }
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub operation: String,
    pub checkpoint_revision: u64,
    pub duplicate: bool,
    pub outcome: Outcome,
}
pub struct WorldStore {
    manifest: Manifest,
    journal: Journal<State>,
    current: State,
    clock_dirty: bool,
}
impl WorldStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let (manifest, _) = career_store::load(path)?;
        let journal = Journal::open(path, State::new(&manifest)?, |s| s.validate(&manifest))?;
        let current = journal.state().clone();
        Ok(Self {
            manifest,
            journal,
            current,
            clock_dirty: false,
        })
    }
    pub fn execute(&mut self, id: &str, operation: &Operation) -> Result<Receipt, String> {
        if self.journal.requires_recovery() {
            return Err("world requires recovery after uncertain commit".into());
        }
        if let Some(previous) = self.current.operations.get(id) {
            if previous.request_sha256 != career::hash(&career_store::json(operation)?) {
                return Err("operation ID reused with different arguments".into());
            }
            return Ok(Receipt {
                operation: id.into(),
                checkpoint_revision: self.journal.revision(),
                duplicate: true,
                outcome: previous.outcome.clone(),
            });
        }
        let mut next = self.current.clone();
        let outcome = next.apply(id, operation)?;
        let revision = self
            .journal
            .commit(next.clone(), |s| s.validate(&self.manifest))?;
        self.current = next;
        self.clock_dirty = false;
        Ok(Receipt {
            operation: id.into(),
            checkpoint_revision: revision,
            duplicate: false,
            outcome,
        })
    }
    pub fn advance(&mut self, real_ms: u64) -> Result<(), String> {
        let before = self.current.clock.world_ms;
        self.current.clock.advance(real_ms)?;
        self.clock_dirty |= self.current.clock.world_ms != before;
        Ok(())
    }
    pub fn flush(&mut self) -> Result<(), String> {
        if self.clock_dirty {
            self.journal
                .commit(self.current.clone(), |s| s.validate(&self.manifest))?;
            self.clock_dirty = false;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut ranking: Vec<_> = self
            .current
            .companies
            .iter()
            .map(|(player, company)| CompanyView {
                player: player.clone(),
                company: company.clone(),
                ledger_goal_reached: match self.manifest.goal {
                    career::Goal::Unlimited => false,
                    career::Goal::Financial { minor_units } => company.balance_minor >= minor_units,
                },
            })
            .collect();
        ranking.sort_by(|a, b| {
            b.company
                .balance_minor
                .cmp(&a.company.balance_minor)
                .then(a.player.cmp(&b.player))
        });
        Snapshot {
            schema: 1,
            career: self.current.career.clone(),
            checkpoint_revision: self.journal.revision(),
            clock: self.current.clock.clone(),
            clock_checkpoint_world_ms: self.journal.state().clock.world_ms,
            ranking,
            contracts: self.current.contracts.clone(),
            engine_adapters: false,
            original_save_updated: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::career_store::tests::Temp;
    fn id(n: u64) -> String {
        format!("{n:032x}")
    }
    fn setup() -> (Temp, std::path::PathBuf) {
        let dir = Temp::new();
        let save = dir.0.join("source.sav");
        career_store::write_new(&save, b"own fixture").unwrap();
        let path = dir.0.join("career");
        career_store::create(
            &path,
            &save,
            career::Goal::Financial { minor_units: 1000 },
            career::Compatibility {
                profile: crate::session_protocol::PROFILE.into(),
                map_sha256: career::hash(b"map"),
                mods_sha256: vec![],
            },
        )
        .unwrap();
        (dir, path)
    }
    fn register(w: &mut WorldStore, p: u64) {
        w.execute(
            &id(p),
            &Operation::Register {
                player: id(p),
                name: format!("Empresa {p}"),
            },
        )
        .unwrap();
    }
    fn offer(w: &mut WorldStore, n: u64) {
        w.execute(
            &id(n),
            &Operation::Offer {
                contract: id(n),
                origin: "miami".into(),
                destination: "houston".into(),
                reward_minor: 1500,
            },
        )
        .unwrap();
    }
    fn checkpoint(w: &mut WorldStore, n: u64) {
        w.execute(
            &id(n),
            &Operation::Checkpoint {
                player: id(1),
                vehicle: "truck".into(),
                position: Position {
                    x: 1.,
                    y: 2.,
                    z: 3.,
                },
                rotation: Rotation {
                    x: 0.,
                    y: 0.,
                    z: 0.,
                    w: 1.,
                },
            },
        )
        .unwrap();
    }
    #[test]
    fn exclusive_contract_checkpoint_payment_dedup_and_recovery() {
        let (_dir, path) = setup();
        let mut w = WorldStore::open(&path).unwrap();
        register(&mut w, 1);
        register(&mut w, 2);
        offer(&mut w, 10);
        let accept = Operation::Accept {
            player: id(1),
            contract: id(10),
        };
        w.execute(&id(11), &accept).unwrap();
        assert!(
            w.execute(
                &id(12),
                &Operation::Accept {
                    player: id(2),
                    contract: id(10)
                }
            )
            .is_err()
        );
        let deliver = Operation::Deliver {
            player: id(1),
            contract: id(10),
            host_attestation_sha256: career::hash(b"host fixture attestation, not gameplay"),
        };
        assert!(w.execute(&id(13), &deliver).is_err());
        checkpoint(&mut w, 14);
        drop(w); // Disconnect/restart with accepted cargo.
        let mut w = WorldStore::open(&path).unwrap();
        assert_eq!(
            w.snapshot().ranking[0]
                .company
                .checkpoint
                .as_ref()
                .unwrap()
                .cargo,
            Some(id(11))
        );
        assert!(w.execute(&id(11), &accept).unwrap().duplicate);
        let receipt = w.execute(&id(13), &deliver).unwrap();
        assert_eq!(
            receipt.outcome,
            Outcome::Paid {
                reward_minor: 1500,
                balance_minor: 1500
            }
        );
        drop(w); // Lost ACK after durable payment.
        let mut w = WorldStore::open(&path).unwrap();
        assert!(w.execute(&id(13), &deliver).unwrap().duplicate);
        assert!(w.execute(&id(15), &deliver).is_err());
        assert!(
            w.execute(
                &id(13),
                &Operation::ConfigureClock {
                    paused: true,
                    scale: 1
                }
            )
            .is_err()
        );
        let snapshot = w.snapshot();
        assert_eq!(snapshot.ranking[0].company.balance_minor, 1500);
        assert!(snapshot.ranking[0].ledger_goal_reached);
        assert!(
            snapshot.ranking[0]
                .company
                .checkpoint
                .as_ref()
                .unwrap()
                .cargo
                .is_none()
        );
        assert!(!snapshot.engine_adapters && !snapshot.original_save_updated);
    }
    #[test]
    fn clock_pause_scale_restart_and_overflow_are_explicit() {
        let (_dir, path) = setup();
        let mut w = WorldStore::open(&path).unwrap();
        w.advance(1000).unwrap();
        assert_eq!(w.snapshot().clock.world_ms, 1000);
        assert_eq!(w.snapshot().clock_checkpoint_world_ms, 0);
        w.execute(
            &id(1),
            &Operation::ConfigureClock {
                paused: true,
                scale: 60,
            },
        )
        .unwrap();
        w.advance(50_000).unwrap();
        assert_eq!(w.snapshot().clock.world_ms, 1000);
        drop(w);
        let mut w = WorldStore::open(&path).unwrap();
        assert_eq!(w.snapshot().clock.world_ms, 1000);
        w.execute(
            &id(2),
            &Operation::ConfigureClock {
                paused: false,
                scale: 60,
            },
        )
        .unwrap();
        w.advance(1000).unwrap();
        w.flush().unwrap();
        drop(w);
        let mut w = WorldStore::open(&path).unwrap();
        assert_eq!(w.snapshot().clock.world_ms, 61_000);
        assert!(w.advance(u64::MAX).is_err());
        assert_eq!(w.snapshot().clock.world_ms, 61_000);
        assert!(
            w.execute(
                &id(3),
                &Operation::ConfigureClock {
                    paused: false,
                    scale: 0
                }
            )
            .is_err()
        );
    }
    #[test]
    fn invalid_checkpoint_and_career_substitution_cannot_be_committed() {
        let (_dir, path) = setup();
        let mut w = WorldStore::open(&path).unwrap();
        register(&mut w, 1);
        assert!(
            w.execute(
                &id(3),
                &Operation::Checkpoint {
                    player: id(1),
                    vehicle: "../../save".into(),
                    position: Position {
                        x: f32::NAN,
                        y: 0.,
                        z: 0.
                    },
                    rotation: Rotation {
                        x: 0.,
                        y: 0.,
                        z: 0.,
                        w: 1.
                    }
                }
            )
            .is_err()
        );
        drop(w);
        let file = path.join("manifest.json");
        let mut manifest: Manifest = career_store::read_json(&file).unwrap();
        manifest.goal = career::Goal::Unlimited;
        std::fs::write(file, career_store::json(&manifest).unwrap()).unwrap();
        assert!(WorldStore::open(&path).is_err());
    }
    #[test]
    fn simultaneous_accepts_have_one_owner_and_one_durable_winner() {
        let (_dir, path) = setup();
        let mut world = WorldStore::open(&path).unwrap();
        register(&mut world, 1);
        register(&mut world, 2);
        offer(&mut world, 10);
        let world = std::sync::Arc::new(std::sync::Mutex::new(world));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let jobs: Vec<_> = (1..=2)
            .map(|p| {
                let world = world.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    world
                        .lock()
                        .unwrap()
                        .execute(
                            &id(p + 100),
                            &Operation::Accept {
                                player: id(p),
                                contract: id(10),
                            },
                        )
                        .is_ok()
                })
            })
            .collect();
        let wins = jobs
            .into_iter()
            .filter_map(|job| job.join().unwrap().then_some(()))
            .count();
        assert_eq!(wins, 1);
        drop(world);
        let recovered = WorldStore::open(&path).unwrap();
        assert!(matches!(
            recovered.snapshot().contracts[&id(10)].progress,
            Progress::Accepted { .. }
        ));
        assert_eq!(recovered.snapshot().checkpoint_revision, 4);
    }
    #[test]
    fn operation_limits_and_snapshot_tampering_do_not_erase_dedup_history() {
        let (_dir, path) = setup();
        let (manifest, _) = career_store::load(&path).unwrap();
        let mut state = State::new(&manifest).unwrap();
        for i in 0..MAX_OPERATIONS {
            state
                .apply(
                    &id(i as u64),
                    &Operation::ConfigureClock {
                        paused: false,
                        scale: 1,
                    },
                )
                .unwrap();
        }
        assert!(
            state
                .apply(
                    &id(MAX_OPERATIONS as u64),
                    &Operation::ConfigureClock {
                        paused: true,
                        scale: 1
                    }
                )
                .is_err()
        );
        assert_eq!(state.operations.len(), MAX_OPERATIONS);
        state.validate(&manifest).unwrap();
        let mut world = WorldStore::open(&path).unwrap();
        register(&mut world, 1);
        let mut snapshot = world.snapshot();
        snapshot.validate(&manifest).unwrap();
        snapshot.ranking[0].company.balance_minor = 100;
        assert!(snapshot.validate(&manifest).is_err());
        let mut snapshot = world.snapshot();
        snapshot.engine_adapters = true;
        assert!(snapshot.validate(&manifest).is_err());
    }
}

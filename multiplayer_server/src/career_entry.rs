//! Career admission and launcher preflight. Files remain private; no engine access.
pub mod assistant;
use crate::{
    career::{self, Compatibility, Manifest},
    career_store as store, identity, session_protocol as wire,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const LEASE_SECS: u64 = 3600;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub ticket: String,
}
impl fmt::Debug for Proof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CareerProof([redacted])")
    }
}
impl Proof {
    pub fn valid(&self) -> bool {
        wire::identity_valid(&self.ticket)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u32,
    pub career: String,
    pub manifest_sha256: String,
    pub save_sha256: String,
    pub bytes: u64,
    pub bootstrap_verified: bool,
    pub engine_loaded: bool,
    pub world_synchronized: bool,
}
pub fn inspect_import(path: &Path) -> Result<Receipt, String> {
    verified_import(path).map(|(_, receipt)| receipt)
}
fn verified_import(path: &Path) -> Result<(Manifest, Receipt), String> {
    if path
        .join("INCOMPLETE")
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Err("incomplete import".into());
    }
    let (manifest, _) = store::load(path)?;
    let receipt: Receipt = store::read_json(&path.join("receipt.json"))?;
    if receipt.schema != 1
        || !receipt.bootstrap_verified
        || receipt.engine_loaded
        || receipt.world_synchronized
        || receipt.career != manifest.career
        || receipt.manifest_sha256 != manifest_hash(&manifest)?
        || receipt.save_sha256 != manifest.bootstrap.sha256
        || receipt.bytes != manifest.bootstrap.bytes
    {
        return Err("inconsistent import receipt".into());
    }
    Ok((manifest, receipt))
}
pub fn manifest_hash(manifest: &Manifest) -> Result<String, String> {
    manifest.validate()?;
    Ok(career::hash(&store::json(manifest)?))
}
pub fn endpoint_valid(endpoint: SocketAddr) -> bool {
    endpoint.port() != 0
        && !endpoint.ip().is_unspecified()
        && !endpoint.ip().is_multicast()
        && !matches!(endpoint.ip(), std::net::IpAddr::V4(v) if v.is_broadcast())
}
pub fn fingerprint(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > 4 * 1024 * 1024 * 1024u64 {
        return Err("invalid archive size/type".into());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    // Recheck the opened handle too: the path may have changed after metadata().
    if !before.is_file() || before.len() == 0 || before.len() > 4 * 1024 * 1024 * 1024u64 {
        return Err("invalid opened archive size/type".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > before.len() {
            return Err("archive changed during fingerprint".into());
        }
        hash.update(&buffer[..count]);
    }
    let after = file.metadata().map_err(|e| e.to_string())?;
    if total != before.len()
        || before.len() != after.len()
        || before.modified().map_err(|e| e.to_string())?
            != after.modified().map_err(|e| e.to_string())?
    {
        return Err("archive changed during fingerprint".into());
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn compatibility(profile: &str, map: &Path, mods: &[PathBuf]) -> Result<Compatibility, String> {
    if mods.len() > career::MAX_MODS {
        return Err("too many mods".into());
    }
    let mut hashes = mods
        .iter()
        .map(|p| fingerprint(p))
        .collect::<Result<Vec<_>, _>>()?;
    hashes.sort();
    let value = Compatibility {
        profile: profile.into(),
        map_sha256: fingerprint(map)?,
        mods_sha256: hashes,
    };
    value.validate()?;
    Ok(value)
}

/// Private, portable launch capability. No paths, invite secret, or ledger authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub schema: u32,
    pub career: String,
    pub manifest_sha256: String,
    pub identity: String,
    pub room: String,
    pub relay: SocketAddr,
    pub compatibility: Compatibility,
    pub proof: Proof,
    /// Upper bound at issue time; the server alone checks its monotonic expiry.
    pub valid_for_secs: u64,
}
impl Launch {
    pub fn validate(&self) -> Result<(), String> {
        self.compatibility.validate()?;
        if self.schema != 1
            || !wire::identity_valid(&self.career)
            || !career::hash_valid(&self.manifest_sha256)
            || !wire::identity_valid(&self.identity)
            || !wire::room_valid(&self.room)
            || !endpoint_valid(self.relay)
            || !self.proof.valid()
            || !(1..=LEASE_SECS).contains(&self.valid_for_secs)
        {
            return Err("invalid career launch capability".into());
        }
        Ok(())
    }
    /// Revalidate local bytes and actual session inputs before any DLL operation.
    pub fn preflight(
        &self,
        import: &Path,
        local: &Compatibility,
        identity: &str,
        room: &str,
        relay: SocketAddr,
    ) -> Result<(), String> {
        self.validate()?;
        self.compatibility.compare(local)?;
        let (manifest, receipt) = verified_import(import)?;
        manifest.compatibility.compare(local)?;
        if receipt.career != self.career
            || receipt.manifest_sha256 != self.manifest_sha256
            || identity != self.identity
            || room != self.room
            || relay != self.relay
        {
            return Err("career import/identity/room/relay differs from launch capability".into());
        }
        Ok(())
    }
}

struct Lease {
    launch: Launch,
    expires: Instant,
}
struct State {
    leases: BTreeMap<String, Lease>,
}
/// One in-process authority shared by career TCP and its guarded UDP relay.
/// Restart discards tickets. Invite holders may claim identities: these are not accounts.
#[derive(Clone)]
pub struct Registry {
    manifest: Manifest,
    digest: String,
    room: String,
    relay: SocketAddr,
    state: Arc<Mutex<State>>,
}
impl Registry {
    pub fn new(manifest: Manifest, room: String, relay: SocketAddr) -> Result<Self, String> {
        let digest = manifest_hash(&manifest)?;
        if !wire::room_valid(&room) || !endpoint_valid(relay) {
            return Err("invalid career room/advertised relay".into());
        }
        Ok(Self {
            manifest,
            digest,
            room,
            relay,
            state: Arc::new(Mutex::new(State {
                leases: BTreeMap::new(),
            })),
        })
    }
    pub fn check_context(
        &self,
        manifest: &Manifest,
        room: Option<&str>,
        profile: Option<&str>,
    ) -> Result<(), String> {
        if manifest_hash(manifest)? != self.digest
            || room.is_some_and(|v| v != self.room)
            || profile.is_some_and(|v| v != self.manifest.compatibility.profile)
        {
            return Err("admission authority context mismatch".into());
        }
        Ok(())
    }
    pub fn issue(&self, identity: &str, receipt: &Receipt) -> Result<Launch, String> {
        self.issue_at(identity, receipt, Instant::now())
    }
    pub fn check_settings(&self, room: &str, profile: &str) -> Result<(), String> {
        if room != self.room || profile != self.manifest.compatibility.profile {
            return Err("admission room/profile mismatch".into());
        }
        Ok(())
    }
    fn issue_at(&self, identity: &str, receipt: &Receipt, now: Instant) -> Result<Launch, String> {
        if !wire::identity_valid(identity)
            || receipt.schema != 1
            || !receipt.bootstrap_verified
            || receipt.engine_loaded
            || receipt.world_synchronized
            || receipt.career != self.manifest.career
            || receipt.manifest_sha256 != self.digest
            || receipt.save_sha256 != self.manifest.bootstrap.sha256
            || receipt.bytes != self.manifest.bootstrap.bytes
        {
            return Err("invalid admission import claim".into());
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "admission authority unavailable")?;
        state.leases.retain(|_, lease| now < lease.expires);
        if let Some(lease) = state.leases.get(identity) {
            let mut launch = lease.launch.clone();
            launch.valid_for_secs = lease.expires.duration_since(now).as_secs().max(1);
            return Ok(launch);
        }
        if state.leases.len() >= wire::MAX_PLAYERS {
            return Err("career admission capacity reached".into());
        }
        let launch = Launch {
            schema: 1,
            career: self.manifest.career.clone(),
            manifest_sha256: self.digest.clone(),
            identity: identity.into(),
            room: self.room.clone(),
            relay: self.relay,
            compatibility: self.manifest.compatibility.clone(),
            proof: Proof {
                ticket: identity::random_id()?,
            },
            valid_for_secs: LEASE_SECS,
        };
        state.leases.insert(
            identity.into(),
            Lease {
                launch: launch.clone(),
                expires: now + Duration::from_secs(LEASE_SECS),
            },
        );
        Ok(launch)
    }
    pub fn allows(&self, identity: &str, room: &str, profile: &str, proof: Option<&Proof>) -> bool {
        self.allows_at(identity, room, profile, proof, Instant::now())
    }
    fn allows_at(
        &self,
        identity: &str,
        room: &str,
        profile: &str,
        proof: Option<&Proof>,
        now: Instant,
    ) -> bool {
        let Some(proof) = proof.filter(|p| p.valid()) else {
            return false;
        };
        if room != self.room || profile != self.manifest.compatibility.profile {
            return false;
        }
        let Ok(state) = self.state.lock() else {
            return false;
        };
        let Some(lease) = state.leases.get(identity).filter(|l| now < l.expires) else {
            return false;
        };
        proof
            .ticket
            .bytes()
            .zip(lease.launch.proof.ticket.bytes())
            .fold(0u8, |a, (b, c)| a | (b ^ c))
            == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn launcher_rechecks_import_files_rules_content_and_actual_session_inputs() {
        let tmp = store::tests::Temp::new();
        let (a, receipt) = fixture();
        let id = "1".repeat(32);
        let launch = a.issue(&id, &receipt).unwrap();
        let import = tmp.0.join("import");
        std::fs::create_dir(&import).unwrap();
        store::write_new(&import.join("bootstrap.sav"), b"save").unwrap();
        store::write_new(
            &import.join("manifest.json"),
            &store::json(&a.manifest).unwrap(),
        )
        .unwrap();
        store::write_new(
            &import.join("receipt.json"),
            &store::json(&receipt).unwrap(),
        )
        .unwrap();
        let check =
            |l: &Launch| l.preflight(&import, &a.manifest.compatibility, &id, "career", a.relay);
        check(&launch).unwrap();
        for field in 0..4 {
            let mut bad = launch.clone();
            match field {
                0 => bad.identity = "2".repeat(32),
                1 => bad.room = "other".into(),
                2 => bad.relay = "127.0.0.1:39002".parse().unwrap(),
                _ => bad.manifest_sha256 = career::hash(b"other rules"),
            };
            assert!(check(&bad).is_err());
        }
        let mut local = a.manifest.compatibility.clone();
        local.map_sha256 = career::hash(b"other map");
        let mut tampered = launch.clone();
        tampered.compatibility = local.clone();
        assert!(
            tampered
                .preflight(&import, &local, &id, "career", a.relay)
                .is_err()
        );
        assert!(
            launch
                .preflight(&import, &local, &id, "career", a.relay)
                .is_err()
        );
        local = a.manifest.compatibility.clone();
        local.mods_sha256.push(career::hash(b"mod"));
        assert!(
            launch
                .preflight(&import, &local, &id, "career", a.relay)
                .is_err()
        );
        store::write_new(&import.join("INCOMPLETE"), b"pending").unwrap();
        assert!(check(&launch).is_err());
        std::fs::remove_file(import.join("INCOMPLETE")).unwrap();
        std::fs::write(import.join("bootstrap.sav"), b"tampered").unwrap();
        assert!(check(&launch).is_err());
        let missing = tmp.0.join("missing-identity");
        assert!(identity::load_existing(&missing).is_err());
        assert!(!missing.exists());
        let map = tmp.0.join("map");
        store::write_new(&map, b"map").unwrap();
        assert_eq!(fingerprint(&map).unwrap(), career::hash(b"map"));
        assert!(compatibility(wire::PROFILE, &map, &[map.clone(), map.clone()]).is_err());
        assert!(fingerprint(&tmp.0).is_err());
    }
    #[test]
    fn maximum_options_fit_existing_control_message_and_wire_without_admission_downgrade() {
        let proof = Proof {
            ticket: "a".repeat(32),
        };
        let options = crate::session_client::Options {
            room: "a".repeat(32),
            profile: "b".repeat(64),
            identity: "c".repeat(32),
            admission: Some(proof),
        };
        let json = serde_json::to_vec(&options).unwrap();
        assert!(json.len() < 256);
        let mut client = crate::session_client::Client::new(options, 1).unwrap();
        let frame = client.poll(0).0.remove(0);
        let encoded = wire::encode_client(&frame).unwrap();
        assert!(encoded.len() <= crate::protocol::MAX_PACKET);
        assert!(wire::decode_client(&encoded).is_some());
        let mut old = serde_json::to_value(&frame).unwrap();
        old["message"].as_object_mut().unwrap().remove("admission");
        assert!(wire::decode_client(&serde_json::to_vec(&old).unwrap()).is_some());
    }
    fn fixture() -> (Registry, Receipt) {
        let m = Manifest::new(
            career::Goal::Unlimited,
            Compatibility {
                profile: wire::PROFILE.into(),
                map_sha256: career::hash(b"map"),
                mods_sha256: vec![],
            },
            b"save",
        )
        .unwrap();
        let r = Receipt {
            schema: 1,
            career: m.career.clone(),
            manifest_sha256: manifest_hash(&m).unwrap(),
            save_sha256: m.bootstrap.sha256.clone(),
            bytes: 4,
            bootstrap_verified: true,
            engine_loaded: false,
            world_synchronized: false,
        };
        (
            Registry::new(m, "career".into(), "127.0.0.1:39000".parse().unwrap()).unwrap(),
            r,
        )
    }
    #[test]
    fn leases_bind_context_identity_expiry_and_restart_without_rotating_live_tokens() {
        let (a, r) = fixture();
        let id = "1".repeat(32);
        let now = Instant::now();
        let l = a.issue_at(&id, &r, now).unwrap();
        assert!(a.allows_at(&id, "career", wire::PROFILE, Some(&l.proof), now));
        assert_eq!(
            a.issue_at(&id, &r, now + Duration::from_secs(1))
                .unwrap()
                .proof,
            l.proof
        );
        for (identity, room, profile) in [
            ("2".repeat(32), "career", wire::PROFILE),
            (id.clone(), "other", wire::PROFILE),
            (id.clone(), "career", "other"),
        ] {
            assert!(!a.allows_at(&identity, room, profile, Some(&l.proof), now));
        }
        let mut forged = l.proof.clone();
        forged.ticket = "0".repeat(32);
        assert!(!a.allows_at(&id, "career", wire::PROFILE, Some(&forged), now));
        assert!(!a.allows_at(
            &id,
            "career",
            wire::PROFILE,
            Some(&l.proof),
            now + Duration::from_secs(LEASE_SECS)
        ));
        let restart = Registry::new(a.manifest.clone(), a.room.clone(), a.relay).unwrap();
        assert!(!restart.allows(&id, "career", wire::PROFILE, Some(&l.proof)));
        assert!(!format!("{l:?}").contains(&l.proof.ticket));
    }
    #[test]
    fn bounded_registry_refuses_bad_claims_and_recovers_expired_capacity() {
        let (a, r) = fixture();
        let now = Instant::now();
        let mut bad = r.clone();
        bad.save_sha256 = career::hash(b"bad");
        assert!(a.issue_at(&"1".repeat(32), &bad, now).is_err());
        for id in 0..wire::MAX_PLAYERS {
            a.issue_at(&format!("{id:032x}"), &r, now).unwrap();
        }
        assert!(a.issue_at(&"f".repeat(32), &r, now).is_err());
        a.issue_at(&"f".repeat(32), &r, now + Duration::from_secs(LEASE_SECS))
            .unwrap();
    }
}

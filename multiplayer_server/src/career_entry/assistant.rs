//! Local launch plan for a console/UI companion. Paths are local to ONE peer;
//! capabilities/identities/saves are referenced, never embedded or copied.
use super::{Launch, compatibility};
use crate::{career, career_store as store, identity};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub launch: PathBuf,
    pub import: PathBuf,
    pub identity_file: PathBuf,
    pub map: PathBuf,
    pub mods: Vec<PathBuf>,
}
#[derive(Debug, Serialize)]
pub struct Check {
    pub schema: u32,
    pub local_preflight_passed: bool,
    pub server_lease_checked: bool,
    pub engine_loaded: bool,
    pub world_synchronized: bool,
}
pub struct Prepared {
    pub paths: Plan,
    pub room: String,
    pub relay: SocketAddr,
    pub check: Check,
}
fn regular(path: &Path) -> Result<(), String> {
    if !fs::metadata(path).map_err(|e| e.to_string())?.is_file() {
        return Err("expected a regular plan/input file".into());
    }
    Ok(())
}
fn resolve(base: &Path, path: &Path, directory: bool) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("empty local plan path".into());
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    };
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if directory {
        if !path.is_dir() {
            return Err("import must be a directory".into());
        }
    } else {
        regular(&path)?;
    }
    Ok(path)
}
impl Plan {
    pub fn resolve_paths(&self, base: &Path) -> Result<Self, String> {
        if self.schema != 1 || self.mods.len() > career::MAX_MODS {
            return Err("invalid local launch plan schema/mod count".into());
        }
        Ok(Self {
            schema: 1,
            launch: resolve(base, &self.launch, false)?,
            import: resolve(base, &self.import, true)?,
            identity_file: resolve(base, &self.identity_file, false)?,
            map: resolve(base, &self.map, false)?,
            mods: self
                .mods
                .iter()
                .map(|p| resolve(base, p, false))
                .collect::<Result<_, _>>()?,
        })
    }
    fn prepare(&self, base: &Path, profile: &str) -> Result<Prepared, String> {
        let paths = self.resolve_paths(base)?;
        let launch: Launch = store::read_json(&paths.launch)?;
        let local = compatibility(profile, &paths.map, &paths.mods)?;
        let id = identity::load_existing(&paths.identity_file)?;
        launch.preflight(&paths.import, &local, &id, &launch.room, launch.relay)?;
        Ok(Prepared {
            paths,
            room: launch.room,
            relay: launch.relay,
            check: Check {
                schema: 1,
                local_preflight_passed: true,
                server_lease_checked: false,
                engine_loaded: false,
                world_synchronized: false,
            },
        })
    }
}
/// Refuse collisions BEFORE requesting a ticket or writing any capability.
pub fn new_destination(path: &Path) -> Result<PathBuf, String> {
    let leaf = path.file_name().ok_or("output filename required")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let destination = parent.canonicalize().map_err(|e| e.to_string())?.join(leaf);
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err("output already exists".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    Ok(destination)
}
/// Read-only: no identities, imported files, network operations or processes.
pub fn load(path: &Path, profile: &str) -> Result<Prepared, String> {
    regular(path)?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let plan: Plan = store::read_json(&path)?;
    plan.prepare(path.parent().ok_or("plan parent missing")?, profile)
}
/// Publish a NEW private plan only after verifying all referenced local bytes.
/// Relative paths in this API are relative to the future plan's parent.
pub fn write(path: &Path, plan: &Plan, profile: &str) -> Result<Check, String> {
    let destination = new_destination(path)?;
    let prepared = plan.prepare(destination.parent().ok_or("plan parent missing")?, profile)?;
    store::write_new(&destination, &store::json(&prepared.paths)?)?;
    Ok(prepared.check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        career_entry::{self, Receipt, Registry},
        session_protocol as wire,
    };
    fn fixture() -> (store::tests::Temp, Plan, String) {
        let t = store::tests::Temp::new();
        let root = &t.0;
        fs::write(root.join("map.scs"), b"own map fixture").unwrap();
        fs::write(root.join("mod.scs"), b"own mod fixture").unwrap();
        let compat = compatibility(
            wire::PROFILE,
            &root.join("map.scs"),
            &[root.join("mod.scs")],
        )
        .unwrap();
        let save = b"own frozen fixture";
        let manifest = career::Manifest::new(career::Goal::Unlimited, compat, save).unwrap();
        let receipt = Receipt {
            schema: 1,
            career: manifest.career.clone(),
            manifest_sha256: career_entry::manifest_hash(&manifest).unwrap(),
            save_sha256: manifest.bootstrap.sha256.clone(),
            bytes: save.len() as u64,
            bootstrap_verified: true,
            engine_loaded: false,
            world_synchronized: false,
        };
        fs::create_dir(root.join("import")).unwrap();
        for (name, bytes) in [
            ("manifest.json", store::json(&manifest).unwrap()),
            ("receipt.json", store::json(&receipt).unwrap()),
            ("bootstrap.sav", save.to_vec()),
        ] {
            store::write_new(&root.join("import").join(name), &bytes).unwrap();
        }
        let id = identity::load_or_create(&root.join("identity.json")).unwrap();
        let launch = Registry::new(
            manifest,
            "career".into(),
            "127.0.0.1:39000".parse().unwrap(),
        )
        .unwrap()
        .issue(&id, &receipt)
        .unwrap();
        let secret = launch.proof.ticket.clone();
        store::write_new(&root.join("launch.json"), &store::json(&launch).unwrap()).unwrap();
        let plan = Plan {
            schema: 1,
            launch: "launch.json".into(),
            import: "import".into(),
            identity_file: "identity.json".into(),
            map: "map.scs".into(),
            mods: vec!["mod.scs".into()],
        };
        (t, plan, secret)
    }
    #[test]
    fn plan_resolves_relative_to_its_file_verifies_and_exports_no_capability_or_save() {
        let (t, plan, secret) = fixture();
        let out = t.0.join("plan.json");
        let check = write(&out, &plan, wire::PROFILE).unwrap();
        assert!(
            check.local_preflight_passed && !check.server_lease_checked && !check.engine_loaded
        );
        let p = load(&out, wire::PROFILE).unwrap();
        assert_eq!(p.room, "career");
        assert!(p.paths.map.is_absolute());
        assert_eq!(p.relay, "127.0.0.1:39000".parse().unwrap());
        let encoded = fs::read_to_string(&out).unwrap();
        assert!(!encoded.contains(&secret) && !encoded.contains("own frozen fixture"));
        let report = serde_json::to_string(&p.check).unwrap();
        assert!(!report.contains(&secret) && !report.contains("identity.json"));
        assert!(write(&out, &plan, wire::PROFILE).is_err());
        assert_eq!(fs::read_to_string(out).unwrap(), encoded);
    }
    #[test]
    fn changed_map_mod_save_identity_and_incomplete_import_fail_without_new_output() {
        for file in [
            "map.scs",
            "mod.scs",
            "identity.json",
            "import/bootstrap.sav",
        ] {
            let (t, plan, _) = fixture();
            fs::write(t.0.join(file), b"changed own fixture").unwrap();
            let out = t.0.join("plan.json");
            assert!(write(&out, &plan, wire::PROFILE).is_err(), "{file}");
            assert!(!out.exists());
        }
        let (t, plan, _) = fixture();
        fs::write(t.0.join("import/INCOMPLETE"), b"").unwrap();
        assert!(write(&t.0.join("plan.json"), &plan, wire::PROFILE).is_err());
    }
    #[test]
    fn schema_paths_profile_and_output_aliases_fail_without_new_identities() {
        let (t, mut plan, _) = fixture();
        let out = t.0.join("plan.json");
        assert!(write(&out, &plan, "other-profile").is_err());
        assert!(!out.exists());
        plan.identity_file = "never-create-identity.json".into();
        assert!(write(&out, &plan, wire::PROFILE).is_err());
        assert!(!t.0.join("never-create-identity.json").exists());
        plan.schema = 2;
        assert!(write(&out, &plan, wire::PROFILE).is_err());
        assert!(new_destination(&t.0.join("missing").join("out.json")).is_err());
        let mut value = serde_json::to_value(plan).unwrap();
        value["ticket"] = "unwanted embedded secret".into();
        fs::write(&out, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(load(&out, wire::PROFILE).is_err());
        assert!(new_destination(&t.0.join(".").join("plan.json")).is_err());
    }
}

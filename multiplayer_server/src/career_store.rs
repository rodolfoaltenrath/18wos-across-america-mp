//! Frozen save-base plus immutable, hash-chained sidecar revisions. One OS lock
//! per directory, fsync before atomic no-replace publication. No game writes.
use crate::career::{self, Compatibility, Goal, MAX_METADATA, MAX_SAVE, Manifest};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("expected a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("file exceeds size limit".into());
    }
    Ok(bytes)
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_bounded(path, MAX_METADATA)?)
        .map_err(|_| "invalid bounded JSON".into())
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
pub fn sync_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn json<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    serde_json::to_vec_pretty(value).map_err(|e| e.to_string())
}
/// Dedicated fresh directory only. Failures leave evidence; callers must never
/// recursively delete a caller-supplied directory or pretend it is complete.
pub fn create(
    path: &Path,
    save: &Path,
    goal: Goal,
    compatibility: Compatibility,
) -> Result<Manifest, String> {
    let bytes = read_bounded(save, MAX_SAVE)?;
    let manifest = Manifest::new(goal, compatibility, &bytes)?;
    fs::create_dir(path).map_err(|e| e.to_string())?;
    write_new(&path.join("bootstrap.sav"), &bytes)?;
    write_new(&path.join("manifest.json"), &json(&manifest)?)?;
    sync_dir(path)?;
    Ok(manifest)
}
pub fn load(path: &Path) -> Result<(Manifest, Vec<u8>), String> {
    let manifest: Manifest = read_json(&path.join("manifest.json"))?;
    manifest.validate()?;
    let save = read_bounded(&path.join("bootstrap.sav"), MAX_SAVE)?;
    manifest.verify_save(&save)?;
    Ok((manifest, save))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision<T> {
    schema: u32,
    revision: u64,
    previous_sha256: String,
    state_sha256: String,
    state: T,
}
/// The lock handle lives as long as the store; OS releases it on process exit.
/// Persist failure poisons this writer: restart/recover before any other mutation.
pub struct Journal<T> {
    path: PathBuf,
    _lock: StoreLock,
    state: T,
    revision: u64,
    previous: String,
    poisoned: bool,
}
pub struct StoreLock(File);
impl StoreLock {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.join("career.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "career is already in use by another writer")?;
        Ok(Self(lock))
    }
}
impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
impl<T: Serialize + DeserializeOwned + Clone> Journal<T> {
    pub fn open(
        path: &Path,
        initial: T,
        validate: impl Fn(&T) -> Result<(), String>,
    ) -> Result<Self, String> {
        let lock = StoreLock::acquire(path)?;
        let mut files = Vec::new();
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(number) = name
                .strip_prefix("revision-")
                .and_then(|s| s.strip_suffix(".json"))
            {
                let value: u64 = number.parse().map_err(|_| "invalid revision filename")?;
                if name != format!("revision-{value:020}.json") {
                    return Err("noncanonical revision name".into());
                }
                files.push((value, entry.path()));
                if files.len() > 100_000 {
                    return Err("revision count exceeds limit".into());
                }
            }
        }
        files.sort_by_key(|f| f.0);
        let mut state = initial;
        let mut revision = 0;
        let mut previous = "0".repeat(64);
        for (number, file) in files {
            if number != revision + 1 {
                return Err("missing or reordered revision; refusing rollback".into());
            }
            let bytes = read_bounded(&file, MAX_METADATA)?;
            let record: Revision<T> =
                serde_json::from_slice(&bytes).map_err(|_| "corrupt committed revision")?;
            if record.schema != 1
                || record.revision != number
                || record.previous_sha256 != previous
                || record.state_sha256 != career::hash(&json(&record.state)?)
            {
                return Err("broken revision chain".into());
            }
            validate(&record.state)?;
            state = record.state;
            revision = number;
            previous = career::hash(&bytes);
        }
        validate(&state)?;
        Ok(Self {
            path: path.into(),
            _lock: lock,
            state,
            revision,
            previous,
            poisoned: false,
        })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn state(&self) -> &T {
        &self.state
    }
    pub fn commit(
        &mut self,
        state: T,
        validate: impl Fn(&T) -> Result<(), String>,
    ) -> Result<u64, String> {
        if self.poisoned {
            return Err("journal requires recovery after failed commit".into());
        }
        validate(&state)?;
        let revision = self.revision.checked_add(1).ok_or("revision exhausted")?;
        let bytes = json(&Revision {
            schema: 1,
            revision,
            previous_sha256: self.previous.clone(),
            state_sha256: career::hash(&json(&state)?),
            state: state.clone(),
        })?;
        if bytes.len() > MAX_METADATA {
            return Err("state exceeds metadata limit".into());
        }
        let temporary = self
            .path
            .join(format!("pending-{}.tmp", crate::identity::random_id()?));
        let destination = self.path.join(format!("revision-{revision:020}.json"));
        self.poisoned = true;
        write_new(&temporary, &bytes)?;
        // hard_link atomically publishes an already synced file and refuses an
        // existing name on both Windows and Unix. No replace/rename fallback.
        fs::hard_link(&temporary, destination).map_err(|e| e.to_string())?;
        sync_dir(&self.path)?;
        self.state = state;
        self.revision = revision;
        self.previous = career::hash(&bytes);
        fs::remove_file(temporary).map_err(|e| e.to_string())?;
        self.poisoned = false;
        Ok(revision)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub struct Temp(pub PathBuf);
    impl Temp {
        pub fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "18wos-career-{}",
                crate::identity::random_id().unwrap()
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn journal_lock_recovery_corruption_and_no_replace() {
        let dir = Temp::new();
        let validate = |n: &u64| {
            if *n <= 100 {
                Ok(())
            } else {
                Err("invalid".into())
            }
        };
        let mut j = Journal::open(&dir.0, 0u64, validate).unwrap();
        assert!(Journal::open(&dir.0, 0u64, validate).is_err());
        assert!(j.commit(101, validate).is_err());
        assert_eq!(j.revision(), 0);
        j.commit(10, validate).unwrap();
        j.commit(20, validate).unwrap();
        drop(j);
        let mut j = Journal::open(&dir.0, 0u64, validate).unwrap();
        assert_eq!(j.state, 20);
        assert_eq!(j.revision(), 2);
        write_new(
            &dir.0.join("revision-00000000000000000003.json"),
            b"corrupt",
        )
        .unwrap();
        assert!(j.commit(30, validate).is_err());
        assert!(j.commit(40, validate).is_err());
        drop(j);
        assert!(Journal::open(&dir.0, 0u64, validate).is_err());
    }
    #[test]
    fn frozen_save_is_separate_and_checks_corruption() {
        let dir = Temp::new();
        let input = dir.0.join("original.sav");
        write_new(&input, b"own fixture").unwrap();
        let dest = dir.0.join("career");
        let c = Compatibility {
            profile: crate::session_protocol::PROFILE.into(),
            map_sha256: career::hash(b"map"),
            mods_sha256: vec![],
        };
        create(&dest, &input, Goal::Unlimited, c.clone()).unwrap();
        load(&dest).unwrap();
        assert!(create(&dest, &input, Goal::Unlimited, c).is_err());
        fs::write(dest.join("bootstrap.sav"), b"tampered").unwrap();
        assert!(load(&dest).is_err());
        assert_eq!(fs::read(input).unwrap(), b"own fixture");
    }
}

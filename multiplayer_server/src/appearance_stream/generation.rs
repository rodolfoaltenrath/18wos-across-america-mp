//! Durable reservation of a NEW authority generation before serving appearance
//! RPCs. Does not persist event history, appearance state or the live sidecar.
use super::Pin;
use crate::{career, career_store::Journal};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema: u32,
    career_manifest_sha256: String,
    previous: Option<Pin>,
}
fn validate(state: &State, career_hash: &str) -> Result<(), String> {
    if state.schema != 1
        || state.career_manifest_sha256 != career_hash
        || !career::hash_valid(career_hash)
    {
        return Err("appearance generation journal career differs".into());
    }
    if let Some(pin) = &state.previous
        && (pin.career_manifest_sha256 != career_hash
            || pin.world_epoch == 0
            || pin.authority_generation == 0
            || !career::hash_valid(&pin.content_sha256)
            || !career::hash_valid(&pin.catalogue_sha256))
    {
        return Err("invalid appearance generation reservation".into());
    }
    Ok(())
}
pub struct Guard {
    _journal: Journal<State>,
}
impl Guard {
    /// The data directory is the existing PRIVATE career store. Holding this
    /// guard keeps the OS lock until the appearance service has shut down.
    /// A crash after publication consumes the generation; choose a newer one.
    pub fn reserve(data: &Path, pin: &Pin) -> Result<Self, String> {
        let next = State {
            schema: 1,
            career_manifest_sha256: pin.career_manifest_sha256.clone(),
            previous: Some(pin.clone()),
        };
        validate(&next, &pin.career_manifest_sha256)?;
        let path = data.join("appearance-generations");
        match fs::create_dir(&path) {
            Ok(()) => crate::career_store::sync_dir(data)?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("appearance generation directory reparse point refused".into());
                    }
                }
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err("appearance generation journal needs a regular directory".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
        let mut journal = Journal::open(
            &path,
            State {
                schema: 1,
                career_manifest_sha256: pin.career_manifest_sha256.clone(),
                previous: None,
            },
            |s| validate(s, &pin.career_manifest_sha256),
        )?;
        if let Some(previous) = &journal.state().previous
            && (pin.authority_generation <= previous.authority_generation
                || pin.world_epoch < previous.world_epoch)
        {
            return Err(format!(
                "appearance authority generation must exceed {}; world epoch must be at least {}",
                previous.authority_generation, previous.world_epoch
            ));
        }
        journal.commit(next, |s| validate(s, &pin.career_manifest_sha256))?;
        Ok(Self { _journal: journal })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        career_store::tests::Temp,
        world_join::{self as join, Context},
    };
    fn pin() -> Pin {
        let (m, c, a, _) = join::tests::fixture();
        Pin::new(
            &Context {
                career: &m,
                content: &c,
                catalogue: &a,
            },
            1,
            1,
        )
        .unwrap()
    }
    #[test]
    fn restart_requires_a_strictly_new_generation_and_never_rewinds_world_epoch() {
        let t = Temp::new();
        let mut pin = pin();
        let first = Guard::reserve(&t.0, &pin).unwrap();
        let mut newer = pin.clone();
        newer.authority_generation = 2;
        assert!(Guard::reserve(&t.0, &newer).is_err()); // lock survives startup
        drop(first);
        assert!(Guard::reserve(&t.0, &pin).is_err());
        drop(Guard::reserve(&t.0, &newer).unwrap());
        pin.authority_generation = 3;
        pin.world_epoch = 2;
        drop(Guard::reserve(&t.0, &pin).unwrap());
        newer.authority_generation = 4;
        assert!(Guard::reserve(&t.0, &newer).is_err());
        pin.authority_generation = 4;
        drop(Guard::reserve(&t.0, &pin).unwrap());
        assert!(Guard::reserve(&t.0, &pin).is_err());
    }
    #[test]
    fn corrupt_or_foreign_journal_refuses_and_ignores_an_unpublished_temporary() {
        let t = Temp::new();
        let mut pin = pin();
        drop(Guard::reserve(&t.0, &pin).unwrap());
        let path = t.0.join("appearance-generations");
        let committed = path.join("revision-00000000000000000001.json");
        let before = fs::read(&committed).unwrap();
        fs::write(path.join("pending-owned.tmp"), b"interrupted").unwrap();
        let mut foreign = pin.clone();
        foreign.career_manifest_sha256 = career::hash(b"foreign");
        foreign.authority_generation = 2;
        assert!(Guard::reserve(&t.0, &foreign).is_err());
        assert_eq!(fs::read(&committed).unwrap(), before);
        pin.authority_generation = 2;
        drop(Guard::reserve(&t.0, &pin).unwrap());
        fs::write(&committed, b"corrupt").unwrap();
        pin.authority_generation = 3;
        assert!(Guard::reserve(&t.0, &pin).is_err());
        assert!(!path.join("revision-00000000000000000003.json").exists());
        assert!(path.join("pending-owned.tmp").exists());
    }
    #[test]
    fn invalid_pin_and_directory_alias_never_create_a_reservation() {
        let t = Temp::new();
        let mut invalid = pin();
        invalid.authority_generation = 0;
        assert!(Guard::reserve(&t.0, &invalid).is_err());
        assert!(!t.0.join("appearance-generations").exists());
        #[cfg(unix)]
        {
            let other = Temp::new();
            std::os::unix::fs::symlink(&other.0, t.0.join("appearance-generations")).unwrap();
            assert!(Guard::reserve(&t.0, &pin()).is_err());
            assert_eq!(fs::read_dir(&other.0).unwrap().count(), 0);
        }
    }
}

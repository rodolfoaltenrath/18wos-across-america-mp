//! OS entropy and local identifier files. These identifiers are NOT accounts.
use crate::session_protocol::identity_valid;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub fn random_id() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| "OS randomness unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn random_nonce() -> Result<u64, String> {
    loop {
        let value = getrandom::u64().map_err(|_| "OS randomness unavailable")?;
        if value != 0 {
            return Ok(value);
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    schema: u32,
    identity: String,
}
fn read(path: &Path) -> Result<String, String> {
    let mut data = String::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(1025)
        .read_to_string(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > 1024 {
        return Err("identity file exceeds limit".into());
    }
    let stored: Stored = serde_json::from_str(&data).map_err(|_| "invalid identity file")?;
    if stored.schema != 1 || !identity_valid(&stored.identity) {
        return Err("invalid identity schema/value".into());
    }
    Ok(stored.identity)
}
pub fn load_or_create(path: &Path) -> Result<String, String> {
    if path.try_exists().map_err(|e| e.to_string())? {
        return read(path);
    }
    // Acquire entropy before reserving a new file; failure must not leave an
    // empty identifier. create_new still arbitrates concurrent creators.
    let stored = Stored {
        schema: 1,
        identity: random_id()?,
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(mut file) => {
            file.write_all(&serde_json::to_vec(&stored).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            Ok(stored.identity)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read(path),
        Err(error) => Err(error.to_string()),
    }
}

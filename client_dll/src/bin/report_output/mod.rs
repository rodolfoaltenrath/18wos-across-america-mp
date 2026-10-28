use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::Path,
};
/// Serialize before creating a new file. Never truncate an earlier capture.
pub fn emit(value: &Value, output: Option<&Path>) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    if let Some(path) = output {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("output must be a new file: {}", path.display()))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        eprintln!("Saved {}", path.display());
    } else {
        io::stdout().lock().write_all(&bytes)?;
    }
    Ok(())
}

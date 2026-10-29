//! Windows permits renaming a mapped executable, but not overwriting it.
//! Park only our normal build output so Cargo can refresh dev_env itself.
#[cfg(windows)]
use anyhow::{Context, Result};
#[cfg(windows)]
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(windows)]
pub struct RunningExecutable {
    original: PathBuf,
    parked: PathBuf,
}

#[cfg(windows)]
impl RunningExecutable {
    pub fn park(workspace: &Path) -> Result<Option<Self>> {
        let current = fs::canonicalize(std::env::current_exe()?)?;
        let original = workspace.join("target/x86_64-pc-windows-gnu/release/dev_env.exe");
        if fs::canonicalize(&original).ok().as_ref() != Some(&current) {
            return Ok(None);
        }
        let directory = workspace.join("target/dev-env-running");
        fs::create_dir_all(&directory)?;
        // Only stale files bearing this helper's exact generated-name pattern.
        // A live mapped executable refuses deletion; leave it alone.
        for entry in fs::read_dir(&directory)?.take(128).flatten() {
            if let Some(name) = entry.file_name().to_str()
                && runner_name(name)
                && entry
                    .file_type()
                    .is_ok_and(|t| t.is_file() && !t.is_symlink())
            {
                let _ = fs::remove_file(entry.path());
            }
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let parked = directory.join(format!("dev_env-{}-{nonce}.exe", std::process::id()));
        fs::rename(&original, &parked).context("parking the running dev_env before rebuilding it; if blocked by another tool, run a copy from target/analysis")?;
        println!(
            "Running helper parked under target/dev-env-running; Cargo can update dev_env.exe"
        );
        Ok(Some(Self { original, parked }))
    }
}

#[cfg(windows)]
impl Drop for RunningExecutable {
    fn drop(&mut self) {
        // create-only hard link: never replace a new executable from Cargo or
        // another build. Restore the original when a build stops before linking.
        if !self.original.exists()
            && let Err(error) = fs::hard_link(&self.parked, &self.original)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            eprintln!(
                "Could not restore dev_env.exe: {error}; runner retained at {}",
                self.parked.display()
            );
            return;
        }
        let _ = fs::remove_file(&self.parked);
    }
}

#[cfg(windows)]
fn runner_name(name: &str) -> bool {
    let Some(id) = name
        .strip_prefix("dev_env-")
        .and_then(|v| v.strip_suffix(".exe"))
    else {
        return false;
    };
    let Some((pid, nonce)) = id.split_once('-') else {
        return false;
    };
    !pid.is_empty()
        && !nonce.is_empty()
        && pid.bytes().chain(nonce.bytes()).all(|v| v.is_ascii_digit())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn build_failure_restores_original_without_overwriting_a_new_cargo_output() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("dev_env.exe");
        let parked = temp.path().join("old.exe");
        fs::write(&parked, b"old helper").unwrap();
        drop(RunningExecutable {
            original: original.clone(),
            parked: parked.clone(),
        });
        assert_eq!(fs::read(&original).unwrap(), b"old helper");
        fs::write(&parked, b"old helper").unwrap();
        fs::write(&original, b"new Cargo output").unwrap();
        drop(RunningExecutable {
            original: original.clone(),
            parked,
        });
        assert_eq!(fs::read(&original).unwrap(), b"new Cargo output");
    }
    #[test]
    fn stale_cleanup_names_are_narrow_and_only_target_the_helper() {
        assert!(runner_name("dev_env-123-456.exe"));
        for bad in [
            "dev_env.exe",
            "game.exe",
            "dev_env--.exe",
            "dev_env-123-x.exe",
            "dev_env-123-4-5.exe",
        ] {
            assert!(!runner_name(bad));
        }
    }
}

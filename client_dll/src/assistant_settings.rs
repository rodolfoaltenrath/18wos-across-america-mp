//! Local player choices, independent of career tickets, saves and server prefs.
use crate::{
    interpolation::{Config, Timebase},
    pose_capture::Selection,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
const CAP: usize = 4096;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema: u8,
    pub capture: Selection,
    pub presentation: Config,
    /// Opt-in candidate metadata only, saved separately from connection logs.
    #[serde(default, skip_serializing_if = "is_false")]
    pub appearance_diagnostics: bool,
}
fn is_false(value: &bool) -> bool {
    !value
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            capture: Selection::Drawn,
            presentation: Config::DEFAULT,
            appearance_diagnostics: false,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<()> {
        ensure!(self.schema == 1, "unsupported player settings schema");
        self.presentation.validate().map_err(anyhow::Error::msg)
    }
    /// Existing, already bounded budgets; names are choices, not quality claims.
    pub fn preset(&mut self, choice: u8) -> Result<()> {
        let (delay_ms, extrapolation_ms) = match choice {
            1 => (100, 200),
            2 => (50, 100),
            3 => (100, 0),
            _ => anyhow::bail!("unknown smoothing choice"),
        };
        self.presentation = Config {
            delay_ms,
            extrapolation_ms,
            timebase: Timebase::Arrival,
        };
        Ok(())
    }
    pub fn arguments(self, override_capture: Option<Selection>) -> Result<Vec<OsString>> {
        self.validate()?;
        Ok(vec![
            "--capture".into(),
            override_capture.unwrap_or(self.capture).argument().into(),
            "--interpolation-ms".into(),
            self.presentation.delay_ms.to_string().into(),
            "--extrapolation-ms".into(),
            self.presentation.extrapolation_ms.to_string().into(),
            "--timebase".into(),
            match self.presentation.timebase {
                Timebase::Arrival => "arrival",
                Timebase::Server => "server",
            }
            .into(),
        ])
    }
}
pub fn load(path: &Path) -> Result<Settings> {
    let bytes = match crate::local_files::read(path, CAP) {
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(Settings::default());
        }
        Err(error) => return Err(error),
    };
    let settings: Settings = serde_json::from_slice(&bytes)?;
    settings.validate()?;
    Ok(settings)
}
/// Sync a private new temporary file before replacing settings. This avoids a
/// partly written JSON on interruption, not a power-loss/parallel-writer proof.
pub fn save(path: &Path, settings: Settings) -> Result<()> {
    settings.validate()?;
    let bytes = serde_json::to_vec_pretty(&settings)?;
    ensure!(bytes.len() <= CAP, "player settings too large");
    match crate::local_files::metadata(path) {
        Ok(meta) => ensure!(meta.is_file(), "player settings must be a file"),
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
        Err(e) => return Err(e),
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("settings parent missing"))?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut owned = None;
    for _ in 0..16 {
        let tmp = parent.join(format!(
            ".mp-player-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().create_new(true).write(true).open(&tmp) {
            Ok(file) => {
                owned = Some((tmp, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let (tmp, mut file) =
        owned.ok_or_else(|| anyhow::anyhow!("settings temporary file unavailable"))?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_file_preserves_defaults_and_invalid_choices_do_not_modify_them() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("player.json");
        let mut s = load(&p).unwrap();
        assert_eq!(s, Settings::default());
        assert!(!p.exists());
        assert!(s.preset(4).is_err());
        assert_eq!(s, Settings::default());
        for n in 1..=3 {
            s.preset(n).unwrap();
            s.validate().unwrap();
        }
        s.schema = 2;
        assert!(save(&p, s).is_err());
        assert!(!p.exists());
    }
    #[test]
    fn updates_are_validated_and_capture_override_is_temporary() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("player.json");
        save(&p, Settings::default()).unwrap();
        let mut s = Settings {
            capture: Selection::GamePose,
            ..Settings::default()
        };
        s.preset(2).unwrap();
        save(&p, s).unwrap();
        assert_eq!(load(&p).unwrap(), s);
        let args = s.arguments(Some(Selection::Drawn)).unwrap();
        assert_eq!(args[1], "drawn");
        assert_eq!(args[3], "50");
        assert_eq!(load(&p).unwrap().capture, Selection::GamePose);
        assert_eq!(fs::read_dir(t.path()).unwrap().count(), 1);
        fs::write(&p, b"{\"schema\":1,\"private\":\"no\"}").unwrap();
        assert!(load(&p).is_err());
        fs::write(&p, vec![b' '; CAP + 1]).unwrap();
        assert!(load(&p).is_err());
    }
    #[test]
    fn legacy_settings_stay_byte_compatible_and_diagnostics_do_not_change_launcher_arguments() {
        let legacy=br#"{"schema":1,"capture":"drawn","presentation":{"delay_ms":100,"extrapolation_ms":200,"timebase":"arrival"}}"#;
        let original: Settings = serde_json::from_slice(legacy).unwrap();
        assert!(!original.appearance_diagnostics);
        assert_eq!(serde_json::to_vec(&original).unwrap(), legacy);
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("player.json");
        let mut enabled = original;
        enabled.appearance_diagnostics = true;
        save(&p, enabled).unwrap();
        assert!(load(&p).unwrap().appearance_diagnostics);
        assert_eq!(
            enabled.arguments(None).unwrap(),
            original.arguments(None).unwrap()
        );
    }
}

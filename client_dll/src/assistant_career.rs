//! Process-free assistant preparation. The launcher performs its own final
//! preflight/admission before touching a process; no ticket is put on argv.
use crate::lan_kit::Settings;
use anyhow::{Result, ensure};
use multiplayer_server::career_entry::assistant;
use std::{ffi::OsString, path::Path};

pub struct Career {
    prepared: assistant::Prepared,
}
impl Career {
    pub fn load(path: &Path) -> Result<Self> {
        Ok(Self {
            prepared: assistant::load(path, crate::profile::ID).map_err(anyhow::Error::msg)?,
        })
    }
    pub fn settings(&self) -> Settings {
        Settings {
            server: self.prepared.relay,
            room: self.prepared.room.clone(),
        }
    }
    pub fn check(&self) -> &assistant::Check {
        &self.prepared.check
    }
    pub fn start_arguments(&self, settings: &Settings) -> Result<Vec<OsString>> {
        ensure!(
            settings.server == self.prepared.relay && settings.room == self.prepared.room,
            "sala/servidor diferem do arquivo de carreira"
        );
        let p = &self.prepared.paths;
        let mut args = visual_arguments(settings, &p.identity_file)?;
        for (flag, path) in [
            ("--career-launch", &p.launch),
            ("--career-import", &p.import),
            ("--career-map", &p.map),
        ] {
            args.push(flag.into());
            args.push(path.as_os_str().into());
        }
        for path in &p.mods {
            args.push("--career-mod".into());
            args.push(path.as_os_str().into());
        }
        Ok(args)
    }
}
pub fn visual_arguments(settings: &Settings, identity: &Path) -> Result<Vec<OsString>> {
    settings.validate()?;
    ensure!(!identity.as_os_str().is_empty(), "identity path required");
    Ok(vec![
        "session-start".into(),
        "--server".into(),
        settings.server.to_string().into(),
        "--room".into(),
        settings.room.clone().into(),
        "--identity-file".into(),
        identity.as_os_str().into(),
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn career_arguments_bind_session_and_preserve_unicode_paths_as_separate_os_arguments() {
        let c = Career {
            prepared: assistant::Prepared {
                paths: assistant::Plan {
                    schema: 1,
                    launch: "own dir/João/launch.json".into(),
                    import: "own dir/import".into(),
                    identity_file: "own dir/identity.json".into(),
                    map: "own map.scs".into(),
                    mods: vec!["own mod.scs".into()],
                },
                room: "career".into(),
                relay: "127.0.0.1:39000".parse().unwrap(),
                check: assistant::Check {
                    schema: 1,
                    local_preflight_passed: true,
                    server_lease_checked: false,
                    engine_loaded: false,
                    world_synchronized: false,
                },
            },
        };
        let s = c.settings();
        let a = c.start_arguments(&s).unwrap();
        assert_eq!(
            a,
            vec![
                "session-start",
                "--server",
                "127.0.0.1:39000",
                "--room",
                "career",
                "--identity-file",
                "own dir/identity.json",
                "--career-launch",
                "own dir/João/launch.json",
                "--career-import",
                "own dir/import",
                "--career-map",
                "own map.scs",
                "--career-mod",
                "own mod.scs"
            ]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>()
        );
        let mut wrong = s;
        wrong.room = "other".into();
        assert!(c.start_arguments(&wrong).is_err());
        wrong = c.settings();
        wrong.server.set_port(39002);
        assert!(c.start_arguments(&wrong).is_err());
        assert!(!c.check().server_lease_checked);
    }
    #[test]
    fn visual_flow_does_not_add_career_flags_or_create_identity_files() {
        let t = tempfile::tempdir().unwrap();
        let id = t.path().join("new identity.json");
        let mut s = Settings {
            server: "127.0.0.1:39000".parse().unwrap(),
            room: "lan".into(),
        };
        let a = visual_arguments(&s, &id).unwrap();
        assert_eq!(a.len(), 7);
        assert!(!id.exists());
        assert!(!a.iter().any(|p| p.to_string_lossy().contains("career")));
        s.server.set_port(0);
        assert!(visual_arguments(&s, &id).is_err());
        assert!(Career::load(&t.path().join("missing-plan.json")).is_err());
        assert!(!t.path().join("missing-plan.json").exists());
    }
}

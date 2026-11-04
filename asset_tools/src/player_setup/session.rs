//! Local setup state. A pointer is published BEFORE mounting so reopening can
//! find the backup after an interruption. No background/game/network processes.
use super::*;
use std::fs::OpenOptions;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema: u8,
    root_sha256: String,
    component: Component,
    run: String,
}
#[derive(Serialize)]
pub struct State {
    pub component: Component,
    pub managed: bool,
    pub overlay_present: bool,
    pub stage: &'static str,
    pub restorable: bool,
    pub engine_validated: bool,
}
pub fn root_key(game: &Path) -> Result<String> {
    Ok(ui::hash(
        package::checked(game)?
            .canonicalize()?
            .to_string_lossy()
            .as_bytes(),
    ))
}
pub fn default_data(game: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA ausente")?);
    #[cfg(not(windows))]
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(base) => PathBuf::from(base),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME ausente")?).join(".local/state")
        }
    };
    Ok(base.join("18wos-mp-setup").join(&root_key(game)?[..16]))
}
fn pointer(data: &Path, component: Component) -> PathBuf {
    data.join(format!("current-{}.json", component.name()))
}
fn active(game: &Path, data: &Path, component: Component) -> Result<Option<PathBuf>> {
    let path = package::checked(&pointer(data, component))?;
    let bytes = match ui::read(&path, 4096) {
        Ok(bytes) => bytes,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let selected: Selection = serde_json::from_slice(&bytes)?;
    ensure!(
        selected.schema == 1
            && selected.root_sha256 == root_key(game)?
            && selected.component == component
            && multiplayer_server::session_protocol::identity_valid(&selected.run),
        "referencia local invalida; preserve os backups para inspecao"
    );
    let work = package::checked(&data.join(format!("run-{}", selected.run)))?;
    let (_, plan) = super::load(game, &work)?;
    ensure!(
        plan.component == component,
        "componente do preparo difere da referencia"
    );
    Ok(Some(work))
}
pub fn inspect(game: &Path, data: &Path) -> Result<Vec<State>> {
    let data = package::checked(data)?;
    [Component::Translation, Component::Menu]
        .into_iter()
        .map(|component| {
            let name = match component {
                Component::Translation => "aa/rust_mp_ptbr.scs",
                Component::Menu => "aa/rust_mp_ui.scs",
            };
            let overlay_present = game.join(name).try_exists()?;
            let Some(work) = active(game, &data, component)? else {
                return Ok(State {
                    component,
                    managed: false,
                    overlay_present,
                    stage: if overlay_present {
                        "unmanaged"
                    } else {
                        "absent"
                    },
                    restorable: false,
                    engine_validated: false,
                });
            };
            let status = super::status(game, &work)?;
            Ok(State {
                component,
                managed: true,
                overlay_present,
                stage: status.as_ref().map_or("prepared", |s| s.state),
                restorable: status.is_some_and(|s| s.restorable),
                engine_validated: false,
            })
        })
        .collect()
}
pub struct Session {
    game: PathBuf,
    data: PathBuf,
    _lock: File,
}
impl Session {
    pub fn open(game: &Path, data: &Path) -> Result<Self> {
        let game = package::checked(game)?.canonicalize()?;
        let data = package::checked(data)?;
        let mut dirs = fs::DirBuilder::new();
        dirs.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            dirs.mode(0o700);
        }
        dirs.create(&data)?;
        let lock_path = package::checked(&data.join("setup.lock"))?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path)?;
        lock.try_lock()
            .context("Outro instalador esta usando estes backups")?;
        Ok(Self {
            game,
            data,
            _lock: lock,
        })
    }
    fn select(&self, component: Component, run: &str) -> Result<()> {
        ensure!(
            multiplayer_server::session_protocol::identity_valid(run),
            "invalid local run"
        );
        let work = self.data.join(format!("run-{run}"));
        let (_, plan) = super::load(&self.game, &work)?;
        ensure!(plan.component == component, "component differs");
        let selected = Selection {
            schema: 1,
            root_sha256: root_key(&self.game)?,
            component,
            run: run.into(),
        };
        ui::write_new(
            &pointer(&self.data, component),
            &serde_json::to_vec_pretty(&selected)?,
        )
    }
    pub fn install(&self, component: Component, typeface: Option<&Path>) -> Result<mount::Status> {
        mount::ensure_game_closed()?;
        self.install_with(component, typeface, super::prepare, super::install)
    }
    fn install_with(
        &self,
        component: Component,
        typeface: Option<&Path>,
        prepare: impl FnOnce(&Path, Component, Option<&Path>, &Path) -> Result<Report>,
        install: impl Fn(&Path, &Path) -> Result<mount::Status>,
    ) -> Result<mount::Status> {
        if let Some(work) = active(&self.game, &self.data, component)? {
            match super::status(&self.game, &work)? {
                None => return install(&self.game, &work),
                Some(status) if status.state == "installed" => return Ok(status),
                Some(status) if status.state == "restored" => {
                    fs::remove_file(pointer(&self.data, component))?
                }
                Some(_) => anyhow::bail!(
                    "Ha uma instalacao interrompida ou alterada. Use Restaurar e preserve os backups; nada foi sobrescrito."
                ),
            }
        }
        let run = multiplayer_server::identity::random_id().map_err(anyhow::Error::msg)?;
        let work = self.data.join(format!("run-{run}"));
        prepare(&self.game, component, typeface, &work)?;
        self.select(component, &run)?;
        install(&self.game, &work)
    }
    pub fn restore(&self, component: Component) -> Result<mount::Status> {
        mount::ensure_game_closed()?;
        let work = active(&self.game, &self.data, component)?.context("Nao ha backup registrado por este assistente; preserve o backup do instalador anterior")?;
        let result = super::restore(&self.game, &work)?;
        ensure!(
            result.state == "restored",
            "restauracao ainda incompleta; referencia preservada"
        );
        fs::remove_file(pointer(&self.data, component))?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("game");
        fs::create_dir_all(game.join("aa/lib")).unwrap();
        for name in INPUTS {
            fs::write(game.join(name), b"own source").unwrap();
        }
        let data = t.path().join("private");
        (t, game, data)
    }
    #[test]
    fn inspect_is_read_only_and_missing_local_state_never_claims_an_overlay_is_absent() {
        let (_t, game, data) = fixture();
        let state = inspect(&game, &data).unwrap();
        assert!(state.iter().all(|s| s.stage == "absent" && !s.managed));
        assert!(!data.exists());
        fs::write(game.join("aa/rust_mp_ptbr.scs"), b"own existing overlay").unwrap();
        assert_eq!(inspect(&game, &data).unwrap()[0].stage, "unmanaged");
        assert!(!data.exists());
    }
    #[test]
    fn local_lock_and_prepared_reference_survive_reopening_without_game_writes() {
        let (_t, game, data) = fixture();
        let session = Session::open(&game, &data).unwrap();
        assert!(Session::open(&game, &data).is_err());
        let run = "a".repeat(32);
        let work = data.join(format!("run-{run}"));
        super::super::prepare_with(
            &game,
            &[],
            Component::Menu,
            None,
            &work,
            super::super::generate,
        )
        .unwrap();
        session.select(Component::Menu, &run).unwrap();
        assert!(session.select(Component::Menu, &run).is_err());
        drop(session);
        let session = Session::open(&game, &data).unwrap();
        let state = inspect(&game, &data).unwrap();
        assert_eq!(state[1].stage, "prepared");
        assert!(state[1].managed && !state[1].overlay_present);
        assert!(session.restore(Component::Menu).is_err()); // no backup, never hide a failed preparation
        assert!(pointer(&data, Component::Menu).is_file());
        assert_eq!(active(&game, &data, Component::Menu).unwrap(), Some(work));
    }
    #[test]
    fn corrupt_cross_component_and_traversing_references_are_never_followed() {
        let (t, game, data) = fixture();
        let session = Session::open(&game, &data).unwrap();
        let path = pointer(&data, Component::Menu);
        for value in [
            serde_json::json!({"schema":1,"root_sha256":root_key(&game).unwrap(),"component":"menu","run":"../outside"}),
            serde_json::json!({"schema":1,"root_sha256":root_key(&game).unwrap(),"component":"translation","run":"a".repeat(32)}),
            serde_json::json!({"schema":1,"root_sha256":"0".repeat(64),"component":"menu","run":"a".repeat(32)}),
        ] {
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(active(&game, &data, Component::Menu).is_err());
            assert!(session.restore(Component::Menu).is_err());
            assert!(path.exists());
        }
        assert!(!t.path().join("outside").exists());
        assert!(!game.join("aa/rust_mp_ui.scs").exists());
    }
    #[test]
    fn interrupted_selection_resumes_real_mount_and_restore_preserves_backups_and_user_changes() {
        let (_t, game, data) = fixture();
        let original = fs::read(game.join("aa/init.cfg")).unwrap();
        let expected = ui::hash(&fs::read(game.join("aa/lib/game.dll")).unwrap());
        let prepare = |root: &Path, component, _: Option<&Path>, work: &Path| {
            super::super::prepare_with(root, &[], component, None, work, super::super::generate)
        };
        let install = |root: &Path, work: &Path| {
            super::super::check_prepared(root, work)?;
            mount::install_owned_fixture(
                root,
                &work.join("package"),
                &work.join("backup"),
                &expected,
            )
        };
        let session = Session::open(&game, &data).unwrap();
        assert!(
            session
                .install_with(Component::Menu, None, prepare, |_, _| anyhow::bail!(
                    "owned interruption before mount"
                ))
                .is_err()
        );
        let work = active(&game, &data, Component::Menu).unwrap().unwrap();
        assert!(!work.join("backup").exists());
        assert_eq!(fs::read(game.join("aa/init.cfg")).unwrap(), original);
        drop(session);
        let session = Session::open(&game, &data).unwrap();
        let applied = session
            .install_with(
                Component::Menu,
                None,
                |_, _, _, _| panic!("must reuse completed preparation"),
                install,
            )
            .unwrap();
        assert_eq!(applied.state, "installed");
        assert_eq!(inspect(&game, &data).unwrap()[1].stage, "installed");
        assert!(work.join("backup/plan.json").is_file());
        let installed = fs::read(game.join("aa/init.cfg")).unwrap();
        fs::write(game.join("aa/init.cfg"), b"own user's change").unwrap();
        assert!(session.restore(Component::Menu).is_err());
        assert_eq!(
            fs::read(game.join("aa/init.cfg")).unwrap(),
            b"own user's change"
        );
        assert!(pointer(&data, Component::Menu).exists());
        fs::write(game.join("aa/init.cfg"), installed).unwrap();
        session.restore(Component::Menu).unwrap();
        assert_eq!(fs::read(game.join("aa/init.cfg")).unwrap(), original);
        assert!(!pointer(&data, Component::Menu).exists());
        assert!(!game.join("aa/rust_mp_ui.scs").exists());
        assert!(work.join("backup/plan.json").exists());
    }
}

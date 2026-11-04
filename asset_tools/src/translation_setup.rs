//! One-command PT-BR for a player's machine: the authored catalogues travel inside
//! the executable, the package is derived from that player's own game files and
//! stays on that machine. Original fonts keep their metrics; only accents are added.
use crate::{
    career_rules::{Goal, Language},
    localization::{self as l10n, Mounts, catalog, literals},
    mod_package,
    ui_tools::mount::{self, Overlay, Status},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const TABLE: &[u8] = include_bytes!("../../translations/pt-BR/local_def.json");
const LITERALS: &[u8] = include_bytes!("../../translations/pt-BR/literal_def.json");
/// Part and paint texts stay out until the opponent-truck crash of 03/11 is isolated.
pub const EXCLUDED_PREFIX: &str = "makes/";
pub const ARCHIVES: [&str; 3] = ["data.scs", "aa/base.scs", "aa/patch_1_10.scs"];

pub fn embedded() -> Result<(catalog::Catalog, literals::Catalog)> {
    let mut extra = literals::load(LITERALS)?;
    extra
        .files
        .retain(|path, _| !path.starts_with(EXCLUDED_PREFIX));
    Ok((catalog::load(TABLE)?, extra))
}
fn install_dirs(state: &Path) -> Result<BTreeMap<u32, PathBuf>> {
    let mut found = BTreeMap::new();
    if !state.try_exists()? {
        return Ok(found);
    }
    for entry in fs::read_dir(state)? {
        let entry = entry?;
        let number = entry.file_name().to_str().and_then(|n| n.parse().ok());
        if let (Some(number), true) = (number, entry.file_type()?.is_dir()) {
            found.insert(number, entry.path());
        }
    }
    Ok(found)
}
/// The newest attempt whose backup was written, whatever happened afterwards.
fn latest_backup(state: &Path) -> Result<Option<PathBuf>> {
    Ok(install_dirs(state)?
        .into_values()
        .rev()
        .map(|dir| dir.join("backup"))
        .find(|backup| backup.join("plan.json").is_file()))
}
#[derive(Debug, serde::Serialize)]
pub struct Outcome {
    pub work: PathBuf,
    pub translated_keys: usize,
    pub files: usize,
    pub skipped_fonts: Vec<String>,
    pub state: &'static str,
}
/// Refuses while a previous install from this state folder is still mounted.
fn run(
    game: &Path,
    state: &Path,
    table: &catalog::Catalog,
    extra: Option<&literals::Catalog>,
    install: impl FnOnce(&Path, &Path) -> Result<Status>,
) -> Result<Outcome> {
    if let Some(backup) = latest_backup(state)? {
        let current = mount::status(game, &backup);
        ensure!(
            !current.is_ok_and(|s| s.state == "installed" || s.state == "interrupted"),
            "a traducao ja esta instalada; use restore antes de instalar de novo"
        );
    }
    fs::create_dir_all(state).context("criando a pasta de dados da traducao")?;
    let next = install_dirs(state)?.into_keys().last().map_or(1, |n| n + 1);
    let work = state.join(next.to_string());
    fs::create_dir(&work)?;
    let archives = ARCHIVES.map(|a| game.join(a));
    let mounts = Mounts::open(&archives).context("arquivos do jogo nao encontrados nesta pasta")?;
    let project = work.join("project");
    let report = l10n::build_with_literals(&mounts, table, &project, None, extra)
        .context("esta instalacao difere da versao suportada pela traducao")?;
    let package = work.join("package");
    mod_package::build(&project.join("project.json"), &package, false)?;
    let status = install(&package, &work.join("backup")).context(
        "instalacao recusada; feche o jogo e confira se outro pacote esta montado em aa/init.cfg",
    )?;
    Ok(Outcome {
        work,
        translated_keys: report.catalog.translated,
        files: report.files,
        skipped_fonts: report.skipped_fonts,
        state: status.state,
    })
}
pub fn install(game: &Path, state: &Path) -> Result<Outcome> {
    let (table, extra) = embedded()?;
    run(game, state, &table, Some(&extra), |package, backup| {
        mount::install_overlay(game, package, backup, Overlay::PtBrText)
    })
}
pub fn status(game: &Path, state: &Path) -> Result<Option<Status>> {
    latest_backup(state)?
        .map(|backup| mount::status(game, &backup))
        .transpose()
}
fn installed(game: &Path, state: &Path) -> bool {
    status(game, state).is_ok_and(|s| s.is_some_and(|s| s.state == "installed"))
}
fn career_state(state: &Path) -> PathBuf {
    state.join("carreira")
}
pub fn career_installed(game: &Path, state: &Path) -> bool {
    installed(game, &career_state(state))
}
fn install_career(game: &Path, state: &Path, goal: Goal) -> Result<()> {
    let state = career_state(state);
    fs::create_dir_all(&state)?;
    let next = install_dirs(&state)?
        .into_keys()
        .last()
        .map_or(1, |n| n + 1);
    let work = state.join(next.to_string());
    fs::create_dir(&work)?;
    let mounts = Mounts::open(&ARCHIVES.map(|a| game.join(a)))?;
    let project = work.join("project");
    crate::career_rules::build(&mounts, goal, Language::PtBr, &project)?;
    let package = work.join("package");
    mod_package::build(&project.join("project.json"), &package, false)?;
    mount::install_overlay(game, &package, &work.join("backup"), Overlay::CareerRules)?;
    Ok(())
}
#[derive(Debug, serde::Serialize)]
pub struct Prepared {
    pub translation_installed_now: bool,
    pub career_installed_now: bool,
}
/// Installs only what is missing, translation first (p5) and career rules above
/// it (p6). `goal` is needed only when the career rules are not installed yet.
pub fn prepare(game: &Path, state: &Path, goal: Option<Goal>) -> Result<Prepared> {
    let (translation, career) = missing(installed(game, state), career_installed(game, state));
    if translation {
        install(game, state)?;
    }
    if career {
        install_career(game, state, goal.context("meta da carreira nao informada")?)?;
    }
    Ok(Prepared {
        translation_installed_now: translation,
        career_installed_now: career,
    })
}
/// What is left to install. A mounted career means the translation is below it,
/// even though its own record then no longer matches `init.cfg` (real cycle, 04/11).
fn missing(translation_recorded: bool, career: bool) -> (bool, bool) {
    (!translation_recorded && !career, !career)
}
/// Human state of both layers, for the same reason as `missing`.
pub fn summary(game: &Path, state: &Path) -> (bool, bool) {
    let career = career_installed(game, state);
    (career || installed(game, state), career)
}
/// Removes in reverse installation order: career rules, then translation.
pub fn restore(game: &Path, state: &Path) -> Result<Status> {
    let career = career_state(state);
    if let Some(backup) = latest_backup(&career)?
        && mount::status(game, &backup)?.state != "restored"
    {
        mount::restore(game, &backup)?;
    }
    let backup = latest_backup(state)?.context("nenhuma instalacao encontrada nesta pasta")?;
    mount::restore(game, &backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::catalog::{Entry, LANGUAGE, source_hash};
    use std::io::Write;
    fn game(root: &Path) {
        fs::create_dir_all(root.join("aa/lib")).unwrap();
        fs::write(root.join("aa/lib/game.dll"), b"own module fixture").unwrap();
        fs::write(root.join("aa/init.cfg"), b"# own config\n").unwrap();
        for archive in ARCHIVES {
            let mut z = zip::ZipWriter::new(fs::File::create(root.join(archive)).unwrap());
            if archive == "aa/base.scs" {
                z.start_file(l10n::TABLE, zip::write::SimpleFileOptions::default())
                    .unwrap();
                z.write_all(b"def translate {\nown_key:\"Own text\"\n}\n")
                    .unwrap();
            }
            z.finish().unwrap();
        }
    }
    fn table(source: &str) -> catalog::Catalog {
        catalog::Catalog {
            schema: 1,
            language: LANGUAGE.into(),
            source: l10n::TABLE.into(),
            entries: BTreeMap::from([(
                "own_key".to_string(),
                Entry {
                    src: source_hash(source),
                    text: "Próprio texto".into(),
                },
            )]),
        }
    }
    #[test]
    fn install_derives_from_local_files_refuses_repeats_and_restores() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("game");
        game(&root);
        let state = t.path().join("state");
        let original = fs::read(root.join("aa/init.cfg")).unwrap();
        let fixture = |package: &Path, backup: &Path| {
            mount::install_for_test(&root, package, backup, b"own module fixture")
        };
        let done = run(&root, &state, &table("Own text"), None, fixture).unwrap();
        assert_eq!((done.state, done.translated_keys), ("installed", 1));
        assert!(done.work.ends_with("1"));
        assert!(root.join("aa/rust_mp_ptbr.scs").exists());
        assert_eq!(status(&root, &state).unwrap().unwrap().state, "installed");
        assert!(run(&root, &state, &table("Own text"), None, fixture).is_err());

        let restored = mount::restore_for_test(&root, &state.join("1/backup")).unwrap();
        assert_eq!(restored.state, "restored");
        assert_eq!(fs::read(root.join("aa/init.cfg")).unwrap(), original);
        let again = run(&root, &state, &table("Own text"), None, fixture).unwrap();
        assert!(again.work.ends_with("2"));
        assert_eq!(status(&root, &state).unwrap().unwrap().state, "installed");
    }
    #[test]
    fn another_game_version_stops_before_any_installation_write() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("game");
        game(&root);
        let state = t.path().join("state");
        let original = fs::read(root.join("aa/init.cfg")).unwrap();
        let error = run(&root, &state, &table("Changed upstream"), None, |_, _| {
            panic!("must not reach the installer")
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("versao suportada"));
        assert_eq!(fs::read(root.join("aa/init.cfg")).unwrap(), original);
        assert!(status(&root, &state).unwrap().is_none());
        assert!(restore(&root, &state).is_err());
    }
    #[test]
    fn a_mounted_career_counts_as_translation_present() {
        assert_eq!(missing(false, false), (true, true));
        assert_eq!(missing(true, false), (false, true));
        assert_eq!(missing(false, true), (false, false));
        assert_eq!(missing(true, true), (false, false));
    }
    #[test]
    fn embedded_catalogues_load_without_the_suspect_part_texts() {
        let (table, extra) = embedded().unwrap();
        assert_eq!(table.entries.len(), 484);
        assert!(extra.files.keys().any(|p| p.starts_with("defs/voices/")));
        assert!(!extra.files.keys().any(|p| p.starts_with(EXCLUDED_PREFIX)));
    }
}

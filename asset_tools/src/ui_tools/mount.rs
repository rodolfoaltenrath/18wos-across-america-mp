//! Reversible mount of our exact menu-preview or PT-BR text package. Game must
//! remain closed. Trusted local tree with no concurrent editors; restore refuses
//! unknown changes. With both mounted, restore in reverse installation order.
use super::{hash, read, write_new};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
const GAME_SHA: &str = "ccb327c2a5c2f7d01d74d94629cf65847ee1535ca9020b3a5fc5d412a8ffbc7d";
const CFG: &str = "aa/init.cfg";
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlay {
    UiPreview,
    /// Translated `scripts/local.def` plus extended fonts, one priority above the menu.
    PtBrText,
}
impl Overlay {
    fn kind(self) -> &'static str {
        match self {
            Self::UiPreview => "own-ui-preview",
            Self::PtBrText => "own-ptbr-text",
        }
    }
    fn from_kind(kind: &str) -> Option<Self> {
        [Self::UiPreview, Self::PtBrText]
            .into_iter()
            .find(|o| o.kind() == kind)
    }
    fn file(self) -> &'static str {
        match self {
            Self::UiPreview => "aa/rust_mp_ui.scs",
            Self::PtBrText => "aa/rust_mp_ptbr.scs",
        }
    }
    fn temp(self) -> &'static str {
        match self {
            Self::UiPreview => "aa/rust_mp_ui_init.tmp",
            Self::PtBrText => "aa/rust_mp_ptbr_init.tmp",
        }
    }
    fn marker(self) -> &'static str {
        match self {
            Self::UiPreview => "across-america-mp UI preview",
            Self::PtBrText => "across-america-mp PT-BR text",
        }
    }
    fn priority(self) -> &'static str {
        match self {
            Self::UiPreview => "p4",
            Self::PtBrText => "p5",
        }
    }
    fn line(self) -> String {
        format!(
            "\n# BEGIN {m}\nmount -o ro,{p} \"{f}\" /\n# END {m}\n",
            m = self.marker(),
            p = self.priority(),
            f = self.file()
        )
    }
    /// Accepts only the package this overlay was built for.
    fn accept(self, package: &Path) -> Result<crate::mod_package::Manifest> {
        match self {
            Self::UiPreview => {
                let m = crate::mod_package::verify(package, true)?;
                ui_preview(&m)?;
                Ok(m)
            }
            Self::PtBrText => {
                // Derived from local files by design: never a distributable package.
                let m = crate::mod_package::verify(package, false)?;
                ensure!(
                    m.id == "ptbr-text"
                        && m.files.iter().any(|f| f.input.path == "scripts/local.def")
                        && m.files.iter().all(|f| {
                            let p = &f.input.path;
                            p == "scripts/local.def"
                                || (p.starts_with("fonts/")
                                    && !p[6..].contains('/')
                                    && [".font", ".tga", ".shd"].iter().any(|e| p.ends_with(e)))
                        }),
                    "not the PT-BR text project"
                );
                Ok(m)
            }
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    kind: String,
    root: PathBuf,
    before: String,
    after: String,
    payload: String,
}
#[derive(Serialize)]
pub struct Status {
    pub kind: &'static str,
    pub state: &'static str,
    pub restorable: bool,
    pub original_config: bool,
    pub overlay_present: bool,
    pub preview_only: bool,
}
pub fn ensure_game_closed() -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_NO_MORE_FILES, GetLastError, INVALID_HANDLE_VALUE},
            System::Diagnostics::ToolHelp::*,
        };
        // SAFETY: snapshot/entry are owned and have the documented native size.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            ensure!(
                snapshot != INVALID_HANDLE_VALUE,
                "cannot enumerate processes; keep game closed"
            );
            struct Handle(windows_sys::Win32::Foundation::HANDLE);
            impl Drop for Handle {
                fn drop(&mut self) {
                    unsafe {
                        CloseHandle(self.0);
                    }
                }
            }
            let _handle = Handle(snapshot);
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            ensure!(
                Process32FirstW(snapshot, &mut entry) != 0,
                "cannot inspect process list"
            );
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                ensure!(
                    !name.eq_ignore_ascii_case("prism3d.exe"),
                    "close the game normally before changing UI files"
                );
                if Process32NextW(snapshot, &mut entry) == 0 {
                    ensure!(
                        GetLastError() == ERROR_NO_MORE_FILES,
                        "process enumeration incomplete"
                    );
                    break;
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
            {
                continue;
            }
            match fs::read(entry.path().join("comm")) {
                Ok(name) => ensure!(
                    !String::from_utf8_lossy(&name)
                        .trim()
                        .eq_ignore_ascii_case("prism3d.exe"),
                    "close the game normally before changing UI files"
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    anyhow::bail!("process guard not implemented for this host");
    Ok(())
}
fn checked_root(root: &Path) -> Result<PathBuf> {
    crate::mod_package::checked(root)?
        .canonicalize()
        .map_err(Into::into)
}
/// The backup's plan names which overlay it belongs to.
fn load(root: &Path, backup: &Path) -> Result<(PathBuf, Plan, Vec<u8>, Overlay)> {
    let root = checked_root(root)?;
    let backup = crate::mod_package::checked(backup)?;
    let p: Plan = serde_json::from_slice(&read(&backup.join("plan.json"), 65536)?)?;
    let overlay = Overlay::from_kind(&p.kind);
    ensure!(
        p.schema == 1 && overlay.is_some() && p.root == root,
        "backup belongs to another installation or schema"
    );
    let before = read(&backup.join("init.cfg.original"), 1024 * 1024)?;
    ensure!(hash(&before) == p.before, "backup checksum mismatch");
    Ok((root, p, before, overlay.expect("checked")))
}
fn matches_file(path: &Path, digest: &str, limit: usize) -> Result<bool> {
    if !path.try_exists()? {
        return Ok(false);
    }
    crate::mod_package::checked(path)?;
    Ok(hash(&read(path, limit)?) == digest)
}
pub fn status(root: &Path, backup: &Path) -> Result<Status> {
    let (root, p, _, kind) = load(root, backup)?;
    let cfg = root.join(CFG);
    let overlay = root.join(kind.file());
    let original = matches_file(&cfg, &p.before, 1024 * 1024)?;
    let applied = matches_file(&cfg, &p.after, 1024 * 1024)?;
    let exists = overlay.try_exists()?;
    let valid = !exists || matches_file(&overlay, &p.payload, 32 * 1024 * 1024 + 1024 * 1024)?;
    Ok(Status {
        kind: kind.kind(),
        state: if original && !exists {
            "restored"
        } else if applied && exists && valid {
            "installed"
        } else if (original || applied) && valid {
            "interrupted"
        } else {
            "conflict"
        },
        restorable: (original || applied) && valid,
        original_config: original,
        overlay_present: exists,
        preview_only: kind == Overlay::UiPreview,
    })
}
pub fn install(root: &Path, package: &Path, backup: &Path) -> Result<Status> {
    install_overlay(root, package, backup, Overlay::UiPreview)
}
pub fn install_overlay(
    root: &Path,
    package: &Path,
    backup: &Path,
    overlay: Overlay,
) -> Result<Status> {
    install_guarded(root, package, backup, GAME_SHA, ensure_game_closed, overlay)
}
fn ui_preview(m: &crate::mod_package::Manifest) -> Result<()> {
    ensure!(
        m.id == "mp-ui-foundation" && m.version == super::PACKAGE_VERSION && m.files.len() == 10,
        "not the native menu-preview project"
    );
    let mut main = false;
    for entry in &m.files {
        let path = &entry.input.path;
        let generated = if path == "ui/mmain.esc" {
            main = true;
            [mod_ui::locale::Language::PtBr, mod_ui::locale::Language::En]
                .iter()
                .any(|&l| hash(mod_ui::native::main_preview(l).as_bytes()) == entry.sha256)
        } else {
            mod_ui::flow::SCREENS.iter().any(|&screen| {
                path == &format!("ui/mp/{}.esc", mod_ui::native::name(screen))
                    && [mod_ui::locale::Language::PtBr, mod_ui::locale::Language::En]
                        .iter()
                        .any(|&language| {
                            hash(
                                mod_ui::native::screen(&mod_ui::flow::App::preview(
                                    screen, language,
                                ))
                                .as_bytes(),
                            ) == entry.sha256
                        })
            })
        };
        ensure!(
            generated,
            "package differs from our exact preview templates"
        );
    }
    ensure!(main, "native main-menu preview missing");
    Ok(())
}
fn install_guarded(
    root: &Path,
    package: &Path,
    backup: &Path,
    expected_game: &str,
    guard: impl Fn() -> Result<()>,
    overlay: Overlay,
) -> Result<Status> {
    guard()?;
    let root = checked_root(root)?;
    let package = crate::mod_package::checked(package)?;
    let backup = crate::mod_package::checked(backup)?;
    ensure!(
        hash(&read(&root.join("aa/lib/game.dll"), 16 * 1024 * 1024)?) == expected_game,
        "unsupported game profile"
    );
    let m = overlay.accept(&package)?;
    let original = read(&root.join(CFG), 1024 * 1024)?;
    let cfg = std::str::from_utf8(&original).context("init.cfg encoding unsupported")?;
    let priority = format!(",{}", overlay.priority());
    ensure!(
        !cfg.contains(overlay.marker())
            && !cfg
                .lines()
                .filter(|l| !l.trim().starts_with('#'))
                .any(|l| l.contains(&priority)),
        "overlay or priority {} already present",
        overlay.priority()
    );
    ensure!(
        !root.join(overlay.file()).try_exists()? && !root.join(overlay.temp()).try_exists()?,
        "overlay/temporary destination already exists"
    );
    let payload = read(&package.join("overlay.scs"), 33 * 1024 * 1024)?;
    ensure!(
        hash(&payload) == m.payload_sha256,
        "package changed during preparation"
    );
    let mut after = original.clone();
    after.extend_from_slice(overlay.line().as_bytes());
    let p = Plan {
        schema: 1,
        kind: overlay.kind().into(),
        root: root.clone(),
        before: hash(&original),
        after: hash(&after),
        payload: m.payload_sha256,
    };
    fs::create_dir(&backup).context("backup must be a NEW directory")?;
    write_new(&backup.join("init.cfg.original"), &original)?;
    write_new(&backup.join("plan.json"), &serde_json::to_vec_pretty(&p)?)?;
    // Backup is durable before any installation write. No originals/assets replaced.
    guard()?;
    ensure!(
        matches_file(&root.join(CFG), &p.before, 1024 * 1024)?,
        "configuration changed before install"
    );
    write_new(&root.join(overlay.file()), &payload)?;
    replace_cfg(&root, &after, &p.before, overlay)?;
    status(&root, &backup)
}
fn replace_cfg(root: &Path, bytes: &[u8], expected: &str, overlay: Overlay) -> Result<()> {
    let cfg = root.join(CFG);
    let temp = root.join(overlay.temp());
    let permissions = fs::metadata(&cfg)?.permissions();
    write_new(&temp, bytes)?;
    fs::set_permissions(&temp, permissions)?;
    ensure!(
        matches_file(&cfg, expected, 1024 * 1024)?,
        "configuration changed; refusing replace"
    );
    fs::rename(temp, cfg).context("replacing init.cfg; backup retained")?;
    Ok(())
}
pub fn restore(root: &Path, backup: &Path) -> Result<Status> {
    restore_guarded(root, backup, ensure_game_closed)
}
fn restore_guarded(root: &Path, backup: &Path, guard: impl Fn() -> Result<()>) -> Result<Status> {
    guard()?;
    let (root, p, before, kind) = load(root, backup)?;
    let s = status(&root, backup)?;
    ensure!(
        s.restorable,
        "files changed since install; refusing destructive restore"
    );
    let temp = root.join(kind.temp());
    if temp.try_exists()? {
        ensure!(
            matches_file(&temp, &p.before, 1024 * 1024)?
                || matches_file(&temp, &p.after, 1024 * 1024)?,
            "unknown temporary file; refusing cleanup"
        );
        fs::remove_file(&temp)?;
    }
    if !s.original_config {
        guard()?;
        replace_cfg(&root, &before, &p.after, kind)?;
    }
    let overlay = root.join(kind.file());
    if overlay.try_exists()? {
        guard()?;
        ensure!(
            matches_file(&overlay, &p.payload, 33 * 1024 * 1024)?,
            "overlay changed before removal"
        );
        fs::remove_file(overlay)?;
    }
    status(&root, backup)
}
#[cfg(test)]
mod tests {
    use super::*;
    const UI: &str = "aa/rust_mp_ui.scs";
    fn install_ui(
        root: &Path,
        package: &Path,
        backup: &Path,
        game: &str,
        guard: impl Fn() -> Result<()>,
    ) -> Result<Status> {
        install_guarded(root, package, backup, game, guard, Overlay::UiPreview)
    }
    /// Own PT-BR-shaped package: a table and one font file, nothing from the game.
    fn ptbr_package(t: &Path, id: &str) -> PathBuf {
        let project = t.join(format!("{id}-project"));
        for (path, bytes) in [
            (
                "scripts/local.def",
                b"def translate {\nown:\"Ol\xe1\"\n}\n".as_slice(),
            ),
            ("fonts/own.shd", b"UI(own.tobj,8,16,999)\n"),
        ] {
            let file = project.join("inputs/local").join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
        let json = format!(
            r#"{{"schema":1,"id":"{id}","version":"0.1.0","files":[
            {{"source":"inputs/local/scripts/local.def","path":"scripts/local.def","origin":"derived_local","depends_on":[]}},
            {{"source":"inputs/local/fonts/own.shd","path":"fonts/own.shd","origin":"derived_local","depends_on":[]}}]}}"#
        );
        fs::write(project.join("project.json"), json).unwrap();
        let pkg = t.join(format!("{id}-package"));
        crate::mod_package::build(&project.join("project.json"), &pkg, false).unwrap();
        pkg
    }
    #[test]
    fn ptbr_overlay_mounts_above_the_menu_and_restores_in_reverse_order() {
        let t = tempfile::tempdir().unwrap();
        let (root, ui_pkg) = fixture(t.path());
        let original = read(&root.join(CFG), 4096).unwrap();
        let game = hash(b"own module fixture");
        let pt = ptbr_package(t.path(), "ptbr-text");
        let wrong = ptbr_package(t.path(), "other-text");
        let ok = || Ok(());
        assert!(
            install_guarded(
                &root,
                &wrong,
                &t.path().join("b0"),
                &game,
                ok,
                Overlay::PtBrText
            )
            .is_err()
        );
        assert!(
            install_guarded(
                &root,
                &ui_pkg,
                &t.path().join("b0"),
                &game,
                ok,
                Overlay::PtBrText
            )
            .is_err()
        );
        assert!(!t.path().join("b0").exists());

        install_ui(&root, &ui_pkg, &t.path().join("b1"), &game, ok).unwrap();
        let s = install_guarded(
            &root,
            &pt,
            &t.path().join("b2"),
            &game,
            ok,
            Overlay::PtBrText,
        )
        .unwrap();
        assert_eq!(
            (s.kind, s.state, s.preview_only),
            ("own-ptbr-text", "installed", false)
        );
        let cfg = String::from_utf8(read(&root.join(CFG), 4096).unwrap()).unwrap();
        assert!(cfg.contains("mount -o ro,p4 \"aa/rust_mp_ui.scs\" /"));
        assert!(cfg.ends_with(
            "mount -o ro,p5 \"aa/rust_mp_ptbr.scs\" /\n# END across-america-mp PT-BR text\n"
        ));
        assert!(
            install_guarded(
                &root,
                &pt,
                &t.path().join("b3"),
                &game,
                ok,
                Overlay::PtBrText
            )
            .is_err()
        );

        // Out of order: the menu backup no longer matches; nothing is touched.
        assert!(restore_guarded(&root, &t.path().join("b1"), ok).is_err());
        assert_eq!(
            restore_guarded(&root, &t.path().join("b2"), ok)
                .unwrap()
                .state,
            "restored"
        );
        assert!(!root.join("aa/rust_mp_ptbr.scs").exists());
        assert_eq!(
            restore_guarded(&root, &t.path().join("b1"), ok)
                .unwrap()
                .state,
            "restored"
        );
        assert_eq!(read(&root.join(CFG), 4096).unwrap(), original);
    }
    fn fixture(t: &Path) -> (PathBuf, PathBuf) {
        let root = t.join("game");
        fs::create_dir_all(root.join("aa/lib")).unwrap();
        write_new(&root.join("aa/lib/game.dll"), b"own module fixture").unwrap();
        write_new(
            &root.join(CFG),
            b"# own config fixture\nmount -o ro,p2 base /\n",
        )
        .unwrap();
        let project = t.join("project");
        super::super::project(&project, true, mod_ui::locale::Language::PtBr).unwrap();
        let pkg = t.join("package");
        crate::mod_package::build(&project.join("project.json"), &pkg, true).unwrap();
        (root, pkg)
    }
    #[test]
    fn fixture_mount_restores_exact_config_and_rejects_open_game_before_writes() {
        let t = tempfile::tempdir().unwrap();
        let (root, pkg) = fixture(t.path());
        let backup = t.path().join("backup");
        let original = read(&root.join(CFG), 4096).unwrap();
        assert!(
            install_ui(
                &root,
                &pkg,
                &backup,
                &hash(b"own module fixture"),
                || anyhow::bail!("fixture game running")
            )
            .is_err()
        );
        assert!(!backup.exists());
        assert!(
            install_ui(
                &root,
                &pkg,
                &backup,
                &hash(b"own module fixture"),
                || Ok(())
            )
            .unwrap()
            .restorable
        );
        assert!(root.join(UI).exists());
        assert_eq!(
            restore_guarded(&root, &backup, || Ok(())).unwrap().state,
            "restored"
        );
        assert_eq!(read(&root.join(CFG), 4096).unwrap(), original);
        assert!(!root.join(UI).exists());
        assert_eq!(
            restore_guarded(&root, &backup, || Ok(())).unwrap().state,
            "restored"
        );
    }
    #[test]
    fn user_changes_and_wrong_installation_are_preserved() {
        let t = tempfile::tempdir().unwrap();
        let (root, pkg) = fixture(t.path());
        let backup = t.path().join("backup");
        install_ui(
            &root,
            &pkg,
            &backup,
            &hash(b"own module fixture"),
            || Ok(()),
        )
        .unwrap();
        fs::write(root.join(CFG), b"own later edit").unwrap();
        assert!(restore_guarded(&root, &backup, || Ok(())).is_err());
        assert_eq!(read(&root.join(CFG), 4096).unwrap(), b"own later edit");
        let other = t.path().join("other");
        fs::create_dir(&other).unwrap();
        assert!(restore_guarded(&other, &backup, || Ok(())).is_err());
    }
    #[test]
    fn interrupted_before_cfg_commit_can_remove_only_our_verified_overlay() {
        let t = tempfile::tempdir().unwrap();
        let (root, pkg) = fixture(t.path());
        let backup = t.path().join("backup");
        install_ui(
            &root,
            &pkg,
            &backup,
            &hash(b"own module fixture"),
            || Ok(()),
        )
        .unwrap();
        fs::write(
            root.join(CFG),
            read(&backup.join("init.cfg.original"), 4096).unwrap(),
        )
        .unwrap();
        assert_eq!(status(&root, &backup).unwrap().state, "interrupted");
        assert_eq!(
            restore_guarded(&root, &backup, || Ok(())).unwrap().state,
            "restored"
        );
    }
}

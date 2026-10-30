//! Reversible mount of our exact menu-preview package. Game must remain closed.
//! Trusted local tree with no concurrent editors; restore refuses unknown changes.
use super::{hash, read, write_new};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
const GAME_SHA: &str = "ccb327c2a5c2f7d01d74d94629cf65847ee1535ca9020b3a5fc5d412a8ffbc7d";
const CFG: &str = "aa/init.cfg";
const OVERLAY: &str = "aa/rust_mp_ui.scs";
const TEMP: &str = "aa/rust_mp_ui_init.tmp";
const LINE: &str = "\n# BEGIN across-america-mp UI preview\nmount -o ro,p4 \"aa/rust_mp_ui.scs\" /\n# END across-america-mp UI preview\n";
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
fn load(root: &Path, backup: &Path) -> Result<(PathBuf, Plan, Vec<u8>)> {
    let root = checked_root(root)?;
    let backup = crate::mod_package::checked(backup)?;
    let p: Plan = serde_json::from_slice(&read(&backup.join("plan.json"), 65536)?)?;
    ensure!(
        p.schema == 1 && p.kind == "own-ui-preview" && p.root == root,
        "backup belongs to another installation or schema"
    );
    let before = read(&backup.join("init.cfg.original"), 1024 * 1024)?;
    ensure!(hash(&before) == p.before, "backup checksum mismatch");
    Ok((root, p, before))
}
fn matches_file(path: &Path, digest: &str, limit: usize) -> Result<bool> {
    if !path.try_exists()? {
        return Ok(false);
    }
    crate::mod_package::checked(path)?;
    Ok(hash(&read(path, limit)?) == digest)
}
pub fn status(root: &Path, backup: &Path) -> Result<Status> {
    let (root, p, _) = load(root, backup)?;
    let cfg = root.join(CFG);
    let overlay = root.join(OVERLAY);
    let original = matches_file(&cfg, &p.before, 1024 * 1024)?;
    let applied = matches_file(&cfg, &p.after, 1024 * 1024)?;
    let exists = overlay.try_exists()?;
    let valid = !exists || matches_file(&overlay, &p.payload, 32 * 1024 * 1024 + 1024 * 1024)?;
    Ok(Status {
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
        preview_only: true,
    })
}
pub fn install(root: &Path, package: &Path, backup: &Path) -> Result<Status> {
    install_guarded(root, package, backup, GAME_SHA, ensure_game_closed)
}
fn install_guarded(
    root: &Path,
    package: &Path,
    backup: &Path,
    expected_game: &str,
    guard: impl Fn() -> Result<()>,
) -> Result<Status> {
    guard()?;
    let root = checked_root(root)?;
    let package = crate::mod_package::checked(package)?;
    let backup = crate::mod_package::checked(backup)?;
    ensure!(
        hash(&read(&root.join("aa/lib/game.dll"), 16 * 1024 * 1024)?) == expected_game,
        "unsupported game profile"
    );
    let m = crate::mod_package::verify(&package, true)?;
    ensure!(
        m.id == "mp-ui-foundation" && m.version == "0.1.0" && m.files.len() == 10,
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
    let original = read(&root.join(CFG), 1024 * 1024)?;
    let cfg = std::str::from_utf8(&original).context("init.cfg encoding unsupported")?;
    ensure!(
        !cfg.contains("across-america-mp UI preview")
            && !cfg
                .lines()
                .filter(|l| !l.trim().starts_with('#'))
                .any(|l| l.contains(",p4")),
        "UI preview or priority p4 already present"
    );
    ensure!(
        !root.join(OVERLAY).try_exists()? && !root.join(TEMP).try_exists()?,
        "preview/temporary destination already exists"
    );
    let payload = read(&package.join("overlay.scs"), 33 * 1024 * 1024)?;
    ensure!(
        hash(&payload) == m.payload_sha256,
        "package changed during preparation"
    );
    let mut after = original.clone();
    after.extend_from_slice(LINE.as_bytes());
    let p = Plan {
        schema: 1,
        kind: "own-ui-preview".into(),
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
    write_new(&root.join(OVERLAY), &payload)?;
    replace_cfg(&root, &after, &p.before)?;
    status(&root, &backup)
}
fn replace_cfg(root: &Path, bytes: &[u8], expected: &str) -> Result<()> {
    let cfg = root.join(CFG);
    let temp = root.join(TEMP);
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
    let (root, p, before) = load(root, backup)?;
    let s = status(&root, backup)?;
    ensure!(
        s.restorable,
        "files changed since install; refusing destructive restore"
    );
    let temp = root.join(TEMP);
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
        replace_cfg(&root, &before, &p.after)?;
    }
    let overlay = root.join(OVERLAY);
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
            install_guarded(
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
            install_guarded(
                &root,
                &pkg,
                &backup,
                &hash(b"own module fixture"),
                || Ok(())
            )
            .unwrap()
            .restorable
        );
        assert!(root.join(OVERLAY).exists());
        assert_eq!(
            restore_guarded(&root, &backup, || Ok(())).unwrap().state,
            "restored"
        );
        assert_eq!(read(&root.join(CFG), 4096).unwrap(), original);
        assert!(!root.join(OVERLAY).exists());
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
        install_guarded(
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
        install_guarded(
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

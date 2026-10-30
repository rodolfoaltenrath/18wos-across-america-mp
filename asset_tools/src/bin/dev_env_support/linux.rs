//! Full Linux + Windows x86 verification using one explicitly selected Proton
//! runtime and a prepared, workspace-local test prefix. Never launches the game.
use super::super::{bench_command, root, windows};
use anyhow::{Context, Result, ensure};
use clap::Args;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    steam: PathBuf,
    /// Exclusive TEST compatdata under this workspace's target; never the game.
    #[arg(long, default_value = "target/proton-tests")]
    compat: PathBuf,
    #[arg(long)]
    proton: PathBuf,
    #[arg(long)]
    runtime: PathBuf,
}
struct Prepared {
    steam: PathBuf,
    compat: PathBuf,
    proton: PathBuf,
    runtime: PathBuf,
    wineserver: PathBuf,
}
impl Prepared {
    fn new(workspace: &Path, options: Options) -> Result<Self> {
        let target = workspace
            .join("target")
            .canonicalize()
            .context("build the helper before verify")?;
        let compat = if options.compat.is_absolute() {
            options.compat
        } else {
            workspace.join(options.compat)
        }
        .canonicalize()
        .context("prepare the separate test prefix as documented")?;
        ensure!(
            compat != target && compat.starts_with(&target),
            "test compatdata must be strictly inside this workspace's target (symlinks resolved)"
        );
        ensure!(
            compat.join("pfx/drive_c").is_dir()
                && compat.join("pfx/user.reg").is_file()
                && compat.join("pfx/system.reg").is_file(),
            "select an initialized TEST prefix, not the game's compatdata"
        );
        // Prefix internals can be links too; do not let WINEPREFIX escape target.
        let pfx = compat.join("pfx").canonicalize()?;
        ensure!(
            pfx.starts_with(&compat),
            "test pfx escapes its compatdata directory"
        );
        let steam = options.steam.canonicalize()?;
        ensure!(
            steam.join("steamapps").is_dir(),
            "Steam root is missing steamapps"
        );
        let proton = options.proton.canonicalize()?;
        let runtime = options.runtime.canonicalize()?;
        let wineserver = proton
            .parent()
            .context("Proton parent missing")?
            .join("files/bin/wineserver");
        ensure!(
            proton.is_file() && runtime.join("_v2-entry-point").is_file() && wineserver.is_file(),
            "Proton/runtime entrypoints missing"
        );
        Ok(Self {
            steam,
            compat,
            proton,
            runtime,
            wineserver,
        })
    }
    fn stop_test_server(&self) -> Result<()> {
        ensure_no_game(Path::new("/proc"))?;
        let status = Command::new(&self.wineserver)
            .arg("-k")
            .env("WINEPREFIX", self.compat.join("pfx"))
            .status()?;
        // wineserver -k returns 1 when no server exists for this exact prefix.
        ensure!(
            matches!(status.code(), Some(0 | 1)),
            "cannot stop the exclusive test wineserver: {status}"
        );
        Ok(())
    }
    fn command(&self, workspace: &Path) -> Command {
        let mut command = Command::new(workspace.join("target/release/proton_host"));
        command
            .current_dir(workspace)
            .arg("--steam")
            .arg(&self.steam)
            .arg("--compat")
            .arg(&self.compat)
            .arg("--proton")
            .arg(&self.proton)
            .arg("--runtime")
            .arg(&self.runtime)
            .arg("--")
            .arg(workspace.join("target/i686-pc-windows-gnu/release/test_host.exe"))
            .arg(workspace.join("target/windows-tests.json"));
        command
    }
}
/// Advisory process guard, repeated before builds and before stopping a prefix.
/// It is not a global lock against someone opening the game later.
fn ensure_no_game(proc: &Path) -> Result<()> {
    for entry in fs::read_dir(proc)? {
        let entry = entry?;
        if !entry
            .file_name()
            .as_encoded_bytes()
            .iter()
            .all(u8::is_ascii_digit)
        {
            continue;
        }
        match fs::read_to_string(entry.path().join("comm")) {
            Ok(name) => ensure!(
                !name.trim().eq_ignore_ascii_case("prism3d.exe"),
                "close the game normally before rebuilding the process-pinned DLL"
            ),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
struct Lock {
    path: PathBuf,
    contents: Vec<u8>,
}
impl Lock {
    fn acquire(path: PathBuf) -> Result<Self> {
        use std::io::Write;
        let mut file=fs::File::create_new(&path).context("another verify may own target/verify-linux.lock; inspect it before removing a stale lock")?;
        let contents = format!("{}\n", std::process::id()).into_bytes();
        if let Err(error) = file.write_all(&contents) {
            let _ = fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(Self { path, contents })
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        if fs::read(&self.path).ok().as_ref() == Some(&self.contents) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn cargo(workspace: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(workspace)
        .status()?;
    ensure!(status.success(), "native Cargo step failed: {args:?}");
    Ok(())
}
fn config(workspace: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(workspace.join(".cargo/config.toml")) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn run(options: Options) -> Result<()> {
    let workspace = root().canonicalize()?;
    let prepared = Prepared::new(&workspace, options)?;
    let _lock = Lock::acquire(workspace.join("target/verify-linux.lock"))?;
    ensure_no_game(Path::new("/proc"))?;
    let before = config(&workspace)?;
    let inventory =
        asset_tools::source_audit::audit(&workspace, asset_tools::source_audit::Scope::Worktree)?;
    println!(
        "Source audit: {} own files, {} bytes",
        inventory.files.len(),
        inventory.total_bytes
    );
    windows(false, false, false, false)?;
    cargo(
        &workspace,
        &["build", "--workspace", "--release", "--locked"],
    )?;
    windows(false, false, true, false)?;
    prepared.stop_test_server()?;
    let x86 = bench_command(prepared.command(&workspace));
    // Cleanup even after a failed test; only the validated test prefix is used.
    let cleanup = prepared.stop_test_server();
    let x86 = x86?;
    cleanup?;
    let mut native = Command::new("cargo");
    native
        .args(["test", "--workspace", "--all-targets", "--locked"])
        .current_dir(&workspace);
    let native = bench_command(native)?;
    windows(false, false, false, true)?;
    cargo(
        &workspace,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    cargo(&workspace, &["fmt", "--all", "--", "--check"])?;
    ensure!(
        config(&workspace)? == before,
        "local Cargo configuration changed during verification"
    );
    println!(
        "verify: x86 {} passed under Proton, native {} passed, release builds refreshed, Clippy x86/native clean, rustfmt clean; game/LAN not tested",
        x86.0, native.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup(root: &Path) -> Options {
        for dir in [
            "target/test/pfx/drive_c",
            "steam/steamapps",
            "proton/files/bin",
            "runtime",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "target/test/pfx/user.reg",
            "target/test/pfx/system.reg",
            "proton/proton",
            "proton/files/bin/wineserver",
            "runtime/_v2-entry-point",
        ] {
            fs::write(root.join(file), b"fixture").unwrap();
        }
        Options {
            steam: root.join("steam"),
            compat: PathBuf::from("target/test"),
            proton: root.join("proton/proton"),
            runtime: root.join("runtime"),
        }
    }
    #[test]
    fn rejects_game_prefix_and_symlink_escape_before_any_command() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let options = setup(root);
        assert!(Prepared::new(root, options).is_ok());
        let mut options = setup(root);
        options.compat = root.join("steam/steamapps");
        assert!(Prepared::new(root, options).is_err());
        std::os::unix::fs::symlink(root.join("steam"), root.join("target/escape")).unwrap();
        let mut options = setup(root);
        options.compat = PathBuf::from("target/escape");
        assert!(Prepared::new(root, options).is_err());
        fs::remove_dir(root.join("target/test/pfx/drive_c")).unwrap();
        let options = Options {
            steam: root.join("steam"),
            compat: PathBuf::from("target/test"),
            proton: root.join("proton/proton"),
            runtime: root.join("runtime"),
        };
        assert!(Prepared::new(root, options).is_err());
    }
    #[test]
    fn rejects_nested_pfx_link_even_when_the_compatdata_itself_is_local() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let options = setup(root);
        fs::rename(root.join("target/test/pfx"), root.join("external-pfx")).unwrap();
        std::os::unix::fs::symlink(root.join("external-pfx"), root.join("target/test/pfx"))
            .unwrap();
        assert!(
            Prepared::new(root, options)
                .err()
                .unwrap()
                .to_string()
                .contains("pfx escapes")
        );
    }
    #[test]
    fn process_guard_refuses_running_game_and_ignores_nonprocess_entries() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::create_dir(root.join("123")).unwrap();
        fs::create_dir(root.join("self")).unwrap();
        fs::write(root.join("123/comm"), "engine_fixture\n").unwrap();
        assert!(ensure_no_game(root).is_ok());
        fs::write(root.join("123/comm"), "prism3d.exe\n").unwrap();
        assert!(ensure_no_game(root).is_err());
    }
    #[test]
    fn workspace_lock_is_exclusive_and_does_not_remove_replaced_owner() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lock");
        let lock = Lock::acquire(path.clone()).unwrap();
        assert!(Lock::acquire(path.clone()).is_err());
        drop(lock);
        let lock = Lock::acquire(path.clone()).unwrap();
        fs::write(&path, b"another owner").unwrap();
        drop(lock);
        assert_eq!(fs::read(path).unwrap(), b"another owner");
    }
}

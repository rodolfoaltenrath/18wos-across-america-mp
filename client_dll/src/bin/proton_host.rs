//! Native Linux entrypoint for our Windows tools in an explicit Proton environment.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use std::{ffi::OsString, path::PathBuf, process::Command};
#[derive(Parser)]
#[command(about = "Run a Windows tool in the game's explicit Steam Proton prefix")]
struct Args {
    #[arg(long)]
    steam: PathBuf,
    #[arg(long)]
    compat: PathBuf,
    #[arg(long)]
    proton: PathBuf,
    #[arg(long)]
    runtime: PathBuf,
    #[arg(last = true, required = true)]
    command: Vec<OsString>,
}
fn main() -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "proton_host is a native Linux tool"
    );
    let args = Args::parse();
    let steam = args.steam.canonicalize()?;
    let compat = args.compat.canonicalize()?;
    let proton = args.proton.canonicalize()?;
    let runtime = args.runtime.canonicalize()?.join("_v2-entry-point");
    ensure!(
        compat.join("pfx").is_dir(),
        "select an existing Proton compatdata directory"
    );
    ensure!(
        proton.is_file() && runtime.is_file(),
        "Proton/runtime entrypoint missing"
    );
    let executable = PathBuf::from(&args.command[0])
        .canonicalize()
        .context("Windows executable must exist")?;
    let status = Command::new(runtime)
        .args(["--verb=run", "--"])
        .arg(proton)
        .arg("runinprefix")
        .arg(executable)
        .args(&args.command[1..])
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam)
        .env("STEAM_COMPAT_DATA_PATH", compat)
        .env(
            "WINEDEBUG",
            std::env::var_os("WINEDEBUG").unwrap_or_else(|| "-all".into()),
        )
        .status()?;
    ensure!(status.success(), "Windows tool failed: {status}");
    Ok(())
}

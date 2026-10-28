//! Run every Cargo Windows test artifact in ONE Proton namespace/wineserver.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use std::{path::PathBuf, process::Command};
    let manifest = std::env::args_os()
        .nth(1)
        .context("usage: test_host.exe windows-tests.json")?;
    let paths: Vec<PathBuf> = serde_json::from_slice(&std::fs::read(manifest)?)?;
    ensure!(!paths.is_empty(), "empty test artifact manifest");
    for path in paths {
        let path = path.canonicalize()?;
        println!("Testing {}", path.display());
        ensure!(
            Command::new(&path).arg("--nocapture").status()?.success(),
            "test failed: {}",
            path.display()
        );
    }
    Ok(())
}
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("test_host runs Windows test artifacts; use the x86 build through proton_host");
    std::process::ExitCode::FAILURE
}

//! Run every Cargo Windows test artifact in ONE Proton namespace/wineserver.
#[cfg(windows)]
#[derive(clap::Parser)]
struct Args {
    manifest: std::path::PathBuf,
    /// Optional libtest name filter; default runs every test.
    #[arg(long)]
    filter: Option<String>,
    /// Explicit exclusions for supplemental benches with unavailable host tools.
    #[arg(long)]
    skip: Vec<String>,
}
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use clap::Parser;
    use std::{path::PathBuf, process::Command};
    let args = Args::parse();
    ensure!(args.skip.len() <= 16, "too many exclusions");
    for value in args.skip.iter().chain(args.filter.iter()) {
        ensure!(
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_:".contains(&b)),
            "use a Rust test name or module path"
        );
    }
    let paths: Vec<PathBuf> = serde_json::from_slice(&std::fs::read(args.manifest)?)?;
    ensure!(!paths.is_empty(), "empty test artifact manifest");
    for path in paths {
        let path = path.canonicalize()?;
        println!(
            "Testing {} (filter={:?}, skip={:?})",
            path.display(),
            args.filter,
            args.skip
        );
        let mut command = Command::new(&path);
        command.arg("--nocapture");
        for skip in &args.skip {
            command.args(["--skip", skip]);
        }
        if let Some(filter) = &args.filter {
            command.arg(filter);
        }
        ensure!(
            command.status()?.success(),
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

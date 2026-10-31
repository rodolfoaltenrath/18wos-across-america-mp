//! Third-party license inventory of the shipped crates, from the locked
//! dependency graph (offline). Fails when a declaration is missing or outside
//! the permissive allowlist; optionally writes the JSON and the notice text.
use anyhow::{Context, Result, ensure};
use asset_tools::licenses;
use clap::Parser;
use std::{io::Write, path::PathBuf, process::Command};
#[derive(Parser)]
struct Args {
    /// Workspace root (with Cargo.toml and Cargo.lock).
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
    /// Targets whose dependency graph is inspected (repeatable).
    #[arg(long = "target", default_values_t = ["i686-pc-windows-gnu".to_string()])]
    targets: Vec<String>,
    /// Workspace crates that produce the shipped binaries (repeatable).
    #[arg(long = "root", default_values_t = ["client_dll".to_string(), "multiplayer_server".to_string()])]
    roots: Vec<String>,
    /// NEW JSON inventory file.
    #[arg(long)]
    json: Option<PathBuf>,
    /// NEW third-party notice file (texts from the local registry sources).
    #[arg(long)]
    notice: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.targets.len() <= 8 && args.roots.len() <= 8,
        "too many targets/roots"
    );
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut documents = Vec::new();
    for target in &args.targets {
        let output = Command::new(&cargo)
            .current_dir(&args.workspace)
            .args(["metadata", "--format-version", "1", "--locked", "--offline"])
            .args(["--filter-platform", target])
            .output()
            .context("run cargo metadata")?;
        ensure!(
            output.status.success(),
            "cargo metadata failed for {target}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        documents.push((target.clone(), serde_json::from_slice(&output.stdout)?));
    }
    let roots: Vec<&str> = args.roots.iter().map(String::as_str).collect();
    let mut inventory = licenses::inventory(&documents, &roots)?;
    // Create outputs only as NEW files; never overwrite an earlier notice.
    let notice = licenses::notice(&mut inventory, "18wos-across-america-mp");
    if let Some(path) = &args.notice {
        std::fs::File::create_new(path)?.write_all(notice.as_bytes())?;
    }
    let json = serde_json::to_vec_pretty(&inventory)?;
    if let Some(path) = &args.json {
        std::fs::File::create_new(path)?.write_all(&json)?;
    }
    println!(
        "{} third-party crates for {:?}; violations: {}; without license file: {}",
        inventory.packages.len(),
        inventory.targets,
        inventory.violations.len(),
        inventory.missing_texts.len()
    );
    for v in &inventory.violations {
        println!("violation: {v}");
    }
    ensure!(inventory.passed, "license policy violations");
    Ok(())
}

//! Deterministic parser robustness run (own seeds/mutations only). Exit code
//! is non-zero when any target panicked or broke a round-trip property.
use anyhow::{Result, ensure};
use clap::Parser;
use std::{io::Write, path::PathBuf};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Inputs per target, seeds first, then mutations.
    #[arg(long, default_value_t = 20_000, value_parser = clap::value_parser!(u64).range(1..=100_000_000))]
    iterations: u64,
    /// A NEW report file; the parent folder must exist.
    #[arg(long)]
    output: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let report = client_dll::parser_fuzz::run(args.seed, args.iterations);
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(path) = &args.output {
        std::fs::File::create_new(path)?.write_all(&bytes)?;
    }
    std::io::stdout().write_all(&bytes)?;
    println!();
    ensure!(report.passed, "parser robustness properties failed");
    Ok(())
}

//! Native/offline inspection of a previously captured transition-report JSON.
mod report_output;
use anyhow::Result;
use clap::Parser;
use client_dll::transition_analysis::{self, MAX_CAPTURE_BYTES};
use std::{fs::File, io::Read, path::PathBuf};
#[derive(Parser)]
#[command(about = "Analyze a transition capture without opening a process or loading the DLL")]
struct Args {
    capture: PathBuf,
    /// Write a NEW analysis file; by default print JSON to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let mut bytes = Vec::new();
    File::open(&args.capture)?
        .take(MAX_CAPTURE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let analysis = transition_analysis::analyze(&bytes).map_err(anyhow::Error::msg)?;
    report_output::emit(&serde_json::to_value(analysis)?, args.output.as_deref())
}

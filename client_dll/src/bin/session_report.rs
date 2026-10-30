use anyhow::{Context, Result, ensure};
use clap::Parser;
use client_dll::session_evidence;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    about = "Offline session-status/stop analysis; explicit capture order from ONE peer, no game access"
)]
struct Args {
    #[arg(required = true, num_args = 1..)]
    reports: Vec<PathBuf>,
    /// NEW file; raw input paths, messages, poses and unknown fields are omitted.
    #[arg(long)]
    output: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.reports.len() <= session_evidence::MAX_INPUTS,
        "too many inputs"
    );
    let captures = args
        .reports
        .iter()
        .map(|path| {
            let meta = fs::metadata(path)?;
            ensure!(
                meta.is_file() && meta.len() <= session_evidence::MAX_INPUT_BYTES as u64,
                "invalid report size/type"
            );
            let mut bytes = Vec::new();
            File::open(path)?
                .take(session_evidence::MAX_INPUT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            session_evidence::parse(&bytes).with_context(|| format!("report {}", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    let report = session_evidence::analyze(captures)?;
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(path) = args.output {
        let mut file =
            File::create_new(path).context("output must be a NEW file with an existing parent")?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    println!("{}", String::from_utf8(bytes)?);
    Ok(())
}

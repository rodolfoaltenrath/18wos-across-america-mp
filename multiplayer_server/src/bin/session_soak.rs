use anyhow::{Result, ensure};
use clap::Parser;
use multiplayer_server::soak::{self, Config};
use std::{fs::File, io::Write, path::PathBuf};

#[derive(Parser)]
#[command(about = "Real-UDP loopback endurance; no game, public bind or identity files")]
struct Args {
    #[arg(long, default_value_t=16, value_parser=clap::value_parser!(u16).range(2..=64))]
    clients: u16,
    #[arg(long, default_value_t=300, value_parser=clap::value_parser!(u64).range(1..=3600))]
    seconds: u64,
    #[arg(long, value_parser=clap::value_parser!(u64).range(1..=3600))]
    restart_every_secs: Option<u64>,
    #[arg(long, default_value_t=3000, value_parser=clap::value_parser!(u64).range(0..=10000))]
    outage_ms: u64,
    #[arg(long, value_parser=clap::value_parser!(u64).range(1..=3600))]
    churn_every_secs: Option<u64>,
    /// Abort the server task on restarts: no final metrics exist for those epochs.
    #[arg(long, requires = "restart_every_secs")]
    abrupt_restarts: bool,
    /// New JSON file; existing parent required. Never overwrites a prior run.
    #[arg(long)]
    output: PathBuf,
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config {
        clients: usize::from(args.clients),
        duration_ms: args.seconds * 1000,
        restart_every_ms: args.restart_every_secs.map(|n| n * 1000),
        outage_ms: args.outage_ms,
        churn_every_ms: args.churn_every_secs.map(|n| n * 1000),
        abrupt_restarts: args.abrupt_restarts,
    };
    config.validate()?;
    let mut file = File::create_new(args.output)?;
    match soak::run(config).await {
        Ok(report) => {
            file.write_all(&serde_json::to_vec_pretty(&report)?)?;
            file.sync_all()?;
            println!(
                "{}",
                serde_json::json!({"event":"soak_complete","passed":report.passed,
                "elapsed_ms":report.elapsed_ms,"epochs":report.epochs.len(),"issues":report.issues})
            );
            ensure!(report.passed, "soak invariants failed; inspect the report");
            Ok(())
        }
        Err(error) => {
            file.write_all(&serde_json::to_vec_pretty(
                &serde_json::json!({"schema":1,"passed":false,
                "partial":true,"game_tested":false,"lan_tested":false,"error":error.to_string()}),
            )?)?;
            file.sync_all()?;
            Err(error)
        }
    }
}

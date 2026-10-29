//! Offline load coordination laboratory. Never opens or starts the game.
use anyhow::Result;
use clap::Parser;
#[derive(Parser)]
#[command(
    about = "Exercise owned load queue, cancellation, timeouts and recovery without game assets"
)]
struct Args {
    #[arg(long,default_value_t=1000,value_parser=clap::value_parser!(u32).range(1..=10000))]
    iterations: u32,
}
fn main() -> Result<()> {
    let report = client_dll::deferred_load::lab::run(Args::parse().iterations)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

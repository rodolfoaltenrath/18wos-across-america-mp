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
    /// Exercise own descriptor hooks and real cdecl fixture callbacks as well.
    #[arg(long)]
    adapter: bool,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let report = if args.adapter {
        serde_json::to_value(client_dll::deferred_load::adapter::lab::run(
            args.iterations,
        )?)?
    } else {
        serde_json::to_value(client_dll::deferred_load::lab::run(args.iterations)?)?
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

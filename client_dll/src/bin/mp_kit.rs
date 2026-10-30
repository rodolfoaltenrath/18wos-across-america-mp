use anyhow::Result;
use clap::Parser;
use client_dll::lan_kit::{self, Settings};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(
    about = "Create a NEW local Windows client kit from explicit x86 release binaries; no game assets"
)]
struct Args {
    #[arg(long)]
    build_dir: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Builder-supplied source revision; actual files also get SHA-256 hashes.
    #[arg(long)]
    revision: String,
    #[arg(long)]
    server: SocketAddr,
    #[arg(long, default_value = "lan")]
    room: String,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let manifest = lan_kit::create(
        &args.build_dir,
        &args.output,
        &args.revision,
        &Settings {
            server: args.server,
            room: args.room,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

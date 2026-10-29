//! Finite diagnostic UDP proxy; one instance per client, no game required.
use anyhow::Result;
use clap::Parser;
use client_dll::{
    network_conditions::Conditions,
    udp_proxy::{Controls, Options, Proxy},
};
use std::{
    io::{self, Write},
    net::SocketAddr,
    path::PathBuf,
};

#[derive(Parser)]
#[command(about = "Bounded two-way UDP impairment proxy for one v1 or v2 client")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:39001")]
    listen: SocketAddr,
    #[arg(long, default_value = "127.0.0.1:39000")]
    server: SocketAddr,
    /// Stop and report after this many milliseconds (at most one hour).
    #[arg(long, default_value_t = 10000, value_parser = clap::value_parser!(u64).range(1..=3600000))]
    duration_ms: u64,
    /// Milliseconds after the first valid client, not after proxy startup.
    #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u64).range(0..=3600000))]
    blackout_after_ms: u64,
    /// Disabled by default. Drop traffic both ways for this interval.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=3600000))]
    blackout_for_ms: u64,
    #[command(flatten)]
    conditions: Conditions,
    /// Reserve a NEW output file before accepting clients, write on normal stop.
    #[arg(long)]
    output: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let proxy = Proxy::bind(Options {
        listen: args.listen,
        server: args.server,
        duration_ms: args.duration_ms,
        blackout_after_ms: args.blackout_after_ms,
        blackout_for_ms: args.blackout_for_ms,
        conditions: args.conditions,
    })?;
    let mut output = args
        .output
        .as_ref()
        .map(std::fs::File::create_new)
        .transpose()?;
    println!(
        "{}",
        serde_json::json!({"event":"ready", "listen":proxy.local_addr()?, "server":args.server})
    );
    io::stdout().flush()?;
    let report = proxy.run(&Controls::default())?;
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(output) = &mut output {
        output.write_all(&bytes)?;
        output.flush()?;
    }
    io::stdout().write_all(&bytes)?;
    println!();
    Ok(())
}

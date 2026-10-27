use anyhow::Result;
use clap::Parser;
use multiplayer_server::{protocol::DEFAULT_SERVER, server};
use std::{net::SocketAddr, time::Duration};
use tokio::net::UdpSocket;

#[derive(Parser)]
#[command(about = "LAN coordinate relay at 60 Hz (prototype)")]
struct Args {
    #[arg(long, default_value = DEFAULT_SERVER)]
    bind: SocketAddr,
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u16).range(1..=64))]
    max_clients: u16,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let socket = UdpSocket::bind(args.bind).await?;
    println!(
        "UDP {} | 60 Hz | max {} clients | Ctrl+C to stop",
        socket.local_addr()?,
        args.max_clients
    );
    server::run(
        socket,
        usize::from(args.max_clients),
        Duration::from_secs(args.timeout_secs),
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    )
    .await
}

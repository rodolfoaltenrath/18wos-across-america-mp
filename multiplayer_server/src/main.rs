use anyhow::Result;
use clap::Parser;
use multiplayer_server::{
    protocol::DEFAULT_SERVER, server, session_hub::Settings, session_protocol, session_server,
};
use std::{io::Write, net::SocketAddr, path::PathBuf, time::Duration};
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
    /// Also send each client its own pose as player 1000000000+id (single-PC tests).
    #[arg(long)]
    echo: bool,
    /// Opt in to wire v2; this server creates one named room. Without it: v1.
    #[arg(long)]
    room: Option<String>,
    #[arg(long, default_value = session_protocol::PROFILE, requires="room")]
    profile: String,
    /// NEW JSON report file, written on Ctrl+C (v2 only).
    #[arg(long, requires = "room")]
    report: Option<PathBuf>,
    /// Optional finite run for automated tests; otherwise stop with Ctrl+C.
    #[arg(long, value_parser=clap::value_parser!(u64).range(1..=86400))]
    run_for_secs: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let shutdown = async {
        if let Some(seconds) = args.run_for_secs {
            tokio::select! { _ = tokio::time::sleep(Duration::from_secs(seconds)) => {},
            _ = tokio::signal::ctrl_c() => {} }
        } else {
            let _ = tokio::signal::ctrl_c().await;
        }
    };
    let socket = UdpSocket::bind(args.bind).await?;
    if let Some(room) = args.room {
        let settings = Settings {
            room,
            profile: args.profile,
            max_clients: usize::from(args.max_clients),
            timeout_ms: args
                .timeout_secs
                .checked_mul(1000)
                .ok_or_else(|| anyhow::anyhow!("timeout overflow"))?,
            echo: args.echo,
        };
        settings.validate().map_err(anyhow::Error::msg)?;
        let mut report_file = args
            .report
            .as_ref()
            .map(std::fs::File::create_new)
            .transpose()?;
        println!(
            "UDP {} | wire v2 | room {} | 60 Hz | Ctrl+C to stop",
            socket.local_addr()?,
            settings.room
        );
        let report = session_server::run(socket, settings, shutdown).await?;
        let bytes = serde_json::to_vec_pretty(&report)?;
        if let Some(file) = &mut report_file {
            file.write_all(&bytes)?;
            file.flush()?;
        }
        println!("{}", String::from_utf8(bytes)?);
        return Ok(());
    }
    println!(
        "UDP {} | 60 Hz | max {} clients | echo {} | Ctrl+C to stop",
        socket.local_addr()?,
        args.max_clients,
        args.echo
    );
    server::run_with_echo(
        socket,
        usize::from(args.max_clients),
        Duration::from_secs(args.timeout_secs),
        args.echo,
        shutdown,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_cli_remains_default_and_session_is_explicit() {
        assert!(
            Args::try_parse_from(["server", "--echo"])
                .unwrap()
                .room
                .is_none()
        );
        assert_eq!(
            Args::try_parse_from(["server", "--room", "lan"])
                .unwrap()
                .room
                .as_deref(),
            Some("lan")
        );
        assert!(Args::try_parse_from(["server", "--report", "unused.json"]).is_err());
    }
}

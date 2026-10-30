//! Career sidecar/bootstrap tooling. Intentionally independent of the game UI/DLL.
use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use multiplayer_server::{
    career::{self, Compatibility, Goal},
    career_net::{self, Invite},
    career_store as store, session_protocol,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Parser)]
#[command(about = "Carreira LAN e distribuição de save-base (sem sincronização do motor)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Args)]
struct Content {
    /// Exact map/archive fingerprint. Use the same archive role on all peers.
    #[arg(long)]
    map: PathBuf,
    /// Additional mod/archive content; order is normalized, duplicates rejected.
    #[arg(long = "mod")]
    mods: Vec<PathBuf>,
    #[arg(long,default_value=session_protocol::PROFILE)]
    profile: String,
}
impl Content {
    fn resolve(&self) -> Result<Compatibility> {
        ensure!(self.mods.len() <= career::MAX_MODS, "too many mods");
        let mut mods_sha256 = self
            .mods
            .iter()
            .map(|p| fingerprint(p))
            .collect::<Result<Vec<_>>>()?;
        mods_sha256.sort();
        let result = Compatibility {
            profile: self.profile.clone(),
            map_sha256: fingerprint(&self.map)?,
            mods_sha256,
        };
        result.validate().map_err(anyhow::Error::msg)?;
        Ok(result)
    }
}
#[derive(Subcommand)]
enum Command {
    /// Freeze a copy of a closed test save into a NEW sidecar folder.
    Create {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        save: PathBuf,
        #[command(flatten)]
        content: Content,
        /// Exact minor units in sidecar only; original game rules are not patched.
        #[arg(
            long,
            required_unless_present = "unlimited",
            conflicts_with = "unlimited"
        )]
        goal_minor: Option<u64>,
        #[arg(long)]
        unlimited: bool,
    },
    /// Serve the frozen save on an independent TCP control endpoint.
    Serve {
        #[arg(long)]
        data: PathBuf,
        #[arg(long, default_value = "127.0.0.1:39001")]
        bind: SocketAddr,
        #[arg(long,value_parser=clap::value_parser!(u64).range(1..=86400))]
        run_for_secs: Option<u64>,
    },
    /// Fetch/verify host save into a NEW staging folder. Does not load the game.
    Join {
        #[arg(long)]
        server: SocketAddr,
        /// Private invite.json copied out of band from the host, never an identity.
        #[arg(long)]
        invite: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        content: Content,
    },
    /// Verify a completed received save/manifest/receipt again, offline.
    VerifyImport {
        #[arg(long)]
        data: PathBuf,
    },
    /// Verify host frozen save and print the public manifest, never the secret.
    Inspect {
        #[arg(long)]
        data: PathBuf,
    },
}
fn fingerprint(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && before.len() > 0 && before.len() <= 4 * 1024 * 1024 * 1024u64,
        "invalid archive size/type"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(total <= before.len(), "archive changed during fingerprint");
        hash.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    ensure!(
        total == before.len()
            && before.len() == after.len()
            && before.modified()? == after.modified()?,
        "archive changed during fingerprint"
    );
    Ok(format!("{:x}", hash.finalize()))
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Create {
            data,
            save,
            content,
            goal_minor,
            unlimited: _,
        } => {
            let goal = match goal_minor {
                Some(minor_units) => Goal::Financial { minor_units },
                None => Goal::Unlimited,
            };
            let manifest = store::create(&data, &save, goal, content.resolve()?)
                .map_err(anyhow::Error::msg)?;
            let invite = Invite::new(&manifest)?;
            store::write_new(
                &data.join("invite.json"),
                &store::json(&invite).map_err(anyhow::Error::msg)?,
            )
            .map_err(anyhow::Error::msg)?;
            store::sync_dir(&data).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
            eprintln!(
                "Save-base preservado. Convite privado: invite.json. Regras declaradas no sidecar; motor ainda local."
            );
        }
        Command::Serve {
            data,
            bind,
            run_for_secs,
        } => {
            let listener = tokio::net::TcpListener::bind(bind).await?;
            eprintln!(
                "Carreira TCP {} (save-base; não altera horário/trânsito/física)",
                listener.local_addr()?
            );
            let report = career_net::serve(listener, &data, async {
                if let Some(seconds) = run_for_secs { tokio::select! { _ = tokio::time::sleep(Duration::from_secs(seconds)) => {}, _ = tokio::signal::ctrl_c() => {} } }
                else { let _ = tokio::signal::ctrl_c().await; }
            }).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Join {
            server,
            invite,
            output,
            content,
        } => {
            let invite: Invite = store::read_json(&invite).map_err(anyhow::Error::msg)?;
            let receipt = career_net::fetch(server, &invite, content.resolve()?, &output).await?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
            eprintln!(
                "Recebido em pasta separada. Ainda não carregado no jogo; mundo/física não sincronizados."
            );
        }
        Command::VerifyImport { data } => println!(
            "{}",
            serde_json::to_string_pretty(&career_net::inspect_import(&data)?)?
        ),
        Command::Inspect { data } => {
            let (manifest, _) = store::load(&data).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&manifest)?);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_goal_and_new_bootstrap_flow() {
        assert!(
            Cli::try_parse_from([
                "career", "create", "--data", "new", "--save", "test.sav", "--map", "data.scs"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "career",
                "create",
                "--data",
                "new",
                "--save",
                "test.sav",
                "--map",
                "data.scs",
                "--unlimited"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "career",
                "create",
                "--data",
                "new",
                "--save",
                "test.sav",
                "--map",
                "data.scs",
                "--unlimited",
                "--goal-minor",
                "100"
            ])
            .is_err()
        );
    }
}

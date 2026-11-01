//! Career sidecar/bootstrap tooling. Intentionally independent of the game UI/DLL.
use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use multiplayer_server::{
    career::{Compatibility, Goal},
    career_entry,
    career_net::{self, Invite},
    career_store as store,
    career_world::{Operation, WorldStore},
    session_protocol,
};
use std::{net::SocketAddr, path::PathBuf, time::Duration};

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
        career_entry::compatibility(&self.profile, &self.map, &self.mods)
            .map_err(anyhow::Error::msg)
    }
}
#[derive(Subcommand)]
enum Command {
    /// Prepare a declared multipart/material plan offline, without engine calls.
    AppearancePlan {
        #[arg(long)]
        content: PathBuf,
        #[arg(long)]
        catalogue: PathBuf,
        #[arg(long)]
        model: String,
        #[arg(long)]
        paint: String,
    },
    /// Offline sidecar/appearance snapshot barrier. Never marks the game ready.
    JoinCheck {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        content: PathBuf,
        #[arg(long)]
        catalogue: PathBuf,
        #[arg(long)]
        snapshot: PathBuf,
        /// Optional bounded JSON array of ordered appearance deltas.
        #[arg(long)]
        events: Option<PathBuf>,
        #[arg(long,value_parser=clap::value_parser!(u64).range(1..))]
        world_epoch: u64,
        #[arg(long,value_parser=clap::value_parser!(u64).range(1..))]
        authority_generation: u64,
    },
    /// Verify a declared appearance catalogue against content, without the game.
    CatalogueCheck {
        #[arg(long)]
        content: PathBuf,
        #[arg(long)]
        catalogue: PathBuf,
    },
    /// Offline catalogue/appearance check. Does not read or change the game.
    AppearanceCheck {
        #[arg(long)]
        content: PathBuf,
        #[arg(long)]
        catalogue: PathBuf,
        #[arg(long)]
        appearance: PathBuf,
    },
    /// Write a NEW private assistant plan from an existing ticket, offline.
    PlanLaunch {
        #[arg(long)]
        launch: PathBuf,
        #[arg(long)]
        import: PathBuf,
        #[arg(long)]
        identity_file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        content: Content,
    },
    /// Read-only local plan check; never contacts the server or opens the game.
    CheckPlan {
        #[arg(long)]
        plan: PathBuf,
        #[arg(long,default_value=session_protocol::PROFILE)]
        profile: String,
    },
    /// Read-only launcher checks without opening a process or creating identity files.
    CheckLaunch {
        #[arg(long)]
        launch: PathBuf,
        #[arg(long)]
        import: PathBuf,
        #[arg(long)]
        identity_file: PathBuf,
        #[arg(long)]
        room: String,
        #[arg(long)]
        relay: SocketAddr,
        #[command(flatten)]
        content: Content,
    },
    /// Career control and admission-guarded v2 relay with one lifetime.
    Host {
        #[arg(long)]
        data: PathBuf,
        #[arg(long, default_value = "127.0.0.1:39001")]
        tcp_bind: SocketAddr,
        #[arg(long, default_value = "127.0.0.1:39000")]
        udp_bind: SocketAddr,
        /// Reachable UDP endpoint advertised to clients (same port/family as bind).
        #[arg(long)]
        advertise: SocketAddr,
        #[arg(long)]
        room: String,
        #[arg(long,value_parser=clap::value_parser!(u64).range(1..=86400))]
        run_for_secs: Option<u64>,
    },
    /// Verify a staged import and obtain a PRIVATE one-hour launch ticket.
    PrepareLaunch {
        #[arg(long)]
        server: SocketAddr,
        #[arg(long)]
        invite: PathBuf,
        #[arg(long)]
        import: PathBuf,
        #[arg(long)]
        identity_file: PathBuf,
        /// New private JSON file; never copy to another player's machine.
        #[arg(long)]
        output: PathBuf,
        /// Optional NEW local plan for mp_lan --career-plan. Never share it.
        #[arg(long)]
        assistant_plan: Option<PathBuf>,
        #[command(flatten)]
        content: Content,
    },
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
    /// Read-only shared sidecar clock/ranking/contracts. No game writes.
    WorldStatus {
        #[arg(long)]
        server: SocketAddr,
        #[arg(long)]
        invite: PathBuf,
        #[command(flatten)]
        content: Content,
    },
    /// Local host administration. Refuses while another writer/server owns data.
    Apply {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        operation: PathBuf,
        /// Retain this exact ID when retrying after an uncertain result.
        #[arg(long)]
        operation_id: String,
    },
    /// Offline checkpoint/ranking query, with exclusive store ownership.
    World {
        #[arg(long)]
        data: PathBuf,
    },
    /// Actual TCP/store/contract recovery, using only synthetic owned fixtures.
    Lab {
        #[arg(long)]
        output: PathBuf,
        #[arg(long,default_value_t=100,value_parser=clap::value_parser!(u32).range(1..=1000))]
        iterations: u32,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::AppearancePlan {
            content,
            catalogue,
            model,
            paint,
        } => {
            let (content, catalogue) = load_catalogue(&content, &catalogue)?;
            let plan = multiplayer_server::vehicle_appearance::assembly::plan(
                &content, &catalogue, &model, &paint,
            )
            .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&plan)?);
        }
        Command::JoinCheck {
            data,
            content,
            catalogue,
            snapshot,
            events,
            world_epoch,
            authority_generation,
        } => {
            use multiplayer_server::world_join as join;
            let (career, _) = store::load(&data).map_err(anyhow::Error::msg)?;
            let (content, catalogue) = load_catalogue(&content, &catalogue)?;
            let snapshot = join::Snapshot::parse(
                &store::read_bounded(&snapshot, join::MAX_SNAPSHOT_JSON)
                    .map_err(anyhow::Error::msg)?,
            )
            .map_err(anyhow::Error::msg)?;
            let mut receiver = join::Receiver::new(
                join::Context {
                    career: &career,
                    content: &content,
                    catalogue: &catalogue,
                },
                world_epoch,
                authority_generation,
            )
            .map_err(anyhow::Error::msg)?;
            receiver.begin(snapshot).map_err(anyhow::Error::msg)?;
            if let Some(events) = events {
                let bytes = store::read_bounded(&events, join::MAX_EVENT_FILE)
                    .map_err(anyhow::Error::msg)?;
                let events: Vec<join::Delta> = serde_json::from_slice(&bytes)
                    .map_err(|_| anyhow::anyhow!("invalid bounded appearance event array"))?;
                ensure!(
                    events.len() <= join::MAX_PENDING,
                    "too many appearance events"
                );
                for event in events {
                    receiver.receive(event).map_err(anyhow::Error::msg)?;
                }
            }
            let prepared = receiver.prepare().map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(prepared.report())?);
        }
        Command::CatalogueCheck { content, catalogue } => {
            let (content, catalogue) = load_catalogue(&content, &catalogue)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &multiplayer_server::vehicle_appearance::inspect(&content, &catalogue)
                        .map_err(anyhow::Error::msg)?
                )?
            );
        }
        Command::AppearanceCheck {
            content,
            catalogue,
            appearance,
        } => {
            use multiplayer_server::vehicle_appearance as va;
            let (content, catalogue) = load_catalogue(&content, &catalogue)?;
            let description = va::Description::parse(
                &store::read_bounded(&appearance, va::MAX_DESCRIPTION_JSON)
                    .map_err(anyhow::Error::msg)?,
                &catalogue,
            )
            .map_err(anyhow::Error::msg)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &va::check(&content, &catalogue, &description).map_err(anyhow::Error::msg)?
                )?
            );
        }
        Command::PlanLaunch {
            launch,
            import,
            identity_file,
            output,
            content,
        } => {
            let check = write_assistant_plan(&output, launch, import, identity_file, &content)?;
            println!("{}", serde_json::to_string_pretty(&check)?);
        }
        Command::CheckPlan { plan, profile } => {
            let prepared =
                career_entry::assistant::load(&plan, &profile).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&prepared.check)?);
        }
        Command::CheckLaunch {
            launch,
            import,
            identity_file,
            room,
            relay,
            content,
        } => {
            let launch: career_entry::Launch =
                store::read_json(&launch).map_err(anyhow::Error::msg)?;
            let identity = multiplayer_server::identity::load_existing(&identity_file)
                .map_err(anyhow::Error::msg)?;
            launch
                .preflight(&import, &content.resolve()?, &identity, &room, relay)
                .map_err(anyhow::Error::msg)?;
            println!(
                "{}",
                serde_json::json!({"local_preflight_passed":true,"server_lease_checked":false,"engine_loaded":false,"world_synchronized":false})
            );
        }
        Command::Host {
            data,
            tcp_bind,
            udp_bind,
            advertise,
            room,
            run_for_secs,
        } => {
            let (manifest, _) = store::load(&data).map_err(anyhow::Error::msg)?;
            let settings = multiplayer_server::session_hub::Settings {
                room,
                profile: manifest.compatibility.profile,
                max_clients: 16,
                timeout_ms: 5000,
                echo: false,
            };
            let listener = tokio::net::TcpListener::bind(tcp_bind).await?;
            let socket = tokio::net::UdpSocket::bind(udp_bind).await?;
            eprintln!(
                "Carreira TCP {} / UDP {} (admissão; motor permanece local)",
                listener.local_addr()?,
                socket.local_addr()?
            );
            let report=career_net::serve_room(listener,socket,&data,settings,advertise,async {
                if let Some(seconds)=run_for_secs { tokio::select! { _=tokio::time::sleep(Duration::from_secs(seconds))=>{}, _=tokio::signal::ctrl_c()=>{} } }
                else { let _=tokio::signal::ctrl_c().await; }
            }).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::PrepareLaunch {
            server,
            invite,
            import,
            identity_file,
            output,
            assistant_plan,
            content,
        } => {
            let output =
                career_entry::assistant::new_destination(&output).map_err(anyhow::Error::msg)?;
            let plan_output = assistant_plan
                .as_deref()
                .map(career_entry::assistant::new_destination)
                .transpose()
                .map_err(anyhow::Error::msg)?;
            ensure!(
                plan_output.as_ref() != Some(&output),
                "launch and assistant plan must be different new files"
            );
            let local = content.resolve()?;
            career_net::inspect_import(&import)?;
            let invite: Invite = store::read_json(&invite).map_err(anyhow::Error::msg)?;
            let (manifest, _) = store::load(&import).map_err(anyhow::Error::msg)?;
            invite.check(&manifest)?;
            manifest
                .compatibility
                .compare(&local)
                .map_err(anyhow::Error::msg)?;
            let identity = multiplayer_server::identity::load_or_create(&identity_file)
                .map_err(anyhow::Error::msg)?;
            let launch =
                career_net::prepare_launch(server, &invite, local, &import, &identity).await?;
            store::write_new(&output, &store::json(&launch).map_err(anyhow::Error::msg)?)
                .map_err(anyhow::Error::msg)?;
            if let Some(path) = &plan_output {
                write_assistant_plan(path,output.clone(),import,identity_file,&content)
                    .map_err(|e|anyhow::anyhow!("launch saved; assistant plan not published: {e:#}; recover with plan-launch"))?;
            }
            println!(
                "{}",
                serde_json::json!({"prepared":true,"assistant_plan_written":plan_output.is_some(),"valid_for_secs":launch.valid_for_secs,"engine_loaded":false,"world_synchronized":false})
            );
        }
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
        Command::WorldStatus {
            server,
            invite,
            content,
        } => {
            let invite: Invite = store::read_json(&invite).map_err(anyhow::Error::msg)?;
            let snapshot = career_net::world_status(server, &invite, content.resolve()?).await?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        }
        Command::Apply {
            data,
            operation,
            operation_id,
        } => {
            let operation: Operation = store::read_json(&operation).map_err(anyhow::Error::msg)?;
            let mut world = WorldStore::open(&data).map_err(anyhow::Error::msg)?;
            let receipt = world
                .execute(&operation_id, &operation)
                .map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&receipt)?);
        }
        Command::World { data } => {
            let world = WorldStore::open(&data).map_err(anyhow::Error::msg)?;
            println!("{}", serde_json::to_string_pretty(&world.snapshot())?);
        }
        Command::Lab { output, iterations } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &multiplayer_server::career_lab::run(&output, iterations).await?
                )?
            );
        }
    }
    Ok(())
}
fn load_catalogue(
    content: &std::path::Path,
    catalogue: &std::path::Path,
) -> Result<(
    multiplayer_server::world_content::Manifest,
    multiplayer_server::vehicle_appearance::Catalogue,
)> {
    use multiplayer_server::{vehicle_appearance as va, world_content as wc};
    let content = wc::Manifest::parse(
        &store::read_bounded(content, wc::MAX_JSON).map_err(anyhow::Error::msg)?,
    )
    .map_err(anyhow::Error::msg)?;
    let catalogue = va::Catalogue::parse(
        &store::read_bounded(catalogue, va::MAX_CATALOGUE_JSON).map_err(anyhow::Error::msg)?,
        &content,
    )
    .map_err(anyhow::Error::msg)?;
    Ok((content, catalogue))
}
fn write_assistant_plan(
    output: &std::path::Path,
    launch: PathBuf,
    import: PathBuf,
    identity_file: PathBuf,
    content: &Content,
) -> Result<career_entry::assistant::Check> {
    let plan = career_entry::assistant::Plan {
        schema: 1,
        launch,
        import,
        identity_file,
        map: content.map.clone(),
        mods: content.mods.clone(),
    }
    .resolve_paths(&std::env::current_dir()?)
    .map_err(anyhow::Error::msg)?;
    career_entry::assistant::write(output, &plan, &content.profile).map_err(anyhow::Error::msg)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_plan_commands_require_local_inputs_and_keep_offline_check_separate() {
        assert!(Cli::try_parse_from(["career", "check-plan", "--plan", "own.json"]).is_ok());
        assert!(Cli::try_parse_from(["career", "plan-launch", "--launch", "own.json"]).is_err());
        assert!(
            Cli::try_parse_from([
                "career",
                "plan-launch",
                "--launch",
                "ticket.json",
                "--import",
                "own-import",
                "--identity-file",
                "own-id.json",
                "--map",
                "own.scs",
                "--output",
                "new-plan.json"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "career",
                "prepare-launch",
                "--server",
                "127.0.0.1:39001",
                "--invite",
                "own.json",
                "--import",
                "own-import",
                "--identity-file",
                "own-id.json",
                "--map",
                "own.scs",
                "--output",
                "ticket.json",
                "--assistant-plan",
                "new-plan.json"
            ])
            .is_ok()
        );
    }
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

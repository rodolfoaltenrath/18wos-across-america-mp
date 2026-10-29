//! Windows x86 controller, run in the same Wine/Proton prefix as the target.
#[cfg(windows)]
mod launcher_session;
#[cfg(windows)]
mod report_output;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    use clap::{Parser, Subcommand, ValueEnum};
    use client_dll::{
        api::{self, ClientConfig, Control},
        controller::Controller,
        profile,
    };
    use std::{net::SocketAddr, path::PathBuf};
    #[derive(Parser)]
    #[command(
        about = "Control the x86 client in an explicit game PID; no original files are replaced"
    )]
    struct Args {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        dll: PathBuf,
        /// Allow only this project's memory_tool.exe host, for automated tests.
        #[arg(long)]
        fixture: bool,
        #[command(subcommand)]
        command: Task,
    }
    #[derive(Subcommand)]
    enum Task {
        Engine {
            #[command(subcommand)]
            task: EngineTask,
        },
        Connect {
            #[arg(long)]
            server: SocketAddr,
            #[arg(long, value_enum, default_value = "game")]
            mode: Mode,
            #[arg(long, default_value_t = 0.0)]
            phase: f32,
        },
        Status,
        Watch {
            #[arg(long, default_value_t=30, value_parser=clap::value_parser!(u64).range(1..=600))]
            seconds: u64,
        },
        Stop,
        Unload,
        /// attach, pump, render, draw, connect --mode drawn, load gate and visual-show, in order.
        SessionStart {
            #[arg(long)]
            server: SocketAddr,
            #[arg(long, default_value = "all", value_parser = parse_player)]
            player: u32,
            #[arg(long, num_args = 3, allow_hyphen_values = true, default_values_t = [0.0f32, 0.0, 0.0])]
            offset: Vec<f32>,
            /// engine_fixture info line saved to a file (bench only, with --fixture).
            #[arg(long)]
            fixture_info: Option<PathBuf>,
            /// NEW folder: copy saves and hash originals+saves before touching the game.
            #[arg(long)]
            backup: Option<PathBuf>,
            /// Saves folder (default: %USERPROFILE%/Documents/18 WoS Across America/saves).
            #[arg(long)]
            saves: Option<PathBuf>,
            /// Keep the fallback shader instead of the AI truck skin.
            #[arg(long)]
            no_skin: bool,
            /// Do not install the command gate (loads/exits then require session-prepare-load).
            #[arg(long)]
            no_load_gate: bool,
            /// Gate deadline from a load request to the recreated visual, in seconds.
            #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u32).range(1..=600))]
            load_timeout: u32,
        },
        /// Client, visual, draw and relayed players in one JSON report.
        SessionStatus,
        /// Read-only backup/hash snapshot without loading the DLL or creating a visual.
        SessionBackup {
            #[arg(long)]
            backup: PathBuf,
            #[arg(long)]
            saves: Option<PathBuf>,
        },
        /// Release the visual BEFORE opening/loading a save; keep probes/network.
        SessionPrepareLoad {
            #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=600))]
            timeout: u64,
        },
        /// Compare originals and saves with a session-start backup; no DLL calls.
        SessionVerify {
            #[arg(long)]
            backup: PathBuf,
        },
        /// Hide, wait for a safe release, remove probes, stop and verify slots.
        SessionStop {
            #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=600))]
            timeout: u64,
            /// Backup folder from session-start: originals and saves must be unchanged.
            #[arg(long)]
            verify: Option<PathBuf>,
        },
    }
    #[derive(Subcommand)]
    enum EngineTask {
        RenderAttach,
        RenderArm,
        /// Read counters and capture state without arming or reading records.
        RenderStatus,
        RenderReport,
        RenderRemove,
        /// Passive actors.car draw observation; pins with the pump like render-attach.
        DrawAttach,
        DrawArm,
        DrawStatus,
        DrawReport,
        DrawRemove,
        /// Passive init/done(phase) capture; never frees mod resources.
        TransitionAttach,
        /// Add passive init_eblock/destroy_eblock import hooks to the same trace.
        TransitionBlocks,
        /// Observe command names at the profiled game handler, without arguments.
        TransitionCommands,
        /// Observe entry/return of the profiled map descriptor; always forwards.
        TransitionMap,
        TransitionArm,
        TransitionStatus,
        /// Snapshot of the append-only capture, even while still armed.
        TransitionReport {
            /// Save a NEW complete snapshot JSON without overwriting an earlier capture.
            #[arg(long)]
            output: Option<PathBuf>,
        },
        TransitionRemove,
        /// ACTIVE gate for map/preview/edit/exit/menu/quit: defers while the visual is retained, releases it,
        /// replays the original once and recreates the visual. Needs pump+render+draw.
        LoadGateAttach {
            #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u32).range(1..=600))]
            timeout: u32,
        },
        LoadGateStatus,
        /// Refused while a deferred load is still active.
        LoadGateRemove,
        /// Retained truck instance drawn after the player's exterior draw; needs draw+render.
        VisualShow {
            #[arg(long, default_value = "models/ai/truck/models/truck.psm")]
            model: String,
            /// World offset in game units (centimeters): x y z.
            #[arg(long, num_args = 3, allow_hyphen_values = true, default_values_t = [400.0f32, 0.0, 0.0])]
            offset: Vec<f32>,
            /// Relayed player id to follow ("all" draws every fresh player);
            /// 0 follows the local player plus offset.
            #[arg(long, default_value = "0", value_parser = parse_player)]
            player: u32,
            /// Skin applied to every part at creation (game: models/ai/truck/shaders/truck.shd).
            #[arg(long)]
            shader: Option<String>,
        },
        VisualHide,
        /// Frees the instance only after queued draws were flushed.
        VisualRelease,
        VisualStatus,
        /// Read-only release checks on the proven outer loop; does not hide/free.
        VisualCheck,
        /// List relayed players (ids for visual-show --player).
        Remotes,
        PhaseProbe,
        /// Private container-backed actor, immediately released; never inserted.
        ItemCycle {
            #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=16))]
            cycles: u32,
            #[arg(long, value_enum, default_value = "car")]
            kind: ItemKind,
        },
        /// Set/read back model_name on a private PMD and immediately release it.
        ModelCycle {
            #[arg(long)]
            model: String,
            #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=16))]
            cycles: u32,
        },
        /// Allocate/release PSM info only; no model instance or rendering.
        InfoCycle {
            #[arg(long)]
            model: String,
            #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=16))]
            cycles: u32,
        },
        /// Create/free one private PSM item per cycle; no draw or world insertion.
        InstanceCycle {
            #[arg(long)]
            model: String,
            #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=16))]
            cycles: u32,
        },
        PrivateCycle {
            #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..=16))]
            cycles: u32,
        },
        PumpAttach,
        PumpStatus,
        PumpRemove,
        Trace,
        Attach {
            #[arg(long)]
            window: Option<u32>,
        },
        Probe,
        Registry,
        Inspect {
            name: String,
        },
        Detach,
        Status,
    }
    #[derive(Clone, Copy, ValueEnum)]
    enum ItemKind {
        Car,
        Pmd,
    }
    #[derive(Clone, Copy, ValueEnum)]
    enum Mode {
        Game,
        Simulated,
        /// Pose from the draw observer, with rotation; install pump+draw first.
        Drawn,
    }
    let args = Args::parse();
    // File verification must remain available after the game has crashed/exited.
    if let Task::SessionVerify { backup } = &args.command {
        println!(
            "{}",
            serde_json::to_string_pretty(&launcher_session::verify(backup)?)?
        );
        return Ok(());
    }
    let dll = args.dll.canonicalize().context("DLL must exist")?;
    ensure!(
        dll.file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("client_dll.dll")),
        "select this project's client_dll.dll"
    );
    let controller = Controller::open(args.pid)?;
    let executable = controller.executable();
    let name = executable.file_name().unwrap_or_default().to_string_lossy();
    if args.fixture {
        ensure!(
            name.eq_ignore_ascii_case("memory_tool.exe")
                || name.eq_ignore_ascii_case("engine_fixture.exe"),
            "fixture target must be memory_tool.exe or engine_fixture.exe"
        );
    } else {
        ensure!(
            name.eq_ignore_ascii_case("prism3d.exe"),
            "target must be prism3d.exe"
        );
    }
    match &args.command {
        Task::SessionBackup { backup, saves } => {
            let saves = match saves {
                Some(path) => path.clone(),
                None => PathBuf::from(std::env::var_os("USERPROFILE").context("USERPROFILE")?)
                    .join("Documents/18 WoS Across America/saves"),
            };
            let root = (!args.fixture).then(|| executable.parent()).flatten();
            let snapshot = launcher_session::backup(backup, root, &saves)?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
            return Ok(());
        }
        Task::SessionStart {
            server,
            player,
            offset,
            fixture_info,
            backup,
            saves,
            no_skin,
            no_load_gate,
            load_timeout,
        } => {
            let backup = match backup {
                Some(directory) => {
                    let saves = match saves {
                        Some(path) => path.clone(),
                        None => {
                            PathBuf::from(std::env::var_os("USERPROFILE").context("USERPROFILE")?)
                                .join("Documents/18 WoS Across America/saves")
                        }
                    };
                    let root = (!args.fixture)
                        .then(|| executable.parent().map(PathBuf::from))
                        .flatten();
                    Some(launcher_session::backup(
                        directory,
                        root.as_deref(),
                        &saves,
                    )?)
                }
                None => None,
            };
            ensure!(
                args.fixture == fixture_info.is_some(),
                "--fixture-info is required with --fixture and only allowed with it"
            );
            let fixture = fixture_info
                .as_ref()
                .map(|path| -> anyhow::Result<serde_json::Value> {
                    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
                })
                .transpose()?;
            let options = launcher_session::Options {
                server: *server,
                player: *player,
                offset: [offset[0], offset[1], offset[2]],
                skin: !*no_skin,
                load_gate: !*no_load_gate,
                load_timeout_ms: *load_timeout * 1000,
                fixture,
            };
            let mut report = launcher_session::start(&controller, &dll, &options)?;
            if let Some(manifest) = backup {
                report["backup_files"] = manifest["files"].as_array().map_or(0, Vec::len).into();
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        Task::SessionStatus => {
            let report = launcher_session::status(&controller, &dll)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        Task::SessionPrepareLoad { timeout } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&launcher_session::prepare_load(
                    &controller,
                    &dll,
                    std::time::Duration::from_secs(*timeout),
                )?)?
            );
            return Ok(());
        }
        Task::SessionStop { timeout, verify } => {
            let mut report = launcher_session::stop(
                &controller,
                &dll,
                args.pid,
                std::time::Duration::from_secs(*timeout),
                !args.fixture,
            )?;
            if let Some(directory) = verify {
                report["files"] = launcher_session::verify(directory)?;
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        _ => {}
    }
    let mut block = match args.command {
        Task::Engine { task } => {
            let render_status_only = matches!(task, EngineTask::RenderStatus);
            let draw_status_only = matches!(task, EngineTask::DrawStatus);
            let transition_status_only = matches!(task, EngineTask::TransitionStatus);
            let transition_output = match &task {
                EngineTask::TransitionReport { output } => output.clone(),
                _ => None,
            };
            if matches!(task, EngineTask::Attach { .. }) {
                controller.load(&dll)?;
            }
            let mut block = match task {
                EngineTask::Attach { window } => {
                    let mut b = api::EngineControl::new(api::UI_ATTACH);
                    b.window = window.unwrap_or(0);
                    b
                }
                EngineTask::Probe => api::EngineControl::new(api::UI_PROBE),
                EngineTask::RenderAttach => api::EngineControl::new(api::UI_RENDER_ATTACH),
                EngineTask::RenderArm => api::EngineControl::new(api::RENDER_ARM),
                EngineTask::RenderStatus => api::EngineControl::new(api::RENDER_STATUS),
                EngineTask::RenderReport => api::EngineControl::new(api::RENDER_STATUS),
                EngineTask::RenderRemove => api::EngineControl::new(api::UI_RENDER_REMOVE),
                EngineTask::DrawAttach => api::EngineControl::new(api::UI_DRAW_ATTACH),
                EngineTask::DrawArm => api::EngineControl::new(api::DRAW_ARM),
                EngineTask::DrawStatus | EngineTask::DrawReport => {
                    api::EngineControl::new(api::DRAW_STATUS)
                }
                EngineTask::DrawRemove => api::EngineControl::new(api::UI_DRAW_REMOVE),
                EngineTask::TransitionAttach => api::EngineControl::new(api::UI_TRANSITION_ATTACH),
                EngineTask::TransitionBlocks => api::EngineControl::new(api::UI_TRANSITION_BLOCKS),
                EngineTask::TransitionCommands => {
                    api::EngineControl::new(api::UI_TRANSITION_COMMANDS)
                }
                EngineTask::TransitionMap => api::EngineControl::new(api::UI_TRANSITION_MAP),
                EngineTask::TransitionArm => api::EngineControl::new(api::TRANSITION_ARM),
                EngineTask::TransitionStatus | EngineTask::TransitionReport { .. } => {
                    api::EngineControl::new(api::TRANSITION_STATUS)
                }
                EngineTask::TransitionRemove => api::EngineControl::new(api::UI_TRANSITION_REMOVE),
                EngineTask::LoadGateAttach { timeout } => {
                    let mut b = api::EngineControl::new(api::UI_LOAD_GATE_ATTACH);
                    b.cycles = timeout * 1000;
                    b
                }
                EngineTask::LoadGateStatus => api::EngineControl::new(api::LOAD_GATE_STATUS),
                EngineTask::LoadGateRemove => api::EngineControl::new(api::UI_LOAD_GATE_REMOVE),
                EngineTask::VisualShow {
                    model,
                    offset,
                    player,
                    shader,
                } => {
                    let mut b = api::EngineControl::new(api::VISUAL_SHOW);
                    if let Some(shader) = shader {
                        api::put(&mut b.message, &shader).map_err(anyhow::Error::msg)?;
                    }
                    b.entity = player;
                    api::put(&mut b.name, &model).map_err(anyhow::Error::msg)?;
                    [b.count_before, b.count_created, b.count_after] =
                        [offset[0], offset[1], offset[2]].map(f32::to_bits);
                    b
                }
                EngineTask::VisualHide => api::EngineControl::new(api::VISUAL_HIDE),
                EngineTask::VisualRelease => api::EngineControl::new(api::VISUAL_RELEASE),
                EngineTask::VisualStatus => api::EngineControl::new(api::VISUAL_STATUS),
                EngineTask::VisualCheck => api::EngineControl::new(api::VISUAL_CHECK),
                EngineTask::Remotes => {
                    let mut rows = Vec::new();
                    loop {
                        let mut b = api::EngineControl::new(api::REMOTE_LIST);
                        b.reserved = rows.len() as u32;
                        controller.engine_control(&dll, &mut b)?;
                        let row = client_dll::draw_observation::Remote::decode(&b);
                        if row.count == 0 {
                            break;
                        }
                        rows.push(row);
                        if rows.len() as u32 >= row.count || rows.len() >= 64 {
                            break;
                        }
                    }
                    println!("{}", serde_json::to_string_pretty(&rows)?);
                    return Ok(());
                }
                EngineTask::Trace => api::EngineControl::new(api::UI_TRACE),
                EngineTask::PumpAttach => api::EngineControl::new(api::UI_PUMP_ATTACH),
                EngineTask::PumpStatus => api::EngineControl::new(api::UI_PUMP_STATUS),
                EngineTask::PumpRemove => api::EngineControl::new(api::UI_PUMP_REMOVE),
                EngineTask::PhaseProbe => api::EngineControl::new(api::PHASE_PROBE),
                EngineTask::ItemCycle { cycles, kind } => {
                    let mut b = api::EngineControl::new(api::PRIVATE_ITEM);
                    b.cycles = cycles;
                    api::put(
                        &mut b.name,
                        match kind {
                            ItemKind::Car => "actors.car",
                            ItemKind::Pmd => "decals.pmd",
                        },
                    )
                    .map_err(anyhow::Error::msg)?;
                    b
                }
                EngineTask::PrivateCycle { cycles } => {
                    let mut b = api::EngineControl::new(api::PRIVATE_CYCLE);
                    b.cycles = cycles;
                    b
                }
                EngineTask::ModelCycle { model, cycles } => {
                    let mut b = api::EngineControl::new(api::PRIVATE_MODEL);
                    b.cycles = cycles;
                    api::put(&mut b.name, &model).map_err(anyhow::Error::msg)?;
                    b
                }
                EngineTask::InfoCycle { model, cycles } => {
                    let mut b = api::EngineControl::new(api::PRIVATE_INFO);
                    b.cycles = cycles;
                    api::put(&mut b.name, &model).map_err(anyhow::Error::msg)?;
                    b
                }
                EngineTask::InstanceCycle { model, cycles } => {
                    let mut b = api::EngineControl::new(api::PRIVATE_INSTANCE);
                    b.cycles = cycles;
                    api::put(&mut b.name, &model).map_err(anyhow::Error::msg)?;
                    b
                }
                EngineTask::Registry => api::EngineControl::new(api::UI_REGISTRY),
                EngineTask::Inspect { name } => {
                    let mut b = api::EngineControl::new(api::UI_INSPECT_TYPE);
                    api::put(&mut b.name, &name).map_err(anyhow::Error::msg)?;
                    b
                }
                EngineTask::Detach => api::EngineControl::new(api::UI_DETACH),
                EngineTask::Status => api::EngineControl::new(api::UI_STATUS),
            };
            controller.engine_control(&dll, &mut block)?;
            if block.command == api::TRANSITION_STATUS {
                use client_dll::transition_observation::{CAPACITY, Record, Summary};
                let summary = Summary::decode(&block);
                if transition_status_only {
                    println!("{}", serde_json::to_string_pretty(&summary)?);
                    return Ok(());
                }
                ensure!(
                    summary.records <= CAPACITY as u32,
                    "transition record limit exceeded"
                );
                let mut records = Vec::new();
                for i in 0..summary.records {
                    let mut row = api::EngineControl::new(api::TRANSITION_READ);
                    row.cycles = summary.generation;
                    row.reserved = i;
                    controller.engine_control(&dll, &mut row)?;
                    records.push(Record::decode(&row).json());
                }
                let mut report = serde_json::json!({"summary":summary,"records":records});
                match client_dll::transition_analysis::analyze(&serde_json::to_vec(&report)?) {
                    Ok(analysis) => report["analysis"] = serde_json::to_value(analysis)?,
                    Err(error) => report["analysis_error"] = error.into(),
                }
                report_output::emit(&report, transition_output.as_deref())?;
                return Ok(());
            }
            if block.command == api::RENDER_STATUS {
                let summary = client_dll::render_observation::Summary::decode(&block);
                if render_status_only {
                    println!("{}", serde_json::to_string_pretty(&summary)?);
                    return Ok(());
                }
                ensure!(
                    summary.flags & 3 == 0,
                    "capture pending; let the game render a complete flush, then retry"
                );
                ensure!(
                    summary.records <= client_dll::render_observation::CAPACITY as u32,
                    "render record limit exceeded"
                );
                let mut records = Vec::new();
                for i in 0..summary.records {
                    let mut row = api::EngineControl::new(api::RENDER_READ);
                    row.cycles = summary.generation;
                    row.reserved = i;
                    controller.engine_control(&dll, &mut row)?;
                    records.push(client_dll::render_observation::Record::decode(&row).json());
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"summary":summary,"records":records})
                    )?
                );
                return Ok(());
            }
            if block.command == api::LOAD_GATE_STATUS {
                let status =
                    client_dll::load_gate::Status::decode(&block).map_err(anyhow::Error::msg)?;
                println!("{}", serde_json::to_string_pretty(&status)?);
                return Ok(());
            }
            if block.command == api::VISUAL_CHECK {
                let report = client_dll::release_policy::Report::decode(&block)
                    .map_err(anyhow::Error::msg)?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                return Ok(());
            }
            if matches!(
                block.command,
                api::VISUAL_SHOW | api::VISUAL_HIDE | api::VISUAL_RELEASE | api::VISUAL_STATUS
            ) {
                let visual = client_dll::draw_observation::Visual::decode(&block);
                println!("{}", serde_json::to_string_pretty(&visual)?);
                return Ok(());
            }
            if block.command == api::DRAW_STATUS {
                use client_dll::draw_observation::{Record, Summary};
                let summary = Summary::decode(&block);
                if draw_status_only {
                    println!("{}", serde_json::to_string_pretty(&summary)?);
                    return Ok(());
                }
                ensure!(
                    summary.flags & 3 == 0,
                    "capture pending; let the game draw the truck, then retry"
                );
                let mut records = Vec::new();
                for i in 0..summary.records {
                    let mut row = api::EngineControl::new(api::DRAW_READ);
                    row.cycles = summary.generation;
                    row.reserved = i;
                    controller.engine_control(&dll, &mut row)?;
                    records.push(Record::decode(&row).json());
                }
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"summary":summary,"records":records})
                    )?
                );
                return Ok(());
            }
            let message = api::text(&block.message).map_err(anyhow::Error::msg)?;
            if !message.is_empty() {
                println!("{message}");
            }
            println!(
                "active={} window=0x{:x} window_thread={} executed_thread={} completed={} inflight={} registry_count={} size={} ctor=0x{:x} dtor=0x{:x}",
                block.active,
                block.window,
                block.window_thread,
                block.executed_thread,
                block.completed,
                block.inflight,
                block.registry_count,
                block.object_size,
                block.constructor,
                block.destructor
            );
            if block.frame_count != 0 {
                println!(
                    "exe=0x{:x} callsite=+0x{:x} frames={:x?}",
                    block.executable_base,
                    block.executable_frame,
                    &block.frames[..block.frame_count.min(32) as usize]
                );
            }
            println!(
                "pump_state={} phase_thread={} phase_site=+0x{:x} phase_hits={} pending={} running={} completed={} cycles={} refs={}/{}/{} vtable=0x{:x} object_dtor=0x{:x}",
                block.phase,
                block.phase_thread,
                block.phase_site,
                block.phase_hits,
                block.phase_pending,
                block.phase_running,
                block.phase_completed,
                block.cycles,
                block.count_before,
                block.count_created,
                block.count_after,
                block.object_vtable,
                block.object_destructor
            );
            return Ok(());
        }
        Task::Connect {
            server,
            mode,
            phase,
        } => {
            if matches!(mode, Mode::Game) {
                ensure!(!args.fixture, "fixture does not load game.dll");
                let root = executable.parent().context("target has no directory")?;
                profile::verify_module(&root.join("aa/lib/game.dll"))
                    .map_err(anyhow::Error::msg)?;
            } else if matches!(mode, Mode::Simulated) {
                ensure!(
                    args.fixture,
                    "simulated mode is restricted to the fixture host"
                );
            }
            let mut config = ClientConfig {
                mode: match mode {
                    Mode::Game => api::GAME,
                    Mode::Simulated => api::SIMULATED,
                    Mode::Drawn => api::DRAWN,
                },
                phase,
                ..ClientConfig::default()
            };
            api::put(&mut config.server, &server.to_string()).map_err(anyhow::Error::msg)?;
            api::put(&mut config.profile, profile::ID).map_err(anyhow::Error::msg)?;
            controller.load(&dll)?;
            let mut block = Control::new(api::START);
            block.config = config;
            block
        }
        Task::Status => Control::new(api::STATUS),
        Task::Watch { seconds } => {
            for _ in 0..seconds {
                let mut block = Control::new(api::STATUS);
                controller.control(&dll, &mut block)?;
                print_status(args.pid, &block);
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            return Ok(());
        }
        Task::SessionStart { .. }
        | Task::SessionBackup { .. }
        | Task::SessionPrepareLoad { .. }
        | Task::SessionStatus
        | Task::SessionStop { .. }
        | Task::SessionVerify { .. } => {
            unreachable!("session tasks return early")
        }
        Task::Stop => Control::new(api::STOP),
        Task::Unload => {
            controller.unload(&dll)?;
            println!("DLL stopped and unloaded from PID {}", args.pid);
            return Ok(());
        }
    };
    controller.control(&dll, &mut block)?;
    print_status(args.pid, &block);
    Ok(())
}
#[cfg(windows)]
fn print_status(pid: u32, block: &client_dll::api::Control) {
    println!(
        "pid={} status={} sample={} sent={} received={} read_failures={} diagnostic={}",
        pid,
        block.status,
        block.sample_state,
        block.sent,
        block.received,
        block.read_failures,
        client_dll::api::text(&block.message).unwrap_or("invalid diagnostic")
    );
}
#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("mp_launcher requires the Windows x86 build, run in the game's Proton prefix");
    std::process::ExitCode::FAILURE
}
#[cfg(windows)]
fn parse_player(text: &str) -> Result<u32, String> {
    if text.eq_ignore_ascii_case("all") {
        Ok(u32::MAX)
    } else {
        text.parse::<u32>()
            .ok()
            .filter(|id| *id != u32::MAX)
            .ok_or_else(|| "player must be an id or \"all\"".into())
    }
}

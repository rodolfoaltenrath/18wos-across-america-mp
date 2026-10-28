//! Windows x86 controller, run in the same Wine/Proton prefix as the target.
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
    }
    let args = Args::parse();
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
    let mut block = match args.command {
        Task::Engine { task } => {
            let render_status_only = matches!(task, EngineTask::RenderStatus);
            let draw_status_only = matches!(task, EngineTask::DrawStatus);
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
            } else {
                ensure!(
                    args.fixture,
                    "simulated mode is restricted to the fixture host"
                );
            }
            let mut config = ClientConfig {
                mode: if matches!(mode, Mode::Game) {
                    api::GAME
                } else {
                    api::SIMULATED
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

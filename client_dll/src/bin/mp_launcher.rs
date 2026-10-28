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
            name.eq_ignore_ascii_case("memory_tool.exe"),
            "fixture target must be memory_tool.exe"
        );
    } else {
        ensure!(
            name.eq_ignore_ascii_case("prism3d.exe"),
            "target must be prism3d.exe"
        );
    }
    let mut block = match args.command {
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

use anyhow::{Result, ensure};
use clap::Parser;
use client_dll::doctor::{self, Options};
use std::{fs::File, io::Write, path::PathBuf};

#[derive(Parser)]
#[command(about = "Read-only static installation report; never loads DLLs or starts the game")]
struct Args {
    /// Explicit installation; otherwise AA_GAME_DIR or current-directory ancestors.
    #[arg(long)]
    game: Option<PathBuf>,
    /// Folder containing Windows x86 client_dll.dll and mp_launcher.exe.
    #[arg(long)]
    client_dir: Option<PathBuf>,
    /// Folder containing the optional native server; defaults next to this tool.
    #[arg(long)]
    native_dir: Option<PathBuf>,
    #[arg(long)]
    check_server: bool,
    /// Actual pfx directory (drive_c, system.reg and user.reg), not compatdata parent.
    #[arg(long)]
    proton_prefix: Option<PathBuf>,
    /// Include private paths in the local report; omit when sharing diagnostics.
    #[arg(long)]
    include_paths: bool,
    /// New report file; parent must exist. Report is also printed to stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let executable = std::env::current_exe()?;
    let parent = executable.parent().unwrap().to_owned();
    let options = Options {
        game: doctor::locate_game(args.game, &std::env::current_dir()?),
        client_dir: args.client_dir.unwrap_or_else(|| parent.clone()),
        native_dir: args.native_dir.unwrap_or(parent),
        check_server: args.check_server,
        proton_prefix: args.proton_prefix,
        include_paths: args.include_paths,
    };
    let mut output = args.output.map(File::create_new).transpose()?;
    let report = doctor::inspect(&options);
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(file) = &mut output {
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    println!("{}", String::from_utf8(bytes)?);
    ensure!(
        report.static_checks_passed,
        "static installation checks failed; inspect report issues (runtime was not tested)"
    );
    Ok(())
}

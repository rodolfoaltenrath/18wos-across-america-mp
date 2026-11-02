use anyhow::{Result, ensure};
use clap::Parser;
use client_dll::doctor::{self, Options};
use std::{fs::File, io::Write, path::PathBuf};

#[derive(Parser)]
#[command(about = "Read-only static installation report; never loads DLLs or starts the game")]
struct Args {
    /// Verify ONLY a candidate-appearance bundle offline; no game/DLL access.
    #[arg(long, conflicts_with_all=["appearance_capture","diagnostics","recording","game","client_dir","native_dir","check_server","proton_prefix","include_paths"])]
    appearance_evidence: Option<PathBuf>,
    /// Read one typed candidate capture; not a connection or visual validation.
    #[arg(long,conflicts_with_all=["diagnostics","recording","game","client_dir","native_dir","check_server","proton_prefix","include_paths"])]
    appearance_capture: Option<PathBuf>,
    /// Explicitly selected second capture; no common clock/player inferred.
    #[arg(long, requires = "appearance_capture")]
    compare_appearance: Option<PathBuf>,
    /// Export only one available typed observation for existing offline auditors.
    #[arg(
        long,
        value_enum,
        requires = "appearance_capture",
        conflicts_with = "compare_appearance"
    )]
    extract_appearance: Option<ExtractAppearance>,
    /// Verify a shareable diagnostic folder offline; never opens game or raw backups.
    #[arg(long, conflicts_with_all=["recording", "game", "client_dir", "native_dir", "check_server", "proton_prefix", "include_paths"])]
    diagnostics: Option<PathBuf>,
    /// Verify a completed session-record directory offline; no process/game access.
    #[arg(long, conflicts_with_all=["game", "client_dir", "native_dir", "check_server", "proton_prefix", "include_paths"])]
    recording: Option<PathBuf>,
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
#[derive(Clone, Copy, clap::ValueEnum)]
enum ExtractAppearance {
    Names,
    Resources,
}
fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(directory) = args.appearance_evidence {
        let manifest = client_dll::appearance_evidence::verify(&directory)?;
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        if let Some(path) = args.output {
            let mut f = File::create_new(path)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        println!("{}", String::from_utf8(bytes)?);
        return Ok(());
    }
    if let Some(path) = args.appearance_capture {
        let a = client_dll::appearance_evidence::load(&path)?;
        let bytes = if let Some(part) = args.extract_appearance {
            match part {
                ExtractAppearance::Names => {
                    serde_json::to_vec_pretty(a.names.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("candidate names unavailable in this capture")
                    })?)?
                }
                ExtractAppearance::Resources => {
                    serde_json::to_vec_pretty(a.resources.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("candidate PSM resources unavailable in this capture")
                    })?)?
                }
            }
        } else if let Some(path) = args.compare_appearance {
            serde_json::to_vec_pretty(&client_dll::appearance_evidence::compare(
                &a,
                &client_dll::appearance_evidence::load(&path)?,
            )?)?
        } else {
            serde_json::to_vec_pretty(&a)?
        };
        if let Some(path) = args.output {
            let mut f = File::create_new(path)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        println!("{}", String::from_utf8(bytes)?);
        return Ok(());
    }
    if let Some(directory) = args.diagnostics {
        let manifest = client_dll::diagnostic_bundle::verify(&directory)?;
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        if let Some(path) = args.output {
            let mut file = File::create_new(path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        println!("{}", String::from_utf8(bytes)?);
        return Ok(());
    }
    if let Some(directory) = args.recording {
        let summary = client_dll::recording::verify(&directory)?;
        let bytes = serde_json::to_vec_pretty(&summary)?;
        if let Some(path) = args.output {
            let mut file = File::create_new(path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        println!("{}", String::from_utf8(bytes)?);
        return Ok(());
    }
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearance_analysis_modes_are_offline_exclusive_and_compare_requires_a_first_capture() {
        assert!(Args::try_parse_from(["doctor", "--appearance-evidence", "own"]).is_ok());
        assert!(
            Args::try_parse_from([
                "doctor",
                "--appearance-capture",
                "a",
                "--compare-appearance",
                "b"
            ])
            .is_ok()
        );
        assert!(Args::try_parse_from(["doctor", "--compare-appearance", "b"]).is_err());
        assert!(Args::try_parse_from(["doctor", "--extract-appearance", "resources"]).is_err());
        assert!(
            Args::try_parse_from([
                "doctor",
                "--appearance-capture",
                "a",
                "--extract-appearance",
                "names"
            ])
            .is_ok()
        );
        assert!(
            Args::try_parse_from([
                "doctor",
                "--appearance-capture",
                "a",
                "--extract-appearance",
                "resources",
                "--compare-appearance",
                "b"
            ])
            .is_err()
        );
        for mode in ["--appearance-evidence", "--appearance-capture"] {
            for other in [
                "--game",
                "--diagnostics",
                "--recording",
                "--client-dir",
                "--native-dir",
                "--proton-prefix",
            ] {
                assert!(Args::try_parse_from(["doctor", mode, "a", other, "b"]).is_err());
            }
            assert!(Args::try_parse_from(["doctor", mode, "a", "--check-server"]).is_err());
        }
        assert!(
            Args::try_parse_from([
                "doctor",
                "--appearance-evidence",
                "a",
                "--appearance-capture",
                "b"
            ])
            .is_err()
        );
    }
}

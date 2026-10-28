//! Local Fedora toolchain preparation without modifying system packages.
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Unpack downloaded MinGW RPMs into a NEW local directory.
    Unpack { rpms: PathBuf, output: PathBuf },
    /// Build or check Windows x86 using a local rustup and unpacked MinGW.
    Windows {
        #[arg(long)]
        check: bool,
        #[arg(long)]
        tests: bool,
        #[arg(long, conflicts_with_all=["check", "tests"])]
        clippy: bool,
    },
}
fn main() -> Result<()> {
    match Args::parse().command {
        Task::Unpack { rpms, output } => {
            let mut files: Vec<_> = fs::read_dir(rpms)?.collect::<std::io::Result<Vec<_>>>()?;
            files.retain(|e| e.path().extension().is_some_and(|x| x == "rpm"));
            files.sort_by_key(|e| e.file_name());
            ensure!(!files.is_empty(), "no RPMs found");
            fs::create_dir(&output).context("output must be a new directory")?;
            let output = output.canonicalize()?;
            for file in files {
                let mut source = Command::new("rpm2cpio")
                    .arg(file.path().canonicalize()?)
                    .stdout(Stdio::piped())
                    .spawn()?;
                let status = Command::new("cpio")
                    .args(["-id", "--quiet", "--no-absolute-filenames"])
                    .current_dir(&output)
                    .stdin(source.stdout.take().unwrap())
                    .status()?;
                let source_status = source.wait()?;
                ensure!(
                    status.success() && source_status.success(),
                    "RPM extraction failed"
                );
            }
            println!("MinGW unpacked into {}", output.display());
        }
        Task::Windows {
            check,
            tests,
            clippy,
        } => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf();
            let local = root.join("target/toolchain");
            let mingw = local.join("mingw/usr/bin");
            let linker = mingw.join("i686-w64-mingw32-gcc");
            ensure!(linker.is_file(), "unpack MinGW first");
            let mut paths = vec![mingw];
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            let mut command = Command::new(local.join("cargo/bin/rustup"));
            command
                .env("RUSTUP_HOME", local.join("rustup"))
                .env("CARGO_HOME", local.join("cargo"))
                .env("PATH", std::env::join_paths(paths)?)
                .env("CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER", linker)
                .current_dir(&root)
                .args(["run", "1.99.0", "cargo"]);
            command.arg(if clippy {
                "clippy"
            } else if tests {
                "test"
            } else if check {
                "check"
            } else {
                "build"
            });
            command.args([
                "-p",
                "client_dll",
                "--target",
                "i686-pc-windows-gnu",
                "--locked",
            ]);
            if tests {
                command.args(["--no-run", "--message-format=json-render-diagnostics"]);
                command.stdout(Stdio::piped());
                let mut child = command.spawn()?;
                let mut paths = Vec::new();
                for line in BufReader::new(child.stdout.take().unwrap()).lines() {
                    let message: serde_json::Value = serde_json::from_str(&line?)?;
                    if message["reason"] == "compiler-artifact"
                        && message["profile"]["test"] == true
                        && let Some(path) = message["executable"].as_str()
                    {
                        paths.push(path.to_string());
                    }
                }
                ensure!(child.wait()?.success(), "Windows test build failed");
                ensure!(!paths.is_empty(), "Cargo returned no test artifacts");
                let manifest = root.join("target/windows-tests.json");
                fs::write(&manifest, serde_json::to_vec_pretty(&paths)?)?;
                println!("Windows test manifest: {}", manifest.display());
                return Ok(());
            } else if clippy {
                command.args(["--all-targets", "--", "-D", "warnings"]);
            } else if !check {
                command.arg("--release");
            }
            ensure!(command.status()?.success(), "Windows build failed");
        }
    }
    Ok(())
}

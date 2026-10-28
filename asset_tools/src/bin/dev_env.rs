//! Host-aware Windows builds without changing persistent Cargo configuration.
use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
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
    /// Build/check Windows x86 using native GNU tools or Fedora's local MinGW.
    Windows {
        /// Build the native Windows GNU workspace instead of only the x86 client.
        #[arg(long)]
        native: bool,
        #[arg(long, conflicts_with = "tests")]
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
            native,
            check,
            tests,
            clippy,
        } => {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .to_path_buf();
            let (mut command, target) = windows_command(&root, native)?;
            command.current_dir(&root);
            command.arg(if clippy {
                "clippy"
            } else if tests {
                "test"
            } else if check {
                "check"
            } else {
                "build"
            });
            if native {
                command.arg("--workspace");
            } else {
                command.args(["-p", "client_dll"]);
            }
            command.args(["--target", target, "--locked"]);
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
                let manifest = root.join(if native {
                    "target/windows-native-tests.json"
                } else {
                    "target/windows-tests.json"
                });
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

#[cfg(not(windows))]
fn windows_command(root: &Path, native: bool) -> Result<(Command, &'static str)> {
    ensure!(
        !native,
        "--native is for a Windows GNU host; use Cargo directly for Linux builds"
    );
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
        .args(["run", "1.99.0", "cargo"]);
    Ok((command, "i686-pc-windows-gnu"))
}

#[cfg(windows)]
fn windows_command(root: &Path, native: bool) -> Result<(Command, &'static str)> {
    let version = Command::new("rustc").arg("-vV").output()?;
    ensure!(version.status.success(), "rustc -vV failed");
    ensure!(
        String::from_utf8(version.stdout)?
            .lines()
            .any(|line| line == "host: x86_64-pc-windows-gnu"),
        "native preparation requires the x86_64-pc-windows-gnu host"
    );
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()?;
    ensure!(sysroot.status.success(), "rustc --print sysroot failed");
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim());
    let bin = sysroot.join("lib/rustlib/x86_64-pc-windows-gnu/bin");
    let gcc = bin.join("self-contained/x86_64-w64-mingw32-gcc.exe");
    let x86_bin = sysroot.join("lib/rustlib/i686-pc-windows-gnu/bin/self-contained");
    let x86_gcc = x86_bin.join("i686-w64-mingw32-gcc.exe");
    let ar = bin.join("llvm-ar.exe");
    ensure!(gcc.is_file(), "install rust-mingw for the Windows GNU host");
    ensure!(
        native || x86_gcc.is_file(),
        "run rustup component add rust-mingw --target i686-pc-windows-gnu"
    );
    ensure!(ar.is_file(), "run rustup component add llvm-tools-preview");
    let cache = root.join("target/toolchain").join(
        sysroot
            .file_name()
            .context("sysroot has no toolchain name")?,
    );
    fs::create_dir_all(&cache)?;
    let dlltool = cache.join("llvm-dlltool.exe");
    if dlltool.exists() {
        ensure!(
            fs::read(&dlltool)? == fs::read(&ar)?,
            "cached dlltool differs from the active toolchain; select a fresh cache directory"
        );
    } else {
        // LLVM's multicall driver chooses dlltool by its executable filename.
        let mut destination = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dlltool)?;
        std::io::copy(&mut fs::File::open(&ar)?, &mut destination)?;
    }
    let mut command = Command::new("cargo");
    let mut paths = vec![bin.join("self-contained"), bin.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    command.env("PATH", std::env::join_paths(paths)?);
    // Cargo also links host build scripts and procedural macros. Pass the
    // active host linker through the environment so stale local target entries
    // cannot select a compiler from an older Rust installation for those jobs.
    command.env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", &gcc);
    command.env("CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER", &x86_gcc);
    for (target, linker, flags) in [
        (
            "x86_64-pc-windows-gnu",
            &gcc,
            vec![
                "-C".to_string(),
                "linker-flavor=gcc".into(),
                "-C".into(),
                format!("link-arg=-B{}", bin.join("self-contained").display()),
                "-C".into(),
                format!("dlltool={}", dlltool.display()),
            ],
        ),
        (
            "i686-pc-windows-gnu",
            &x86_gcc,
            vec![
                "-C".to_string(),
                "linker-flavor=gcc".into(),
                "-C".into(),
                "link-self-contained=yes".into(),
                "-C".into(),
                format!("link-arg=-B{}", x86_bin.display()),
                "-C".into(),
                format!("dlltool={}", dlltool.display()),
            ],
        ),
    ] {
        // JSON strings/arrays are valid TOML here; each config is one argv value,
        // so spaces in the Steam path never pass through a shell parser.
        command.arg("--config").arg(format!(
            "target.{target}.linker={}",
            serde_json::to_string(linker.to_str().context("non-Unicode linker path")?)?
        ));
        command.arg("--config").arg(format!(
            "target.{target}.rustflags={}",
            serde_json::to_string(&flags)?
        ));
    }
    println!(
        "Using GNU tools from {} (existing .cargo configuration preserved)",
        sysroot.display()
    );
    Ok((
        command,
        if native {
            "x86_64-pc-windows-gnu"
        } else {
            "i686-pc-windows-gnu"
        },
    ))
}

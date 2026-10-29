//! Host-aware Windows builds without changing persistent Cargo configuration.
mod dev_env_support;
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
    /// Windows host: x86/native release, both test benches, Clippy and rustfmt.
    Verify,
}
fn main() -> Result<()> {
    let args = Args::parse();
    #[cfg(windows)]
    let _running = if matches!(
        args.command,
        Task::Verify
            | Task::Windows {
                native: true,
                check: false,
                tests: false,
                clippy: false
            }
    ) {
        dev_env_support::RunningExecutable::park(&root())?
    } else {
        None
    };
    match args.command {
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
        } => windows(native, check, tests, clippy)?,
        Task::Verify => verify()?,
    }
    Ok(())
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}
fn windows(native: bool, check: bool, tests: bool, clippy: bool) -> Result<()> {
    let root = root();
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
        let manifest = root.join(manifest(native));
        fs::write(&manifest, serde_json::to_vec_pretty(&paths)?)?;
        println!("Windows test manifest: {}", manifest.display());
        return Ok(());
    } else if clippy {
        command.args(["--all-targets", "--", "-D", "warnings"]);
    } else if !check {
        command.arg("--release");
    }
    ensure!(command.status()?.success(), "Windows build failed");
    Ok(())
}
fn manifest(native: bool) -> &'static str {
    if native {
        "target/windows-native-tests.json"
    } else {
        "target/windows-tests.json"
    }
}
#[cfg_attr(not(windows), allow(dead_code))]
/// Totals parsed from libtest summary lines ("test result: ok. N passed; M failed").
fn totals(line: &str) -> Option<(u32, u32)> {
    let rest = line.trim().strip_prefix("test result: ")?;
    let number = |label: &str| -> Option<u32> {
        let end = rest.find(label)?;
        rest[..end]
            .trim_end()
            .rsplit([' ', ';', '.'])
            .next()?
            .parse()
            .ok()
    };
    Some((number(" passed")?, number(" failed")?))
}
#[cfg_attr(not(windows), allow(dead_code))]
/// Run one bench through this project's test_host, echoing output and summing results.
fn bench(native: bool) -> Result<(u32, u32)> {
    let root = root();
    let host = root.join(if native {
        "target/x86_64-pc-windows-gnu/debug/test_host.exe"
    } else {
        "target/i686-pc-windows-gnu/release/test_host.exe"
    });
    ensure!(host.is_file(), "missing {}", host.display());
    let mut child = Command::new(host)
        .arg(manifest(native))
        .current_dir(&root)
        .stdout(Stdio::piped())
        .spawn()?;
    let (mut passed, mut failed) = (0, 0);
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line?;
        if let Some((p, f)) = totals(&line) {
            passed += p;
            failed += f;
        }
        if line.starts_with("test ") || line.contains("FAILED") {
            println!("{line}");
        }
    }
    let ok = child.wait()?.success();
    ensure!(
        ok && failed == 0,
        "bench failed: {passed} passed, {failed} failed"
    );
    Ok((passed, failed))
}
#[cfg(not(windows))]
fn verify() -> Result<()> {
    anyhow::bail!(
        "verify runs the Windows benches; on Linux use proton_host/test_host as documented"
    )
}
#[cfg(windows)]
fn verify() -> Result<()> {
    let inventory =
        asset_tools::source_audit::audit(&root(), asset_tools::source_audit::Scope::Worktree)?;
    println!(
        "Source audit: {} own files, {} bytes; game/private paths excluded",
        inventory.files.len(),
        inventory.total_bytes
    );
    // The bench loads the release DLL, so build it before the test artifacts.
    windows(false, false, false, false)?;
    // Test artifacts do not refresh the standalone server/asset executables.
    windows(true, false, false, false)?;
    windows(false, false, true, false)?;
    let x86 = bench(false)?;
    windows(true, false, true, false)?;
    let native = bench(true)?;
    windows(false, false, false, true)?;
    windows(true, false, false, true)?;
    let format = Command::new("cargo")
        .args(["fmt", "--all", "--check"])
        .current_dir(root())
        .status()?;
    ensure!(format.success(), "rustfmt reported differences");
    println!(
        "verify: x86 {} passed, native {} passed, Clippy x86/native clean, rustfmt clean",
        x86.0, native.0
    );
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

#[cfg(test)]
mod tests {
    #[test]
    fn parses_libtest_summaries_only() {
        assert_eq!(
            super::totals("test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured"),
            Some((23, 0))
        );
        assert_eq!(
            super::totals("test result: FAILED. 0 passed; 1 failed; 0 ignored"),
            Some((0, 1))
        );
        assert_eq!(super::totals("test foo ... ok"), None);
    }
}

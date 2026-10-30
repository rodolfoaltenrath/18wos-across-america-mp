//! Portable console companion. All injection/teardown remains in mp_launcher.
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Assistente LAN: checagem, PID atual, backup e launcher x86")]
struct Args {
    /// Static checks only: never opens the game, injects or creates private data.
    #[arg(long)]
    check_only: bool,
    /// Installation root; defaults to the folder containing this executable.
    #[arg(long)]
    game: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let interactive = !args.check_only;
    let result = run(args);
    if interactive && let Err(error) = &result {
        eprintln!("\nFalha: {error:#}\nPressione Enter para fechar.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    result
}

fn run(args: Args) -> Result<()> {
    let kit = std::env::current_exe()?.parent().unwrap().to_path_buf();
    let manifest = client_dll::lan_kit::verify(&kit)?;
    let root = args.game.unwrap_or_else(|| kit.clone()).canonicalize()?;
    let report = client_dll::doctor::inspect(&client_dll::doctor::Options {
        game: root.clone(),
        client_dir: kit.clone(),
        native_dir: kit.clone(),
        check_server: false,
        proton_prefix: None,
        include_paths: false,
    });
    if !report.static_checks_passed || args.check_only {
        println!("{}", serde_json::to_string_pretty(&report)?);
    }
    anyhow::ensure!(
        report.static_checks_passed,
        "instalacao incompativel; confira o diagnostico acima"
    );
    println!(
        "Kit {}: hashes e perfil conferidos. LAN/runtime ainda precisam do teste.",
        manifest.source_revision
    );
    if args.check_only {
        return Ok(());
    }
    #[cfg(windows)]
    return windows::interactive(&kit, &root);
    #[cfg(not(windows))]
    anyhow::bail!("O assistente interativo e Windows x86; no Fedora use proton_host/mp_launcher")
}

#[cfg(windows)]
mod windows {
    use anyhow::{Context, Result, ensure};
    use client_dll::{
        inspection::process::{Identity, Process, processes},
        lan_kit::{self, Settings},
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        ffi::{OsStr, OsString},
        fs::{self, File},
        io::{self, Write},
        path::{Path, PathBuf},
        process::Command,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[derive(Serialize, Deserialize)]
    struct Session {
        identity: Identity,
        backup: PathBuf,
        run: PathBuf,
    }
    fn prompt(label: &str, default: &str) -> Result<String> {
        print!("{label} [{default}]: ");
        io::stdout().flush()?;
        let mut text = String::new();
        ensure!(
            io::stdin().read_line(&mut text)? != 0,
            "entrada encerrada; reabra o assistente para recuperar uma parada pendente"
        );
        let text = text.trim().trim_matches('"');
        Ok(if text.is_empty() {
            default.into()
        } else {
            text.into()
        })
    }
    fn load(path: &Path) -> Result<Option<Session>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn save_new(path: &Path, session: &Session) -> Result<()> {
        let mut file = File::create_new(path)?;
        file.write_all(&serde_json::to_vec_pretty(session)?)?;
        file.sync_all()?;
        Ok(())
    }
    fn current(session: &Session) -> Result<()> {
        Process::open(session.identity.pid)?.same_session(&session.identity)
    }
    fn game(root: &Path) -> Result<Identity> {
        let expected = root.join("prism3d.exe").canonicalize()?;
        let mut found = Vec::new();
        for (pid, name) in processes(false)? {
            if name.eq_ignore_ascii_case("prism3d.exe") {
                let p = Process::open(pid)?;
                if Path::new(&p.identity.executable).canonicalize()? == expected {
                    found.push(p.identity);
                }
            }
        }
        ensure!(
            found.len() == 1,
            "esperado um jogo aberto nesta instalacao; encontrados {}. Abra pela Steam e carregue seu save de teste",
            found.len()
        );
        Ok(found.remove(0))
    }
    fn command(
        kit: &Path,
        session: &Session,
        action: &[OsString],
        focus: bool,
        live: bool,
    ) -> Result<()> {
        if live {
            current(session)?;
        }
        let tick = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let log = session.run.join(format!("command-{tick}.log"));
        let mut file = File::create_new(&log)?;
        if focus {
            println!(
                "Volte ao jogo em ate 5 segundos e mantenha-o em primeiro plano durante a operacao."
            );
            std::thread::sleep(Duration::from_secs(5));
        }
        if live {
            current(session)?;
        }
        // Paths/argv are separate OS arguments, never interpreted by a shell.
        let output = Command::new(kit.join("mp_launcher.exe"))
            .arg("--pid")
            .arg(session.identity.pid.to_string())
            .arg("--dll")
            .arg(kit.join("client_dll.dll"))
            .args(action)
            .output()?;
        file.write_all(&output.stdout)?;
        file.write_all(&output.stderr)?;
        file.sync_all()?;
        println!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        println!("Log: {}", log.display());
        ensure!(
            output.status.success(),
            "comando falhou; consulte o log. Uma sessao pendente continua registrada para limpeza"
        );
        Ok(())
    }
    fn args(parts: &[&OsStr]) -> Vec<OsString> {
        parts.iter().map(|v| v.to_os_string()).collect()
    }

    pub fn interactive(kit: &Path, root: &Path) -> Result<()> {
        let bytes = fs::read(kit.join(lan_kit::CONFIG)).context("mp-lan.json ausente")?;
        let defaults: Settings = serde_json::from_slice(&bytes)?;
        defaults.validate()?;
        let appdata = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA")?);
        let digest = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
        let data = appdata.join("18wos-mp").join(&digest[..16]);
        fs::create_dir_all(&data)?;
        let pending = data.join("pending.json");
        let last = data.join("last.json");
        println!(
            "\nDados locais: {}\n1 Iniciar | 2 Status | 3 Parar | 4 Verificar apos saida | 0 Fechar assistente",
            data.display()
        );
        println!(
            "Use 3 ANTES de fechar o jogo. Se o assistente fechar, reabra-o para retomar a limpeza."
        );
        loop {
            let choice = prompt("Opcao", "2")?;
            let result = (|| -> Result<bool> {
                match choice.as_str() {
                    "1" => {
                        ensure!(
                            !pending.exists(),
                            "ja existe uma sessao pendente; use 2/3 ou, se o jogo encerrou, 4"
                        );
                        let identity = game(root)?;
                        let settings = Settings {
                            server: prompt("Servidor IP:porta", &defaults.server.to_string())?
                                .parse()?,
                            room: prompt("Sala", &defaults.room)?,
                        };
                        settings.validate()?;
                        let default_saves =
                            PathBuf::from(std::env::var_os("USERPROFILE").context("USERPROFILE")?)
                                .join("Documents/18 WoS Across America/saves");
                        let saves = PathBuf::from(prompt(
                            "Pasta real dos saves",
                            &default_saves.to_string_lossy(),
                        )?);
                        ensure!(
                            saves.is_dir(),
                            "pasta de saves ausente; informe o caminho real (inclusive OneDrive, se usado)"
                        );
                        ensure!(
                            prompt("Save de teste carregado? Digite sim para iniciar", "nao")?
                                == "sim",
                            "inicio cancelado"
                        );
                        lan_kit::verify(kit)?;
                        let tick = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
                        let run = data.join(format!("run-{tick}"));
                        fs::create_dir(&run)?;
                        let session = Session {
                            identity,
                            backup: run.join("backup"),
                            run,
                        };
                        save_new(&session.run.join("session.json"), &session)?;
                        command(
                            kit,
                            &session,
                            &args(&[
                                OsStr::new("session-backup"),
                                OsStr::new("--backup"),
                                session.backup.as_os_str(),
                                OsStr::new("--saves"),
                                saves.as_os_str(),
                            ]),
                            false,
                            true,
                        )?;
                        save_new(&pending, &session)?;
                        let identity_file = data.join("identity.json");
                        command(
                            kit,
                            &session,
                            &args(&[
                                OsStr::new("session-start"),
                                OsStr::new("--server"),
                                OsStr::new(&settings.server.to_string()),
                                OsStr::new("--room"),
                                OsStr::new(&settings.room),
                                OsStr::new("--identity-file"),
                                identity_file.as_os_str(),
                            ]),
                            true,
                            true,
                        )?;
                        println!(
                            "Inicio solicitado. Use 2 para conferir o aceite de rede e os remotos."
                        );
                    }
                    "2" | "3" => {
                        let session = load(&pending)?.context("nenhuma sessao pendente")?;
                        if choice == "2" {
                            command(
                                kit,
                                &session,
                                &args(&[OsStr::new("session-status")]),
                                false,
                                true,
                            )?;
                        } else {
                            command(
                                kit,
                                &session,
                                &args(&[
                                    OsStr::new("session-stop"),
                                    OsStr::new("--verify"),
                                    session.backup.as_os_str(),
                                ]),
                                true,
                                true,
                            )?;
                            fs::write(&last, serde_json::to_vec_pretty(&session)?)?;
                            fs::remove_file(&pending)?;
                            println!(
                                "Parada e hashes conferidos. Agora pode fechar o jogo normalmente."
                            );
                        }
                    }
                    "4" => {
                        let active = load(&pending)?;
                        let previous = if active.is_none() { load(&last)? } else { None };
                        let session = active
                            .as_ref()
                            .or(previous.as_ref())
                            .context("nenhum backup registrado")?;
                        if active.is_some() {
                            // Failure to open/query is not proof of process exit. Enumeration
                            // must confirm this PID absent; reuse by another process is refused.
                            ensure!(
                                !processes(true)?
                                    .iter()
                                    .any(|(pid, _)| *pid == session.identity.pid),
                                "o PID ainda existe; use 3 para parar com o jogo aberto"
                            );
                        }
                        command(
                            kit,
                            session,
                            &args(&[
                                OsStr::new("session-verify"),
                                OsStr::new("--backup"),
                                session.backup.as_os_str(),
                            ]),
                            false,
                            false,
                        )?;
                        if active.is_some() {
                            fs::write(&last, serde_json::to_vec_pretty(session)?)?;
                            fs::remove_file(&pending)?;
                        }
                        println!(
                            "Hashes conferidos; isto nao comprova restauracao de hooks antes de um crash."
                        );
                    }
                    "0" => {
                        ensure!(
                            !pending.exists(),
                            "use 3 antes de fechar; se o jogo encerrou, use 4"
                        );
                        return Ok(true);
                    }
                    _ => println!("Escolha 1, 2, 3, 4 ou 0."),
                }
                Ok(false)
            })();
            match result {
                Ok(true) => return Ok(()),
                Ok(false) => (),
                Err(error) => eprintln!("\n{error:#}\n"),
            }
        }
    }
}

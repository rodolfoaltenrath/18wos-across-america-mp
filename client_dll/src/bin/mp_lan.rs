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
    /// Resolve a saves folder only (supports %USERPROFILE%); never injects.
    #[arg(long)]
    check_saves: Option<String>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let interactive = !args.check_only && args.check_saves.is_none();
    let result = run(args);
    if interactive && let Err(error) = &result {
        eprintln!("\nFalha: {error:#}\nPressione Enter para fechar.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    result
}

fn run(args: Args) -> Result<()> {
    if let Some(input) = args.check_saves {
        let path = client_dll::assistant_paths::directory(&input)?;
        println!("Pasta de saves confirmada: {}", path.display());
        return Ok(());
    }
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
        assistant_paths::{self, Preferences},
        inspection::process::{Identity, Process, processes},
        lan_kit::{self, Settings},
        session_evidence::{self, Capture},
    };
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        ffi::{OsStr, OsString},
        fs::{self, File},
        io::{self, Write},
        path::{Path, PathBuf},
        process::Command,
        sync::OnceLock,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
        let text = text.trim();
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
        let unix = SystemTime::now().duration_since(UNIX_EPOCH)?;
        let tick = unix.as_nanos();
        let role = match action.first().and_then(|v| v.to_str()) {
            Some("session-backup") => "backup",
            Some("session-start") => "start",
            Some("session-status") => "status",
            Some("session-stop") => "stop",
            Some("session-verify") => "verify",
            _ => anyhow::bail!("unsupported assistant operation"),
        };
        let stem = format!("command-{tick}-{role}");
        let mut stdout = File::create_new(session.run.join(format!("{stem}.stdout.log")))?;
        let mut stderr = File::create_new(session.run.join(format!("{stem}.stderr.log")))?;
        let mut receipt = File::create_new(session.run.join(format!("{stem}.json")))?;
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
        let elapsed = HELPER_CLOCK.get_or_init(Instant::now).elapsed().as_millis();
        let started = Instant::now();
        let output = Command::new(kit.join("mp_launcher.exe"))
            .arg("--pid")
            .arg(session.identity.pid.to_string())
            .arg("--dll")
            .arg(kit.join("client_dll.dll"))
            .args(action)
            .output()?;
        stdout.write_all(&output.stdout)?;
        stderr.write_all(&output.stderr)?;
        stdout.sync_all()?;
        stderr.sync_all()?;
        receipt.write_all(&serde_json::to_vec_pretty(&serde_json::json!({
            "schema":1, "operation":role, "requested_unix_ms":unix.as_millis(),
            "helper_process":std::process::id(), "execution_start_helper_ms":elapsed,
            "focus_countdown_ms":if focus {5000} else {0},
            "operation_duration_ms":started.elapsed().as_millis(),
            "success":output.status.success(), "exit_code":output.status.code(),
            "stdout":format!("{stem}.stdout.log"), "stderr":format!("{stem}.stderr.log"),
            "focus_measured":false,
        }))?)?;
        receipt.sync_all()?;
        if let Ok(bytes) = session_evidence::sanitize(&output.stdout) {
            let exported = session.run.join("diagnosticos");
            let saved = (|| -> Result<()> {
                fs::create_dir_all(&exported)?;
                let mut file = File::create_new(exported.join(format!("{tick}-{role}.json")))?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                Ok(())
            })();
            if let Err(e) = saved {
                eprintln!(
                    "Exportacao do diagnostico indisponivel: {e:#}; o log bruto foi preservado."
                );
            } else {
                println!("Diagnosticos para analise: {}", exported.display());
            }
        }
        if output.status.success() {
            match session_evidence::parse(&output.stdout) {
                Ok(Capture::Snapshot { snapshot, .. }) => {
                    println!(
                        "Conexao: {} | caminhões remotos: {} | origem: {}",
                        snapshot
                            .network_session
                            .as_ref()
                            .map(|s| match s.phase {
                                multiplayer_server::session_client::Phase::Joining => "entrando",
                                multiplayer_server::session_client::Phase::Connected => "conectado",
                                multiplayer_server::session_client::Phase::Reconnecting =>
                                    "reconectando",
                                multiplayer_server::session_client::Phase::Rejected => "recusado",
                                multiplayer_server::session_client::Phase::Closing => "encerrando",
                                multiplayer_server::session_client::Phase::Closed => "encerrado",
                            }
                            .to_owned())
                            .unwrap_or_else(|| "v1".into()),
                        snapshot.remotes.len(),
                        if snapshot.client.sample_state == 1 {
                            "disponivel"
                        } else {
                            "sem pose recente; confira o jogo em primeiro plano"
                        }
                    );
                    if let Some(s) = &snapshot.network_session {
                        println!(
                            "RTT ate o servidor (ms): {}",
                            s.rtt_ms
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "aguardando".into())
                        );
                        if let Some(reason) = s.refusal {
                            println!("Conexao recusada: {reason:?}");
                        }
                    }
                }
                Ok(Capture::Stop { summary, .. }) => println!(
                    "Parada: {} de {} slots conferidos; {} arquivos iguais ao backup.",
                    summary.matching_slots,
                    summary.reported_slots,
                    summary.unchanged_files_reported.unwrap_or(0)
                ),
                Err(_) => println!("Operacao {role} concluida."),
            }
        } else {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        }
        println!(
            "Diagnostico salvo: {}",
            session.run.join(format!("{stem}.stdout.log")).display()
        );
        ensure!(
            output.status.success(),
            "comando falhou; consulte o log. Uma sessao pendente continua registrada para limpeza"
        );
        Ok(())
    }
    fn args(parts: &[&OsStr]) -> Vec<OsString> {
        parts.iter().map(|v| v.to_os_string()).collect()
    }
    static HELPER_CLOCK: OnceLock<Instant> = OnceLock::new();
    fn capture_status(kit: &Path, session: &Session) {
        if let Err(error) = command(
            kit,
            session,
            &args(&[OsStr::new("session-status")]),
            false,
            true,
        ) {
            eprintln!(
                "Diagnostico automatico indisponivel: {error:#}. A parada continua disponivel."
            );
        }
    }

    pub fn interactive(kit: &Path, root: &Path) -> Result<()> {
        let _ = HELPER_CLOCK.set(Instant::now());
        let bytes = fs::read(kit.join(lan_kit::CONFIG)).context("mp-lan.json ausente")?;
        let defaults: Settings = serde_json::from_slice(&bytes)?;
        defaults.validate()?;
        let appdata = PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA")?);
        let digest = format!("{:x}", Sha256::digest(root.to_string_lossy().as_bytes()));
        let data = appdata.join("18wos-mp").join(&digest[..16]);
        fs::create_dir_all(&data)?;
        let pending = data.join("pending.json");
        let last = data.join("last.json");
        let preferences_path = data.join("preferences.json");
        let mut preferences: Option<Preferences> = match fs::read(&preferences_path) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes)
                .and_then(|p| p.validate().map(|_| p).map_err(serde::de::Error::custom))
            {
                Ok(p) => Some(p),
                Err(_) => {
                    eprintln!(
                        "Preferencias invalidas; usando configuracao do kit e Documents do Windows."
                    );
                    None
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("Nao foi possivel ler preferencias: {e}; a limpeza continua disponivel.");
                None
            }
        };
        println!(
            "\nDados locais: {}\n1 Iniciar multiplayer | 2 Consultar conexao | 3 Parar multiplayer (antes de fechar jogo) | 4 Conferir arquivos apos saida | 0 Fechar assistente",
            data.display()
        );
        println!(
            "O diagnostico e salvo automaticamente ao iniciar e parar. Digite a opcao e pressione Enter."
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
                            server: prompt(
                                "Servidor IP:porta",
                                &preferences
                                    .as_ref()
                                    .map(|p| p.server)
                                    .unwrap_or(defaults.server)
                                    .to_string(),
                            )?
                            .parse()?,
                            room: prompt(
                                "Sala",
                                preferences
                                    .as_ref()
                                    .map(|p| p.room.as_str())
                                    .unwrap_or(&defaults.room),
                            )?,
                        };
                        settings.validate()?;
                        let profile =
                            PathBuf::from(std::env::var_os("USERPROFILE").context("USERPROFILE")?);
                        let documents = assistant_paths::documents().map_err(|e| eprintln!("Documents do Windows indisponivel: {e}; confirme o caminho manualmente.")).ok();
                        let default_saves = assistant_paths::default_saves(
                            preferences.as_ref().map(|p| p.saves.as_path()),
                            documents.as_deref(),
                            &profile,
                        );
                        println!(
                            "Informe a PASTA que contem os .sav (pode colar o endereco do Explorador). Digite 0 para cancelar."
                        );
                        let saves = loop {
                            let input = prompt("Pasta de saves", &default_saves.to_string_lossy())?;
                            ensure!(input != "0", "inicio cancelado");
                            match assistant_paths::directory(&input) {
                                Ok(path) => break path,
                                Err(e) => eprintln!("{e:#}"),
                            }
                        };
                        ensure!(
                            prompt("Save de teste carregado? Digite sim para iniciar", "nao")?
                                .eq_ignore_ascii_case("sim"),
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
                        let next_preferences = Preferences {
                            server: settings.server,
                            room: settings.room.clone(),
                            saves,
                        };
                        if let Err(e) = fs::write(
                            &preferences_path,
                            serde_json::to_vec_pretty(&next_preferences)?,
                        ) {
                            eprintln!("Nao foi possivel lembrar estas preferencias: {e}");
                        }
                        preferences = Some(next_preferences);
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
                        capture_status(kit, &session);
                        println!(
                            "Multiplayer iniciado. Volte ao jogo; use 3 aqui antes de fechar o jogo."
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
                            capture_status(kit, &session);
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
                            capture_status(kit, &session);
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

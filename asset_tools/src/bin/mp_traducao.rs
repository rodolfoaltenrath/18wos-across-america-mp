//! Player-facing PT-BR installer: no Rust, no repository, no game files shipped.
use anyhow::{Context, Result};
use asset_tools::{career_rules::Goal, translation_setup as setup};
use clap::{Parser, Subcommand};
use std::{io::Write, path::PathBuf};

#[derive(Parser)]
#[command(about = "Instala a traducao PT-BR usando os arquivos do seu proprio jogo")]
struct Args {
    /// Pasta do jogo (a que contem prism3d.exe).
    #[arg(long, default_value = ".")]
    game: PathBuf,
    /// Dados privados e backups (padrao: %LOCALAPPDATA%\18wos-mp\traducao).
    #[arg(long)]
    state: Option<PathBuf>,
    #[command(subcommand)]
    task: Option<Task>,
}
#[derive(Subcommand)]
enum Task {
    /// Gera o pacote e instala, com backup. Padrao. Jogo fechado.
    Install,
    /// Instala o que faltar: traducao e regras da carreira. Jogo fechado.
    Preparar {
        /// Meta em dolares inteiros; sem esta opcao ou --sem-limite, pergunta.
        #[arg(long, conflicts_with = "sem_limite")]
        meta: Option<u64>,
        #[arg(long)]
        sem_limite: bool,
    },
    /// Mostra se a traducao esta instalada.
    Status,
    /// Remove regras e traducao e devolve o init.cfg anterior. Jogo fechado.
    Restore,
}
fn ask_goal() -> Result<Goal> {
    print!("Meta da carreira em dolares (Enter = sem limite): ");
    std::io::stdout().flush()?;
    let mut text = String::new();
    std::io::stdin().read_line(&mut text)?;
    let text = text.trim().replace(['.', ','], "");
    if text.is_empty() {
        return Ok(Goal::Unlimited);
    }
    Goal::dollars(text.parse().context("informe somente numeros")?)
}
fn main() -> Result<()> {
    let args = Args::parse();
    let state = match args.state {
        Some(path) => path,
        None => PathBuf::from(
            std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA ausente; use --state")?,
        )
        .join("18wos-mp")
        .join("traducao"),
    };
    match args.task.unwrap_or(Task::Install) {
        Task::Install => {
            let done = setup::install(&args.game, &state)?;
            println!(
                "Traducao instalada: {} textos, {} arquivos. Nao testada em todas as telas.",
                done.translated_keys, done.files
            );
            for font in &done.skipped_fonts {
                println!("Fonte mantida como original: {font}");
            }
            println!("Para remover: mp_traducao restore (com o jogo fechado).");
        }
        Task::Preparar { meta, sem_limite } => {
            let goal = match (meta, sem_limite) {
                (Some(amount), _) => Some(Goal::dollars(amount)?),
                (None, true) => Some(Goal::Unlimited),
                // Asks only when there is something to install with it.
                (None, false) if setup::career_installed(&args.game, &state) => None,
                (None, false) => Some(ask_goal()?),
            };
            let done = setup::prepare(&args.game, &state, goal)?;
            println!(
                "Traducao: {}. Regras da carreira: {}.",
                if done.translation_installed_now {
                    "instalada agora"
                } else {
                    "ja estava instalada"
                },
                if done.career_installed_now {
                    "instaladas agora"
                } else {
                    "ja estavam instaladas"
                }
            );
        }
        Task::Status => {
            let word = |on: bool| if on { "instalada" } else { "nao instalada" };
            let (translation, career) = setup::summary(&args.game, &state);
            println!(
                "Traducao: {}. Regras da carreira: {}.",
                word(translation),
                word(career)
            );
        }
        Task::Restore => {
            let status = setup::restore(&args.game, &state)?;
            println!("Estado apos restaurar: {}", status.state);
        }
    }
    Ok(())
}

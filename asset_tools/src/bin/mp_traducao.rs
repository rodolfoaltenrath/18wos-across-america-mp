//! Player-facing PT-BR installer: no Rust, no repository, no game files shipped.
use anyhow::{Context, Result};
use asset_tools::translation_setup as setup;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    /// Mostra se a traducao esta instalada.
    Status,
    /// Remove a traducao e devolve o init.cfg anterior. Jogo fechado.
    Restore,
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
        Task::Status => match setup::status(&args.game, &state)? {
            Some(status) => println!("{}", serde_json::to_string_pretty(&status)?),
            None => println!("Nenhuma instalacao registrada nesta pasta de dados."),
        },
        Task::Restore => {
            let status = setup::restore(&args.game, &state)?;
            println!("Estado apos restaurar: {}", status.state);
        }
    }
    Ok(())
}

//! Portable, user-invoked setup. Never launches the game or downloads content.
use anyhow::{Context, Result, ensure};
use asset_tools::{
    player_setup::{self as setup, Component, session},
    ui_tools::mount,
};
use clap::{Parser, Subcommand};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    about = "Traducao PT-BR, fonte e menu opcionais com backup local; mantenha o jogo fechado"
)]
struct Args {
    #[arg(long)]
    game: Option<PathBuf>,
    #[arg(long)]
    data: Option<PathBuf>,
    #[arg(long)]
    typeface: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Task>,
}
#[derive(Subcommand)]
enum Task {
    /// Somente confere perfil, montagens e fonte; nao instala nem cria backup.
    Check,
    /// Prepara em diretorio NOVO, sem instalar ou alterar o jogo.
    Prepare {
        #[arg(long, value_enum)]
        component: Component,
        #[arg(long)]
        output: PathBuf,
    },
    /// Instala um componente e guarda a referencia do backup automaticamente.
    Install {
        #[arg(long, value_enum)]
        component: Component,
    },
    /// Consulta a instalacao e backups sem criar arquivos.
    Status,
    /// Restaura pelo backup registrado; recusa alteracoes desconhecidas.
    Restore {
        #[arg(long, value_enum)]
        component: Component,
    },
}
fn game(explicit: Option<PathBuf>, executable: &Path) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.canonicalize()?);
    }
    for path in executable
        .ancestors()
        .skip(1)
        .chain(std::iter::once(std::env::current_dir()?.as_path()))
    {
        if path.join("aa/lib/game.dll").is_file() {
            return Ok(path.canonicalize()?);
        }
    }
    anyhow::bail!(
        "Coloque mp_setup.exe na raiz do jogo, ao lado de prism3d.exe, ou informe --game PASTA"
    )
}
fn face(explicit: Option<PathBuf>, executable: &Path) -> Result<PathBuf> {
    Ok(explicit.unwrap_or(
        executable
            .parent()
            .context("pasta do assistente")?
            .join("Carlito-Bold.ttf"),
    ))
}
fn check(root: &Path, typeface: &Path) -> Result<()> {
    mount::setup_sources(root, mount::Overlay::PtBrText)?;
    let face = asset_tools::localization::typeface::Typeface::load(typeface)?;
    println!(
        "Perfil e montagens conferidos. Fonte: {}. Nenhum arquivo do jogo foi alterado.",
        face.name
    );
    Ok(())
}
fn print_states(root: &Path, data: &Path) -> Result<()> {
    for state in session::inspect(root, data)? {
        let name = match state.component {
            Component::Translation => "Traducao e fonte",
            Component::Menu => "Menu experimental",
        };
        let stage = match state.stage {
            "absent" => "nao instalado",
            "prepared" => "preparado; instalacao ainda pendente",
            "installed" => "instalado",
            "restored" => "restaurado",
            "unmanaged" => "encontrado; backup pertence a outro instalador",
            "interrupted" => "interrompido; use Restaurar",
            _ => "arquivos alterados; preserve o backup",
        };
        println!("{name}: {stage}");
    }
    Ok(())
}
fn install(root: &Path, data: &Path, typeface: &Path, component: Component) -> Result<()> {
    let session = session::Session::open(root, data)?;
    let status = session.install(
        component,
        (component == Component::Translation).then_some(typeface),
    )?;
    ensure!(
        status.state == "installed",
        "Instalacao ainda incompleta; preserve o backup"
    );
    println!(
        "Instalado. Backup preservado em {}. A aparencia no jogo ainda precisa da conferencia visual.",
        data.display()
    );
    Ok(())
}
fn restore(root: &Path, data: &Path, component: Component) -> Result<()> {
    session::Session::open(root, data)?.restore(component)?;
    println!("Restaurado. Os backups foram preservados.");
    Ok(())
}
fn interactive(root: &Path, data: &Path, typeface: &Path) -> Result<()> {
    println!(
        "\nInstalacao local de traducao e fonte\nMantenha o jogo fechado. Os saves nao sao alterados.\nMenu experimental: instale ANTES da traducao; restaure DEPOIS dela.\n1 Instalar traducao e fonte | 2 Instalar menu experimental | 3 Consultar instalacao\n4 Restaurar traducao | 5 Restaurar menu | 6 Conferir arquivos | 0 Sair"
    );
    loop {
        print!("Opcao [3]: ");
        io::stdout().flush()?;
        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            return Ok(());
        }
        let result = match input.trim() {
            "0" => return Ok(()),
            "1" => install(root, data, typeface, Component::Translation),
            "2" => install(root, data, typeface, Component::Menu),
            "3" | "" => print_states(root, data),
            "4" => restore(root, data, Component::Translation),
            "5" => restore(root, data, Component::Menu),
            "6" => check(root, typeface),
            _ => {
                println!("Escolha 0 a 6.");
                continue;
            }
        };
        if let Err(e) = result {
            eprintln!(
                "Nao foi possivel concluir: {e:#}\nOs arquivos e backups existentes foram preservados. Se houver instalacao parcial, consulte o estado e use Restaurar."
            );
        }
    }
}
fn main() -> Result<()> {
    let args = Args::parse();
    let executable = std::env::current_exe()?;
    let root = game(args.game, &executable)?;
    let typeface = face(args.typeface, &executable)?;
    let data = args
        .data
        .map(Ok)
        .unwrap_or_else(|| session::default_data(&root))?;
    match args.command {
        None => interactive(&root, &data, &typeface),
        Some(Task::Check) => check(&root, &typeface),
        Some(Task::Status) => print_states(&root, &data),
        Some(Task::Install { component }) => install(&root, &data, &typeface, component),
        Some(Task::Restore { component }) => restore(&root, &data, component),
        Some(Task::Prepare { component, output }) => {
            let report = setup::prepare(
                &root,
                component,
                (component == Component::Translation).then_some(typeface.as_path()),
                &output,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutation_requires_an_explicit_component_and_prepare_requires_new_output_argument() {
        assert!(Args::try_parse_from(["setup"]).is_ok());
        for action in ["check", "status"] {
            assert!(Args::try_parse_from(["setup", action]).is_ok());
        }
        for action in ["install", "restore", "prepare"] {
            assert!(Args::try_parse_from(["setup", action]).is_err());
        }
        assert!(Args::try_parse_from(["setup", "install", "--component", "translation"]).is_ok());
        assert!(
            Args::try_parse_from([
                "setup",
                "prepare",
                "--component",
                "translation",
                "--output",
                "new"
            ])
            .is_ok()
        );
        assert!(Args::try_parse_from(["setup", "install", "--component", "unknown"]).is_err());
    }
}

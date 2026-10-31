//! UI foundation tools. Original strings/assets never enter source control.
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
#[derive(Clone, Copy, ValueEnum)]
enum Lang {
    PtBr,
    En,
}
#[derive(Clone, Copy, ValueEnum)]
enum Kind {
    /// Experimental main menu (priority p4).
    UiPreview,
    /// Translated table and extended fonts built by l10n_tools (priority p5).
    PtbrText,
}
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Read-only package/profile/config/DEF checks; never creates a backup or installs.
    Preflight {
        #[arg(long)]
        game: PathBuf,
        #[arg(long)]
        package: PathBuf,
        #[arg(long, value_enum, default_value = "ui-preview")]
        kind: Kind,
    },
    Project {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        main_menu_preview: bool,
        #[arg(long, value_enum, default_value = "pt-br")]
        language: Lang,
    },
    /// Read-only bounded localization/font inventory; NEW private output file.
    Inventory {
        #[arg(long, required = true)]
        archive: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Closed game only: mount our exact package with backup first.
    Install {
        #[arg(long)]
        game: PathBuf,
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        backup: PathBuf,
        #[arg(long, value_enum, default_value = "ui-preview")]
        kind: Kind,
    },
    /// Closed game only; refuses unknown changes, keeps original backup.
    /// The backup records which package it belongs to.
    Restore {
        #[arg(long)]
        game: PathBuf,
        #[arg(long)]
        backup: PathBuf,
    },
    /// Read-only inspection; available even if the game/profile changed.
    Status {
        #[arg(long)]
        game: PathBuf,
        #[arg(long)]
        backup: PathBuf,
    },
}
fn main() -> Result<()> {
    use asset_tools::ui_tools as ui;
    match Args::parse().command {
        Task::Preflight {
            game,
            package,
            kind,
        } => {
            let overlay = match kind {
                Kind::UiPreview => ui::mount::Overlay::UiPreview,
                Kind::PtbrText => ui::mount::Overlay::PtBrText,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&ui::mount::preflight(&game, &package, overlay)?)?
            );
        }
        Task::Project {
            output,
            main_menu_preview,
            language,
        } => ui::project(
            &output,
            main_menu_preview,
            match language {
                Lang::PtBr => mod_ui::locale::Language::PtBr,
                Lang::En => mod_ui::locale::Language::En,
            },
        )?,
        Task::Inventory { archive, output } => {
            let r = ui::inventory::scan(&archive)?;
            let mut f = std::fs::File::create_new(output)?;
            use std::io::Write;
            f.write_all(&serde_json::to_vec_pretty(&r)?)?;
            f.sync_all()?;
            println!(
                "{} chaves referenciadas; nao e cobertura integral da traducao.",
                r.keys.len()
            );
        }
        Task::Install {
            game,
            package,
            backup,
            kind,
        } => {
            let overlay = match kind {
                Kind::UiPreview => ui::mount::Overlay::UiPreview,
                Kind::PtbrText => ui::mount::Overlay::PtBrText,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&ui::mount::install_overlay(
                    &game, &package, &backup, overlay
                )?)?
            )
        }
        Task::Restore { game, backup } => println!(
            "{}",
            serde_json::to_string_pretty(&ui::mount::restore(&game, &backup)?)?
        ),
        Task::Status { game, backup } => println!(
            "{}",
            serde_json::to_string_pretty(&ui::mount::status(&game, &backup)?)?
        ),
    }
    Ok(())
}

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
    /// Career goal/no-limit and single scenario built by career-rules (priority p6).
    CareerRules,
}
impl Kind {
    fn overlay(self) -> asset_tools::ui_tools::mount::Overlay {
        use asset_tools::ui_tools::mount::Overlay;
        match self {
            Self::UiPreview => Overlay::UiPreview,
            Self::PtbrText => Overlay::PtBrText,
            Self::CareerRules => Overlay::CareerRules,
        }
    }
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
    /// NEW derived_local project: only the millionaire career, with the chosen
    /// goal or no limit. Not validated in game; install with --kind career-rules.
    CareerRules {
        #[arg(long, conflicts_with_all = ["unlimited", "career_manifest"])]
        goal_dollars: Option<u64>,
        #[arg(long, conflicts_with = "career_manifest")]
        unlimited: bool,
        /// Take the goal from an existing `mp_career` manifest.
        #[arg(long)]
        career_manifest: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "pt-br")]
        language: Lang,
        #[arg(long)]
        output: PathBuf,
        /// Archives in ascending mount priority (default: the three of aa/init.cfg).
        #[arg(long)]
        archive: Vec<PathBuf>,
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
    use asset_tools::{career_rules as rules, ui_tools as ui};
    match Args::parse().command {
        Task::Preflight {
            game,
            package,
            kind,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&ui::mount::preflight(
                    &game,
                    &package,
                    kind.overlay()
                )?)?
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
        Task::CareerRules {
            goal_dollars,
            unlimited,
            career_manifest,
            language,
            output,
            archive,
        } => {
            let goal = match (goal_dollars, unlimited, career_manifest) {
                (Some(n), false, None) => rules::Goal::dollars(n)?,
                (None, true, None) => rules::Goal::Unlimited,
                (None, false, Some(path)) => {
                    anyhow::ensure!(
                        std::fs::metadata(&path)?.len() <= 64 * 1024,
                        "career manifest limit"
                    );
                    rules::goal_from_manifest(&std::fs::read(&path)?)?
                }
                _ => anyhow::bail!("choose --goal-dollars, --unlimited or --career-manifest"),
            };
            let archives = if archive.is_empty() {
                ["data.scs", "aa/base.scs", "aa/patch_1_10.scs"]
                    .map(PathBuf::from)
                    .to_vec()
            } else {
                archive
            };
            let mounts = asset_tools::localization::Mounts::open(&archives)?;
            let language = match language {
                Lang::PtBr => rules::Language::PtBr,
                Lang::En => rules::Language::En,
            };
            let r = rules::build(&mounts, goal, language, &output)?;
            println!(
                "{} cenarios ocultados, {} arquivos; nao validado no jogo.",
                r.disabled.len(),
                r.files
            );
        }
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
        } => println!(
            "{}",
            serde_json::to_string_pretty(&ui::mount::install_overlay(
                &game,
                &package,
                &backup,
                kind.overlay()
            )?)?
        ),
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

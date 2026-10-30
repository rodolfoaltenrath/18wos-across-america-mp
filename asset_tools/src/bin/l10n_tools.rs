//! Local language/font tooling. No game process or original file is modified.
use anyhow::{Result, ensure};
use asset_tools::localization::{self as l10n, catalog, def};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    task: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Decoded single-byte ORIGINAL text to private stdout; never send it to Git.
    Read {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        entry: String,
    },
    /// Font metrics/slot IDs only. Presence is not appearance validation.
    Font {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        entry: String,
    },
    /// Bounded escaped-byte view; output may contain private original text.
    Inspect {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        entry: String,
        #[arg(long, default_value_t = 512)]
        limit: u16,
    },
    /// Compare a PT-BR catalogue with the visible local.def; JSON report with keys only.
    Check {
        #[arg(long)]
        catalog: PathBuf,
        /// Archives in ascending mount priority (default: the three of aa/init.cfg).
        #[arg(long)]
        archive: Vec<PathBuf>,
    },
    /// Copy a catalogue to a NEW file, filling empty `src` hashes from the game table.
    Stamp {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        archive: Vec<PathBuf>,
    },
    /// Write a NEW derived_local mod project: translated table and extended fonts.
    Build {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        archive: Vec<PathBuf>,
    },
}
fn mounts(archive: Vec<PathBuf>) -> Result<l10n::Mounts> {
    let list = if archive.is_empty() {
        ["data.scs", "aa/base.scs", "aa/patch_1_10.scs"]
            .map(PathBuf::from)
            .to_vec()
    } else {
        archive
    };
    l10n::Mounts::open(&list)
}
fn load(path: &Path) -> Result<catalog::Catalog> {
    catalog::load(&std::fs::read(path)?)
}
fn main() -> Result<()> {
    match Args::parse().task {
        Task::Read { archive, entry } => {
            let b = l10n::codec::decode(&l10n::read_entry(&archive, &entry)?)?;
            println!("{}", def::decode(&b));
        }
        Task::Font { archive, entry } => {
            let b = l10n::read_entry(&archive, &entry)?;
            println!("{}", serde_json::to_string_pretty(&l10n::font::parse(&b)?)?);
        }
        Task::Inspect {
            archive,
            entry,
            limit,
        } => {
            let bytes = l10n::read_entry(&archive, &entry)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&l10n::inspect(&bytes, limit as usize))?
            );
        }
        Task::Check { catalog, archive } => {
            let source = def::parse(&mounts(archive)?.read(l10n::TABLE)?)?;
            let report = catalog::check(&load(&catalog)?, &source);
            println!("{}", serde_json::to_string_pretty(&report)?);
            ensure!(
                report.errors.is_empty() && report.stale.is_empty(),
                "catalogue has errors or stale entries"
            );
        }
        Task::Stamp {
            catalog,
            output,
            archive,
        } => {
            let source = def::parse(&mounts(archive)?.read(l10n::TABLE)?)?;
            let mut cat = load(&catalog)?;
            let mut stamped = 0;
            for (key, entry) in cat.entries.iter_mut().filter(|(_, e)| e.src.is_empty()) {
                if let Some(s) = source.iter().find(|s| &s.key == key) {
                    entry.src = catalog::source_hash(&s.value);
                    stamped += 1;
                }
            }
            let mut json = serde_json::to_string_pretty(&cat)?;
            json.push('\n');
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .and_then(|mut f| std::io::Write::write_all(&mut f, json.as_bytes()))?;
            println!("{stamped} entries stamped into {}", output.display());
        }
        Task::Build {
            catalog,
            output,
            archive,
        } => {
            let report = l10n::build(&mounts(archive)?, &load(&catalog)?, &output)?;
            println!(
                "{} of {} keys translated, {} fonts extended, {} skipped, {} files; not tested in game.",
                report.catalog.translated,
                report.catalog.translatable_keys,
                report.fonts.len(),
                report.skipped_fonts.len(),
                report.files
            );
        }
    }
    Ok(())
}

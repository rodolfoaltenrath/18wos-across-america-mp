//! Local language/font tooling. No game process or original file is modified.
use anyhow::{Result, ensure};
use asset_tools::localization::{self as l10n, catalog, def, literals};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    task: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Read-only PML/reference audit: metadata only, no original prose or rewrites.
    PmlAudit {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        archive: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        /// Fail on catalog/reference/syntax issues after preserving the report.
        #[arg(long)]
        strict: bool,
    },
    /// Inventory literal fields. Original prose is OMITTED unless --include-source.
    LiteralScan {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        archive: Vec<PathBuf>,
        /// PRIVATE original prose for authoring; never version this inventory.
        #[arg(long)]
        include_source: bool,
    },
    /// Bind authored PT-BR choices (source-value SHA256 -> text) to local resources.
    LiteralCompose {
        #[arg(long)]
        choices: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        archive: Vec<PathBuf>,
    },
    /// Validate literal catalog hashes, protected fields, encoding and placeholders.
    LiteralCheck {
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        archive: Vec<PathBuf>,
    },
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
        /// Local TrueType/OpenType file, rasterized with antialiased edges.
        /// Replaces Latin glyphs, preserving legacy advances and line metrics.
        #[arg(long)]
        typeface: Option<PathBuf>,
        /// Coverage contrast (0..100), default 40; never thresholds the edges.
        #[arg(long,requires="typeface",value_parser=clap::value_parser!(u8).range(0..=100))]
        font_strength: Option<u8>,
        /// Authored plain DEF translations outside local.def, checked before writes.
        #[arg(long)]
        literals: Option<PathBuf>,
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
    catalog::load(&l10n::read_catalog_file(path)?)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
fn main() -> Result<()> {
    match Args::parse().task {
        Task::PmlAudit {
            catalog,
            archive,
            output,
            strict,
        } => {
            let r = l10n::pml::audit(&mounts(archive)?, &load(&catalog)?)?;
            write_new(&output, &serde_json::to_vec_pretty(&r)?)?;
            println!(
                "{} fields; {} translated keys, {} configuration keys, {} fields with literal text. Engine not tested.",
                r.fields.len(),
                r.translated_keys,
                r.preserved_config_keys,
                r.fields_with_literal_text
            );
            ensure!(
                !strict || r.checks_passed(),
                "PML/reference audit has issues; inspect the saved metadata report"
            );
        }
        Task::LiteralScan {
            output,
            archive,
            include_source,
        } => {
            let report = literals::inventory(&mounts(archive)?, include_source)?;
            write_new(&output, &serde_json::to_vec_pretty(&report)?)?;
            println!(
                "{} editable literal fields; {} protected/deferred. Original prose exported: {}.",
                report.eligible, report.protected, include_source
            );
            println!(
                "{} malformed resources left untouched; see unreadable_resources in the report.",
                report.unreadable_resources.len()
            );
        }
        Task::LiteralCompose {
            choices,
            output,
            archive,
        } => {
            let bytes = asset_tools::localization::read_catalog_file(&choices)?;
            let choices = serde_json::from_slice(&bytes)?;
            let catalog = literals::compose(&mounts(archive)?, &choices)?;
            write_new(&output, &serde_json::to_vec_pretty(&catalog)?)?;
            println!(
                "{} resource entries authored; no original prose in catalog.",
                catalog.files.len()
            );
        }
        Task::LiteralCheck { catalog, archive } => {
            let cat = literals::load(&l10n::read_catalog_file(&catalog)?)?;
            let result = literals::compile(&mounts(archive)?, &cat)?;
            println!("{}", serde_json::to_string_pretty(&result.report)?);
            ensure!(result.report.errors.is_empty(), "literal catalog rejected");
        }
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
            typeface,
            font_strength,
            literals,
        } => {
            let face = typeface
                .as_deref()
                .map(l10n::typeface::Typeface::load)
                .transpose()?
                .map(|f| f.with_strength(font_strength.unwrap_or(40)))
                .transpose()?;
            let extra = literals
                .as_deref()
                .map(|p| literals::load(&l10n::read_catalog_file(p)?))
                .transpose()?;
            let report = l10n::build_with_literals(
                &mounts(archive)?,
                &load(&catalog)?,
                &output,
                face.as_ref(),
                extra.as_ref(),
            )?;
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

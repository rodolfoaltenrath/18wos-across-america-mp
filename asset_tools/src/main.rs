use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Local asset automation for 18 Wheels of Steel: Across America")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Audit own sources; never traverse ignored game assets or print contents.
    AuditSource {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long, value_enum, default_value = "worktree")]
        scope: asset_tools::source_audit::Scope,
    },
    /// Export only committed HEAD sources as a NEW deterministic ZIP.
    SourceBundle {
        #[arg(default_value = ".")]
        root: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Check a source bundle's allowlist/limits/hashes without extraction.
    VerifySourceBundle { input: PathBuf },
    /// Create a NEW project with our own PNG/text fixtures (no game inputs).
    ModExample { output: PathBuf },
    /// Create a NEW hash-pinned reconstruction recipe with only our fixtures.
    ModRecipeExample { output: PathBuf },
    /// Reconstruct a local overlay using verified inputs and a separate cache.
    ModRecipeBuild {
        recipe: PathBuf,
        output: PathBuf,
        #[arg(long)]
        cache: PathBuf,
        /// Bind a local original archive by SHA-256, never by a private path in the recipe.
        #[arg(long)]
        archive: Vec<PathBuf>,
        #[arg(long)]
        distributable: bool,
    },
    /// Hash a local file and optionally one bounded ZIP/SCS entry, without extraction.
    ModSourceHash {
        input: PathBuf,
        #[arg(long)]
        entry: Option<String>,
    },
    /// Build an immutable deterministic data overlay; reuse only an exact match.
    ModBuild {
        project: PathBuf,
        output: PathBuf,
        /// Reject every asset declared derived_local.
        #[arg(long)]
        distributable: bool,
    },
    /// Verify paths, declared origins, dependencies, hashes and canonical ZIP.
    ModVerify {
        package: PathBuf,
        #[arg(long)]
        distributable: bool,
    },
    /// Copy a verified package to a NEW directory; does not activate it in-game.
    ModStage {
        package: PathBuf,
        destination: PathBuf,
        #[arg(long)]
        distributable: bool,
    },
    /// Backup to a NEW directory, then remove only the two unchanged stage files.
    ModUnstage { package: PathBuf, backup: PathBuf },
    /// Find ZIP/SCS archives and report entry counts and texture formats.
    Scan {
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// List archive entries without extracting game content.
    List {
        archive: PathBuf,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// Print one exact UTF-8 entry without extracting files.
    Read {
        archive: PathBuf,
        entry: String,
        #[arg(long, default_value_t = 64, value_parser = clap::value_parser!(u32).range(1..=1024))]
        max_kib: u32,
    },
    /// Extract to a NEW directory; never overwrite existing files.
    Extract {
        archive: PathBuf,
        output: PathBuf,
        #[arg(long, default_value_t = 2048, value_parser = clap::value_parser!(u64).range(1..=16384))]
        max_mib: u64,
    },
    /// Convert one texture to PNG, optionally flipping or resizing it.
    Convert {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        flip_y: bool,
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=8192))]
        max_edge: Option<u32>,
    },
    /// Pack a directory as a new uncompressed ZIP-compatible .scs file.
    Pack { input: PathBuf, output: PathBuf },
    /// Inspect a local Windows executable's PE machine type.
    InspectPe { executable: PathBuf },
    /// Find candidate packed numeric triples in a small binary file.
    InspectVectors {
        input: PathBuf,
        #[arg(long, default_value_t = 1000.0)]
        min_horizontal: f64,
        #[arg(long, default_value_t = 100000.0)]
        max_horizontal: f64,
        #[arg(long, default_value_t = 1000.0)]
        max_vertical: f64,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long, value_enum, default_value = "f32")]
        kind: asset_tools::binary::NumberKind,
    },
    /// Inspect a bounded window of a binary file without changing it.
    InspectBytes {
        input: PathBuf,
        #[arg(long, default_value = "0", value_parser = asset_tools::binary::offset)]
        offset: u64,
        #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u16).range(1..=4096))]
        length: u16,
        /// Interpret little-endian values every four bytes from the chosen offset.
        #[arg(long)]
        numbers: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::AuditSource { root, scope } => {
            let report = asset_tools::source_audit::audit(&root, scope)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::SourceBundle { root, output } => {
            let manifest = asset_tools::source_audit::bundle(&root, &output)?;
            println!(
                "Bundled {} committed sources from {} into {}",
                manifest.files.len(),
                manifest.commit,
                output.display()
            );
            Ok(())
        }
        Command::VerifySourceBundle { input } => {
            let manifest = asset_tools::source_audit::verify_bundle(&input)?;
            println!(
                "Verified {} committed sources from {} (integrity, not signature)",
                manifest.files.len(),
                manifest.commit
            );
            Ok(())
        }
        Command::Scan { root } => asset_tools::scan(&root),
        Command::ModExample { output } => asset_tools::mod_package::example(&output),
        Command::ModRecipeExample { output } => asset_tools::mod_package::recipe::example(&output),
        Command::ModRecipeBuild {
            recipe,
            output,
            cache,
            archive,
            distributable,
        } => {
            let report = asset_tools::mod_package::recipe::build(
                &recipe,
                &output,
                &cache,
                &archive,
                distributable,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::ModSourceHash { input, entry } => {
            let report = asset_tools::mod_package::recipe::fingerprint(&input, entry.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::ModBuild {
            project,
            output,
            distributable,
        } => {
            let report = asset_tools::mod_package::build(&project, &output, distributable)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::ModVerify {
            package,
            distributable,
        } => {
            let report = asset_tools::mod_package::verify(&package, distributable)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::ModStage {
            package,
            destination,
            distributable,
        } => {
            let report = asset_tools::mod_package::stage(&package, &destination, distributable)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::ModUnstage { package, backup } => {
            let report = asset_tools::mod_package::unstage(&package, &backup)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Command::List { archive, limit } => asset_tools::list(&archive, limit),
        Command::Read {
            archive,
            entry,
            max_kib,
        } => {
            use std::io::Write;
            let text = asset_tools::read_text(&archive, &entry, max_kib as usize * 1024)?;
            std::io::stdout().lock().write_all(text.as_bytes())?;
            Ok(())
        }
        Command::Extract {
            archive,
            output,
            max_mib,
        } => {
            let count = asset_tools::extract(&archive, &output, max_mib * 1024 * 1024)?;
            println!("Extracted {count} files into {}", output.display());
            Ok(())
        }
        Command::Convert {
            input,
            output,
            flip_y,
            max_edge,
        } => asset_tools::convert(&input, &output, flip_y, max_edge),
        Command::Pack { input, output } => {
            let count = asset_tools::pack(&input, &output)?;
            println!("Packed {count} files into {}", output.display());
            Ok(())
        }
        Command::InspectPe { executable } => asset_tools::inspect_pe(&executable),
        Command::InspectVectors {
            input,
            min_horizontal,
            max_horizontal,
            max_vertical,
            limit,
            kind,
        } => asset_tools::binary::vectors(
            &input,
            min_horizontal,
            max_horizontal,
            max_vertical,
            limit,
            kind,
        ),
        Command::InspectBytes {
            input,
            offset,
            length,
            numbers,
        } => asset_tools::binary::inspect(&input, offset, usize::from(length), numbers),
    }
}

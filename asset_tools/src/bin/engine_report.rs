//! Offline analysis only. Reports must stay local; game bytes are never included in Git.
use anyhow::Result;
use asset_tools::{binary::offset, pe32::Pe32};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
struct Args {
    module: PathBuf,
    #[command(subcommand)]
    task: Task,
}
#[derive(Subcommand)]
enum Task {
    /// Read one bounded UTF-8 string by RVA, without loading the module.
    String {
        #[arg(long,value_parser=parse_rva)]
        rva: u32,
    },
    Symbols {
        #[arg(long, default_value = "")]
        contains: String,
    },
    Disassemble {
        #[arg(long,value_parser=parse_rva)]
        rva: u32,
        #[arg(long, default_value_t = 256)]
        length: usize,
    },
    References {
        #[arg(long,value_parser=parse_rva)]
        rva: u32,
    },
    /// Unaligned VA byte matches in code/data; not proof of an instruction.
    Pointers {
        #[arg(long,value_parser=parse_rva)]
        rva: u32,
        #[arg(long, default_value_t=4096, value_parser=clap::value_parser!(u32).range(1..=65536))]
        limit: u32,
    },
    /// Straight-line named GetProcAddress/EAX store candidates; no image execution.
    Bindings {
        #[arg(long,value_parser=parse_rva)]
        rva: u32,
        #[arg(long, default_value_t = 4096)]
        length: usize,
        #[arg(long, default_value = "")]
        contains: String,
    },
    /// Candidate register-based accesses to a field offset, not a type proof.
    Field {
        #[arg(long,value_parser=parse_rva)]
        offset: u32,
    },
    Records,
    Words {
        #[arg(long,value_parser=parse_rva)]
        rva: u32,
        #[arg(long, default_value_t = 8)]
        count: u16,
    },
}
fn parse_rva(text: &str) -> Result<u32, String> {
    u32::try_from(offset(text)?).map_err(|_| "RVA must fit u32".into())
}
fn main() -> Result<()> {
    let args = Args::parse();
    let pe = Pe32::open(&args.module)?;
    match args.task {
        Task::Bindings {
            rva,
            length,
            contains,
        } => {
            let rows = pe.dynamic_bindings(rva, length)?;
            let rows: Vec<_> = rows
                .into_iter()
                .filter(|b| b.symbol.contains(&contains))
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Task::String { rva } => println!("{}", pe.string(rva)?.escape_default()),
        Task::Symbols { contains } => {
            for symbol in pe.exports()?.iter().filter(|s| s.name.contains(&contains)) {
                println!("export +0x{:x} {}", symbol.rva, symbol.name);
            }
            for import in pe.imports()?.iter().filter(|s| s.name.contains(&contains)) {
                println!(
                    "import IAT+0x{:x} {}!{}",
                    import.iat_rva, import.dll, import.name
                );
            }
        }
        Task::Disassemble { rva, length } => {
            for instruction in pe.disassemble(rva, length)? {
                println!("+0x{:08x}: {}", instruction.rva, instruction.text);
            }
        }
        Task::References { rva } => {
            println!("{}", serde_json::to_string_pretty(&pe.references(rva)?)?)
        }
        Task::Field { offset } => {
            for instruction in pe.field_references(offset) {
                println!("+0x{:08x}: {}", instruction.rva, instruction.text);
            }
        }
        Task::Pointers { rva, limit } => println!(
            "{}",
            serde_json::to_string_pretty(&pe.pointer_occurrences(rva, limit as usize)?)?
        ),
        Task::Records => println!(
            "{}",
            serde_json::to_string_pretty(&pe.entity_candidates()?)?
        ),
        Task::Words { rva, count } => {
            for index in 0..u32::from(count.min(1024)) {
                let at = rva
                    .checked_add(index * 4)
                    .ok_or_else(|| anyhow::anyhow!("RVA overflow"))?;
                let word = pe.word(at)?;
                let detail = pe.rva(word).and_then(|r| pe.string(r)).unwrap_or_default();
                println!("+0x{at:08x}: 0x{word:08x} {}", detail.escape_default());
            }
        }
    }
    Ok(())
}

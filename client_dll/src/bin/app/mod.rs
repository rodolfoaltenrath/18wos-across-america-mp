use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use client_dll::{
    addresses::{Address, hex},
    inspection::{
        process::{self, MemoryScope, Process},
        scanner::{self, Change, Options},
        snapshot::{Kind, Snapshot},
    },
};
use multiplayer_server::protocol::{self, ClientUpdate, MAX_PACKET, Position, TICK_HZ, VERSION};
use std::{
    io::{self, BufRead, Write},
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(about = "Read-only Windows coordinate discovery and UDP telemetry in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Decode actors.car value metadata, including inherited definitions; no engine calls.
    Values {
        #[arg(long)]
        pid: u32,
    },
    /// Read hash-identified entity type records; never creates or calls an entity.
    Entities {
        #[arg(long)]
        pid: u32,
    },
    /// List game processes; --all also lists other process names/PIDs.
    Processes {
        #[arg(long)]
        all: bool,
    },
    /// Show loaded modules and base addresses for one process.
    Modules {
        #[arg(long)]
        pid: u32,
    },
    /// Summarize writable regions by type and show the largest allocations.
    Regions {
        #[arg(long)]
        pid: u32,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// Read a bounded window as hexadecimal, u32 and f32; never writes memory.
    Dump {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        address: Address,
        #[arg(long, default_value_t = 128, value_parser = clap::value_parser!(u16).range(1..=4096))]
        length: u16,
    },
    /// Save bounded floating-point candidates from writable memory.
    Scan {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "f32")]
        kind: Kind,
        #[arg(long, value_parser = hex, default_value = "0")]
        start: u64,
        #[arg(long, value_parser = hex)]
        end: Option<u64>,
        #[arg(long, default_value_t = 4)]
        alignment: u64,
        #[arg(long, default_value_t = -1_000_000.0, allow_hyphen_values = true)]
        min: f64,
        #[arg(long, default_value_t = 1_000_000.0, allow_hyphen_values = true)]
        max: f64,
        #[arg(long, default_value_t = 0.01)]
        min_abs: f64,
        #[arg(long)]
        all_readable: bool,
        #[arg(long, value_enum, default_value = "all")]
        memory: MemoryScope,
        #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u64).range(1..=4096))]
        max_mib: u64,
        #[arg(long, default_value_t = 4_000_000)]
        max_candidates: usize,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=60))]
        delay_secs: u64,
    },
    /// Compare a snapshot with the SAME running process; save surviving values.
    Filter {
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum)]
        mode: Change,
        #[arg(long, default_value_t = 0.001)]
        epsilon: f64,
        #[arg(long)]
        max_delta: Option<f64>,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u64).range(0..=60))]
        delay_secs: u64,
    },
    /// Show saved candidate values; does not need the process to be running.
    Inspect {
        input: PathBuf,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// Compare saved values and optionally group identical changes.
    Diff {
        before: PathBuf,
        after: PathBuf,
        #[arg(long, default_value_t = 40)]
        limit: usize,
        #[arg(long, default_value_t = 0.001)]
        min_delta: f64,
        #[arg(long)]
        max_delta: Option<f64>,
        #[arg(long, default_value_t = 0.0)]
        min_abs: f64,
        #[arg(long)]
        group: bool,
    },
    /// Read selected addresses repeatedly without writing to the process.
    Watch {
        #[arg(long)]
        pid: u32,
        #[arg(long, required = true, num_args = 1..=128)]
        address: Vec<Address>,
        #[arg(long, value_enum, default_value = "f32")]
        kind: Kind,
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=86400))]
        seconds: u64,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..=60))]
        hz: u32,
    },
    /// Send three selected remote coordinates to the UDP server at 60 Hz.
    Stream {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        x: Address,
        #[arg(long)]
        y: Address,
        #[arg(long)]
        z: Address,
        #[arg(long, value_enum, default_value = "f32")]
        kind: Kind,
        #[arg(long, default_value = protocol::DEFAULT_SERVER)]
        server: SocketAddr,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=86400))]
        seconds: u64,
    },
    /// Controlled test process. Prints addresses, then accepts "set X Y Z".
    Fixture,
}

fn delay(seconds: u64) {
    if seconds != 0 {
        println!("Capturing in {seconds}s; return to the game now.");
        std::thread::sleep(Duration::from_secs(seconds));
    }
}

fn preview(snapshot: &Snapshot, limit: usize) {
    println!(
        "PID={} executable={} kind={:?} candidates={}",
        snapshot.header.identity.pid,
        process::executable_name(&snapshot.header.identity),
        snapshot.header.kind,
        snapshot.candidates.len()
    );
    for value in snapshot.candidates.iter().take(limit) {
        println!("0x{:x}  {:.9}", value.address, value.value);
    }
}

fn new_output(path: &Path) -> Result<()> {
    ensure!(!path.exists(), "output already exists; choose a new file");
    ensure!(
        path.extension().is_some_and(|ext| ext == "aamem"),
        "scan output must end in .aamem"
    );
    Ok(())
}

fn resolve(process: &Process, address: &Address, modules: &[process::Module]) -> Result<u64> {
    address
        .resolve(
            process.max_address(),
            |name| {
                modules
                    .iter()
                    .find(|module| module.name.eq_ignore_ascii_case(name))
                    .map(|module| module.base)
                    .ok_or_else(|| format!("module {name} is not loaded"))
            },
            |at| process.pointer(at).map_err(|e| e.to_string()),
        )
        .map_err(anyhow::Error::msg)
}

fn sample(
    process: &Process,
    address: &Address,
    modules: &[process::Module],
    kind: Kind,
) -> Result<(u64, f64)> {
    let at = resolve(process, address, modules)?;
    let mut bytes = [0; 8];
    process.read(at, &mut bytes[..kind.width()])?;
    let value = kind.decode(&bytes);
    ensure!(value.is_finite(), "value at 0x{at:x} is NaN/infinite");
    Ok((at, value))
}

fn cadence(next: &mut Instant, interval: Duration) {
    *next += interval;
    let now = Instant::now();
    if *next > now {
        std::thread::sleep(*next - now);
    } else {
        *next = now;
    }
}

pub fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Values { pid } => {
            let process = Process::open(pid)?;
            let rows = client_dll::inspection::entities::car_values(&process)?;
            println!("{}", serde_json::to_string_pretty(&rows)?);
        }
        Command::Entities { pid } => {
            let process = Process::open(pid)?;
            let records = client_dll::inspection::entities::registered(&process)?;
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        Command::Processes { all } => {
            let processes = process::processes(all)?;
            for (pid, name) in &processes {
                println!("{pid:>8}  {name}");
            }
            if processes.is_empty() {
                println!("Game not running; open it and load a driving session first.");
            }
        }
        Command::Dump {
            pid,
            address,
            length,
        } => {
            let process = Process::open(pid)?;
            let at = resolve(&process, &address, &process.modules()?)?;
            let mut bytes = vec![0; usize::from(length)];
            process.read(at, &mut bytes)?;
            for (index, chunk) in bytes.chunks(4).enumerate() {
                let hex = chunk
                    .iter()
                    .map(|v| format!("{v:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                print!("0x{:08x}  {hex:<11}", at + (index * 4) as u64);
                if let Ok(four) = <[u8; 4]>::try_from(chunk) {
                    print!(
                        "  u32=0x{:08x} ({:>10})  f32={:.9}",
                        u32::from_le_bytes(four),
                        u32::from_le_bytes(four),
                        f32::from_le_bytes(four)
                    );
                }
                println!();
            }
        }
        Command::Modules { pid } => {
            let process = Process::open(pid)?;
            println!(
                "{} | {}-bit | created={}",
                process.identity.executable,
                process.identity.pointer_bytes * 8,
                process.identity.created
            );
            for module in process.modules()? {
                println!(
                    "0x{:x}..0x{:x}  {}",
                    module.base,
                    module.base + module.size,
                    module.name
                );
            }
        }
        Command::Regions { pid, limit } => {
            let process = Process::open(pid)?;
            let mut regions = process.region_details(0, None, false)?;
            let mut totals = std::collections::BTreeMap::<&str, u64>::new();
            for region in &regions {
                *totals.entry(region.kind_name()).or_default() += region.end - region.start;
            }
            for (kind, bytes) in totals {
                println!(
                    "{kind}: {} bytes ({:.2} MiB)",
                    bytes,
                    bytes as f64 / 1048576.0
                );
            }
            regions.sort_by_key(|region| std::cmp::Reverse(region.end - region.start));
            for region in regions.iter().take(limit) {
                println!(
                    "0x{:x}..0x{:x}  {:>10} bytes  {} protect=0x{:x}",
                    region.start,
                    region.end,
                    region.end - region.start,
                    region.kind_name(),
                    region.protection
                );
            }
        }
        Command::Scan {
            pid,
            output,
            kind,
            start,
            end,
            alignment,
            min,
            max,
            min_abs,
            all_readable,
            memory,
            max_mib,
            max_candidates,
            delay_secs,
        } => {
            new_output(&output)?;
            let process = Process::open(pid)?;
            delay(delay_secs);
            let (snapshot, report) = scanner::scan(
                &process,
                Options {
                    start,
                    end,
                    kind,
                    alignment,
                    min,
                    max,
                    min_abs,
                    all_readable,
                    memory,
                    max_bytes: max_mib * 1048576,
                    max_candidates,
                },
            )?;
            snapshot.save(&output)?;
            println!(
                "Read {} bytes; unreadable blocks={}; saved {}",
                report.bytes_read,
                report.unreadable,
                output.display()
            );
            preview(&snapshot, 20);
        }
        Command::Filter {
            input,
            output,
            mode,
            epsilon,
            max_delta,
            delay_secs,
        } => {
            new_output(&output)?;
            let previous = Snapshot::load(&input)?;
            let process = Process::open(previous.header.identity.pid)?;
            process.same_session(&previous.header.identity)?;
            delay(delay_secs);
            let (snapshot, report) =
                scanner::refine(&process, &previous, mode, epsilon, max_delta)?;
            snapshot.save(&output)?;
            println!(
                "{} -> {} candidates; unreadable values={}; saved {}",
                previous.candidates.len(),
                snapshot.candidates.len(),
                report.unreadable,
                output.display()
            );
            preview(&snapshot, 20);
        }
        Command::Inspect { input, limit } => preview(&Snapshot::load(&input)?, limit),
        Command::Diff {
            before,
            after,
            limit,
            min_delta,
            max_delta,
            min_abs,
            group,
        } => {
            ensure!(
                min_delta.is_finite() && min_delta >= 0.0 && min_abs.is_finite() && min_abs >= 0.0,
                "minimum values must be finite and nonnegative"
            );
            ensure!(
                max_delta.is_none_or(|value| value.is_finite() && value > 0.0),
                "max-delta must be finite and positive"
            );
            let before = Snapshot::load(&before)?;
            let after = Snapshot::load(&after)?;
            let mut changes = client_dll::inspection::differences::differences(&before, &after)?;
            changes.retain(|change| {
                change.delta().abs() > min_delta
                    && max_delta.is_none_or(|max| change.delta().abs() <= max)
                    && change.before.abs() >= min_abs
                    && change.after.abs() >= min_abs
            });
            println!("{} matching changes", changes.len());
            if group {
                let mut groups = std::collections::BTreeMap::<(u64, u64), Vec<u64>>::new();
                for change in changes {
                    groups
                        .entry((change.before.to_bits(), change.after.to_bits()))
                        .or_default()
                        .push(change.address);
                }
                let mut groups: Vec<_> = groups.into_iter().collect();
                groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.1[0].cmp(&b.1[0])));
                for ((old, new), addresses) in groups.iter().take(limit) {
                    let (old, new) = (f64::from_bits(*old), f64::from_bits(*new));
                    let at = addresses
                        .iter()
                        .take(8)
                        .map(|at| format!("0x{at:x}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!(
                        "count={} {:.9} -> {:.9} delta={:+.9} [{at}]",
                        addresses.len(),
                        old,
                        new,
                        new - old
                    );
                }
            } else {
                changes.sort_by(|a, b| b.delta().abs().total_cmp(&a.delta().abs()));
                for change in changes.iter().take(limit) {
                    println!(
                        "0x{:x} {:.9} -> {:.9} delta={:+.9}",
                        change.address,
                        change.before,
                        change.after,
                        change.delta()
                    );
                }
            }
        }
        Command::Watch {
            pid,
            address,
            kind,
            seconds,
            hz,
        } => {
            let process = Process::open(pid)?;
            let modules = process.modules()?;
            let start = Instant::now();
            let mut next = start;
            while start.elapsed() < Duration::from_secs(seconds) {
                print!("{:>8.3}s", start.elapsed().as_secs_f64());
                for address in &address {
                    let (at, value) = sample(&process, address, &modules, kind)?;
                    print!("  0x{at:x}={value:.6}");
                }
                println!();
                cadence(&mut next, Duration::from_secs_f64(1.0 / f64::from(hz)));
            }
        }
        Command::Stream {
            pid,
            x,
            y,
            z,
            kind,
            server,
            seconds,
        } => {
            let process = Process::open(pid)?;
            let modules = process.modules()?;
            let socket = UdpSocket::bind(if server.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            })?;
            socket.connect(server)?;
            socket.set_nonblocking(true)?;
            let mut buffer = [0; MAX_PACKET + 1];
            let start = Instant::now();
            let mut next = start;
            let mut sequence = 0u64;
            let mut sent = 0u64;
            let mut received = 0u64;
            let mut last_log = 0;
            while start.elapsed() < Duration::from_secs(seconds) {
                let values = [&x, &y, &z].map(|address| {
                    sample(&process, address, &modules, kind).map(|(_, value)| value as f32)
                });
                let [x, y, z] = values;
                let position = Position {
                    x: x?,
                    y: y?,
                    z: z?,
                };
                ensure!(
                    position.is_valid(),
                    "coordinates cannot be represented as finite f32 for the protocol"
                );
                let bytes = protocol::encode(&ClientUpdate {
                    version: VERSION,
                    sequence,
                    position,
                })
                .map_err(anyhow::Error::msg)?;
                match socket.send(&bytes) {
                    Ok(_) => sent += 1,
                    Err(error)
                        if error.kind() == io::ErrorKind::WouldBlock
                            || error.raw_os_error() == Some(10054) => {}
                    Err(error) => return Err(error.into()),
                }
                for _ in 0..256 {
                    match socket.recv(&mut buffer) {
                        Ok(len) => {
                            if protocol::decode_server(&buffer[..len]).is_some() {
                                received += 1;
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                        Err(error) if matches!(error.raw_os_error(), Some(10040 | 10054)) => {
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                if sequence == 0 || start.elapsed().as_secs() > last_log {
                    println!(
                        "sent={sent} received={received} xyz=({:.6}, {:.6}, {:.6})",
                        position.x, position.y, position.z
                    );
                    last_log = start.elapsed().as_secs();
                }
                sequence = sequence.checked_add(1).context("sequence exhausted")?;
                cadence(&mut next, Duration::from_secs_f64(1.0 / f64::from(TICK_HZ)));
            }
            println!("Finished: sent={sent} received={received}");
        }
        Command::Fixture => fixture()?,
    }
    Ok(())
}

fn fixture() -> Result<()> {
    let values = Box::new([1.25f32, -2.5, 3.75].map(|value| AtomicU32::new(value.to_bits())));
    let doubles = Box::new([1.25f64, -2.5, 3.75].map(|value| AtomicU64::new(value.to_bits())));
    let pointer = Box::new(values.as_ptr() as usize);
    let addresses: Vec<_> = values
        .iter()
        .map(|v| format!("0x{:x}", v as *const AtomicU32 as usize))
        .collect();
    let double_addresses: Vec<_> = doubles
        .iter()
        .map(|v| format!("0x{:x}", v as *const AtomicU64 as usize))
        .collect();
    println!(
        "{}",
        serde_json::json!({ "pid": std::process::id(), "addresses": addresses, "double_addresses": double_addresses, "pointer": format!("0x{:x}", &*pointer as *const usize as usize) })
    );
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line == "quit" {
            break;
        }
        if line == "unavailable" {
            values[0].store(f32::NAN.to_bits(), Ordering::SeqCst);
            println!("unavailable");
            io::stdout().flush()?;
            continue;
        }
        let args: Vec<_> = line.split_whitespace().collect();
        ensure!(args.len() == 4 && args[0] == "set", "expected: set X Y Z");
        for index in 0..3 {
            let value: f32 = args[index + 1].parse()?;
            ensure!(value.is_finite(), "fixture values must be finite");
            values[index].store(value.to_bits(), Ordering::SeqCst);
            doubles[index].store(f64::from(value).to_bits(), Ordering::SeqCst);
        }
        println!("updated");
        io::stdout().flush()?;
    }
    // Keep the backing storage alive through the final command.
    std::hint::black_box((&values, &doubles, &pointer));
    Ok(())
}

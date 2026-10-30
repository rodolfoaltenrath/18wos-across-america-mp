//! Portable v2 client: real UDP and the production session machine, synthetic
//! coordinates only. No game, DLL, assets or engine callbacks are required.
use anyhow::{Result, ensure};
use clap::Parser;
use multiplayer_server::{
    identity,
    protocol::{MAX_PACKET, Position, Rotation, TICK_HZ},
    session_client::{Client, Event, Options, Phase, Status},
    session_protocol::{self as wire, Pose},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{self, Write},
    net::{SocketAddr, UdpSocket},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:39000")]
    server: SocketAddr,
    #[arg(long, default_value = "lan")]
    room: String,
    #[arg(long)]
    identity_file: PathBuf,
    #[arg(long,default_value=wire::PROFILE)]
    profile: String,
    #[arg(long,default_value_t=10,value_parser=clap::value_parser!(u64).range(1..=86400))]
    seconds: u64,
    #[arg(long, default_value_t = 0.0)]
    phase: f32,
    #[arg(long)]
    no_pose: bool,
    #[arg(long,value_parser=clap::value_parser!(u64).range(1..=86400000))]
    reconnect_after_ms: Option<u64>,
    #[arg(long)]
    output: Option<PathBuf>,
}
#[derive(Default, Serialize)]
struct Counts {
    sent: u64,
    received: u64,
    send_errors: u64,
    poses: u64,
    resets: u64,
    generation_changes: u64,
    roster_removals: u64,
    peak_remotes: usize,
}
#[derive(Serialize)]
struct Report {
    schema: u32,
    game_tested: bool,
    lan_tested: bool,
    status_before_close: Status,
    status: Status,
    timing: multiplayer_server::session_timing::Timing,
    counts: Counts,
    remotes_before_close: Vec<Pose>,
}
fn events(events: Vec<Event>, remotes: &mut BTreeMap<u32, Pose>, counts: &mut Counts) {
    for event in events {
        match event {
            Event::Reset => {
                remotes.clear();
                counts.resets += 1;
            }
            Event::Roster(ids) => {
                let before = remotes.len();
                remotes.retain(|id, _| ids.contains(id));
                counts.roster_removals += (before - remotes.len()) as u64;
            }
            Event::Pose { pose, .. } => {
                if remotes
                    .get(&pose.player)
                    .is_some_and(|p| p.generation != pose.generation)
                {
                    counts.generation_changes += 1;
                }
                remotes.insert(pose.player, pose);
                counts.poses += 1;
                counts.peak_remotes = counts.peak_remotes.max(remotes.len());
            }
        }
    }
}
fn transmit(socket: &UdpSocket, frame: &wire::ClientFrame, counts: &mut Counts) -> Result<()> {
    match socket.send(&wire::encode_client(frame).map_err(anyhow::Error::msg)?) {
        Ok(_) => counts.sent += 1,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionRefused
            ) || e.raw_os_error() == Some(10054) =>
        {
            counts.send_errors += 1
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn receive(
    socket: &UdpSocket,
    client: &mut Client,
    start: Instant,
    remotes: &mut BTreeMap<u32, Pose>,
    counts: &mut Counts,
) -> Result<()> {
    let mut buffer = [0; MAX_PACKET + 1];
    for _ in 0..256 {
        match socket.recv(&mut buffer) {
            Ok(len) => {
                counts.received += 1;
                events(
                    client.ingest(&buffer[..len], start.elapsed().as_millis() as u64),
                    remotes,
                    counts,
                );
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e)
                if matches!(e.raw_os_error(), Some(10040 | 10054))
                    || e.kind() == io::ErrorKind::ConnectionRefused =>
            {
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.phase.is_finite() && args.phase.abs() <= 1.0e7,
        "phase must be finite and within simulation limits"
    );
    ensure!(
        wire::room_valid(&args.room) && wire::profile_valid(&args.profile),
        "invalid room/profile"
    );
    ensure!(
        args.server.port() != 0
            && !args.server.ip().is_unspecified()
            && !args.server.ip().is_multicast(),
        "invalid server"
    );
    let options = Options {
        admission: None,
        room: args.room,
        profile: args.profile,
        identity: identity::load_or_create(&args.identity_file).map_err(anyhow::Error::msg)?,
    };
    let mut client = Client::new(
        options,
        identity::random_nonce().map_err(anyhow::Error::msg)?,
    )
    .map_err(anyhow::Error::msg)?;
    let socket = UdpSocket::bind(if args.server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    socket.connect(args.server)?;
    socket.set_nonblocking(true)?;
    let mut file = args
        .output
        .as_ref()
        .map(std::fs::File::create_new)
        .transpose()?;
    println!(
        "{}",
        serde_json::json!({"event":"ready","protocol":wire::VERSION,"local":socket.local_addr()?})
    );
    io::stdout().flush()?;
    let start = Instant::now();
    let tick = Duration::from_secs_f64(1.0 / f64::from(TICK_HZ));
    let mut next = start;
    let (mut counts, mut remotes, mut reconnected) = (Counts::default(), BTreeMap::new(), false);
    while start.elapsed() < Duration::from_secs(args.seconds) {
        let now = start.elapsed().as_millis() as u64;
        if !reconnected && args.reconnect_after_ms.is_some_and(|at| now >= at) && client.connected()
        {
            events(client.request_reconnect(), &mut remotes, &mut counts);
            reconnected = true;
        }
        let (frames, new_events) = client.poll(now);
        events(new_events, &mut remotes, &mut counts);
        for frame in frames {
            transmit(&socket, &frame, &mut counts)?;
        }
        if !args.no_pose
            && let Some(frame) = client.pose(
                Position {
                    x: args.phase + start.elapsed().as_secs_f32() * 10.0,
                    y: 0.0,
                    z: args.phase,
                },
                Some(Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                }),
                now,
            )
        {
            transmit(&socket, &frame, &mut counts)?;
        }
        receive(&socket, &mut client, start, &mut remotes, &mut counts)?;
        if client.status().phase == Phase::Rejected {
            break;
        }
        next += tick;
        let now = Instant::now();
        if next > now {
            thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    let status_before_close = client.status();
    let remotes_before_close = remotes.into_values().collect();
    let failed = status_before_close.phase != Phase::Connected || status_before_close.accepted == 0;
    client.close(start.elapsed().as_millis() as u64);
    let closing = Instant::now();
    let mut remotes = BTreeMap::new();
    while closing.elapsed() < Duration::from_millis(220) && client.status().phase != Phase::Closed {
        for frame in client.poll(start.elapsed().as_millis() as u64).0 {
            transmit(&socket, &frame, &mut counts)?;
        }
        receive(&socket, &mut client, start, &mut remotes, &mut counts)?;
        thread::sleep(Duration::from_millis(1));
    }
    let report = Report {
        schema: 1,
        game_tested: false,
        lan_tested: false,
        status_before_close,
        status: client.status(),
        timing: client.timing(),
        counts,
        remotes_before_close,
    };
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(file) = &mut file {
        file.write_all(&bytes)?;
        file.flush()?;
    }
    println!("{}", String::from_utf8(bytes)?);
    ensure!(
        !failed,
        "session was refused or never accepted; inspect the report"
    );
    Ok(())
}

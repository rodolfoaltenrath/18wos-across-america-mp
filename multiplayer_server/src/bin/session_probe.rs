//! Portable v2 client: real UDP and the production session machine, synthetic
//! coordinates by default. The optional loopback visibility cycle follows ONE
//! received player's pose. No game/DLL/memory/assets/engine access.
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
    /// Queue a reliable marker (codes 1, 2, ...) at this interval.
    #[arg(long,value_parser=clap::value_parser!(u64).range(50..=86400000))]
    marker_every_ms: Option<u64>,
    /// Lifetime of each queued marker.
    #[arg(long,default_value_t=wire::EVENT_TTL_MAX_MS,value_parser=clap::value_parser!(u64).range(1..=wire::EVENT_TTL_MAX_MS))]
    marker_ttl_ms: u64,
    /// Local-only 75 s visibility cycle, starting after a fresh target pose.
    #[arg(long, requires = "follow_player", conflicts_with_all=["no_pose","marker_every_ms","reconnect_after_ms"])]
    local_cycle: bool,
    /// Explicit real player id; the probe refuses to follow itself or an echo.
    #[arg(long, requires = "local_cycle")]
    follow_player: Option<u32>,
    /// World-space offset in game centimetres, used only by the local cycle.
    #[arg(long,num_args=3,allow_hyphen_values=true,default_values_t=[400.0f32,0.0,0.0],requires="local_cycle")]
    follow_offset: Vec<f32>,
}
impl Args {
    fn follower(&self) -> Result<Option<multiplayer_server::local_peer::Follower>> {
        if !self.local_cycle {
            return Ok(None);
        }
        ensure!(
            self.server.ip().is_loopback(),
            "local-cycle requires a loopback server"
        );
        ensure!(
            (80..=600).contains(&self.seconds),
            "local-cycle needs --seconds 80..=600 (75 s after first source pose)"
        );
        Ok(Some(
            multiplayer_server::local_peer::Follower::new(
                self.follow_player
                    .ok_or_else(|| anyhow::anyhow!("follow-player required"))?,
                self.follow_offset
                    .clone()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("three offsets required"))?,
            )
            .map_err(anyhow::Error::msg)?,
        ))
    }
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
    markers_queued: u64,
    markers_refused: u64,
    /// Distinct relayed events received, by origin player.
    relays_by_origin: BTreeMap<u32, u64>,
    states: u64,
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
    reliability: multiplayer_server::session_client::Reliability,
    #[serde(skip_serializing_if = "Option::is_none")]
    local_cycle: Option<multiplayer_server::local_peer::Summary>,
}
fn events(
    events: Vec<Event>,
    remotes: &mut BTreeMap<u32, Pose>,
    counts: &mut Counts,
    follower: &mut Option<multiplayer_server::local_peer::Follower>,
    now: u64,
) {
    for event in events {
        if let Some(follower) = follower {
            follower.event(&event, now);
        }
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
            Event::Relay { origin, body, .. } => {
                *counts.relays_by_origin.entry(origin).or_default() += 1;
                if matches!(body, wire::EventBody::State { .. }) {
                    counts.states += 1;
                }
            }
        }
    }
}
fn transmit(socket: &UdpSocket, frame: &wire::ClientFrame, counts: &mut Counts) -> Result<bool> {
    match socket.send(&wire::encode_client(frame).map_err(anyhow::Error::msg)?) {
        Ok(_) => {
            counts.sent += 1;
            return Ok(true);
        }
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
    Ok(false)
}
fn receive(
    socket: &UdpSocket,
    client: &mut Client,
    start: Instant,
    remotes: &mut BTreeMap<u32, Pose>,
    counts: &mut Counts,
    follower: &mut Option<multiplayer_server::local_peer::Follower>,
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
                    follower,
                    start.elapsed().as_millis() as u64,
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
    let mut follower = args.follower()?;
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
    let mut next_marker = args.marker_every_ms;
    let mut cycle_published = None;
    let mut cycle_stage = None;
    let mut connected_player = None;
    let mut invalid_target = false;
    while start.elapsed() < Duration::from_secs(args.seconds) {
        let now = start.elapsed().as_millis() as u64;
        if !reconnected && args.reconnect_after_ms.is_some_and(|at| now >= at) && client.connected()
        {
            events(
                client.request_reconnect(),
                &mut remotes,
                &mut counts,
                &mut follower,
                now,
            );
            reconnected = true;
        }
        if let Some(at) = next_marker.filter(|at| now >= *at) {
            let code = u32::try_from(counts.markers_queued + counts.markers_refused + 1)?;
            match client.event(wire::EventBody::Marker { code }, args.marker_ttl_ms, now) {
                Ok(_) => counts.markers_queued += 1,
                Err(_) => counts.markers_refused += 1,
            }
            next_marker = args.marker_every_ms.map(|every| at + every);
        }
        let (frames, new_events) = client.poll(now);
        events(new_events, &mut remotes, &mut counts, &mut follower, now);
        for frame in frames {
            transmit(&socket, &frame, &mut counts)?;
        }
        if client.connected() && connected_player != Some(client.status().player) {
            connected_player = Some(client.status().player);
            println!(
                "{}",
                serde_json::json!({"event":"connected","player":connected_player})
            );
            io::stdout().flush()?;
        }
        if let Some(follower) = &mut follower {
            if client.connected() {
                if args.follow_player == Some(client.status().player) {
                    invalid_target = true;
                    break;
                }
                let step = follower.step(now);
                if cycle_stage != step.stage {
                    cycle_stage = step.stage;
                    println!(
                        "{}",
                        serde_json::json!({"event":"local_cycle_stage","stage":step.stage,
                        "state":step.state,"elapsed_ms":follower.summary().elapsed_ms,"source_available":step.source_available})
                    );
                    io::stdout().flush()?;
                    if let Some(stage) = step.stage {
                        match client.event(
                            wire::EventBody::Marker {
                                code: 1000 + u32::from(stage),
                            },
                            30_000,
                            now,
                        ) {
                            Ok(_) => counts.markers_queued += 1,
                            Err(_) => counts.markers_refused += 1,
                        }
                    }
                }
                let connection = client.status().connection;
                let due = cycle_published.is_none_or(|(old, state, at)| {
                    old != connection || state != step.state || now.saturating_sub(at) >= 3000
                });
                if due
                    && client
                        .event(wire::EventBody::State { state: step.state }, 10_000, now)
                        .is_ok()
                {
                    cycle_published = Some((connection, step.state, now));
                }
                if let Some((position, rotation)) = step.pose
                    && let Some(frame) = client.pose(position, Some(rotation), now)
                    && transmit(&socket, &frame, &mut counts)?
                {
                    follower.sent(step.stage.unwrap());
                }
            }
        } else if !args.no_pose
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
        receive(
            &socket,
            &mut client,
            start,
            &mut remotes,
            &mut counts,
            &mut follower,
        )?;
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
    let failed = status_before_close.phase != Phase::Connected
        || status_before_close.accepted == 0
        || invalid_target
        || follower.as_ref().is_some_and(|f| !f.summary().complete);
    client.close(start.elapsed().as_millis() as u64);
    let closing = Instant::now();
    let mut remotes = BTreeMap::new();
    while closing.elapsed() < Duration::from_millis(220) && client.status().phase != Phase::Closed {
        for frame in client.poll(start.elapsed().as_millis() as u64).0 {
            transmit(&socket, &frame, &mut counts)?;
        }
        receive(
            &socket,
            &mut client,
            start,
            &mut remotes,
            &mut counts,
            &mut follower,
        )?;
        thread::sleep(Duration::from_millis(1));
    }
    let report = Report {
        schema: if args.local_cycle { 3 } else { 2 },
        game_tested: false,
        lan_tested: false,
        status_before_close,
        status: client.status(),
        timing: client.timing(),
        counts,
        remotes_before_close,
        reliability: client.reliability(),
        local_cycle: follower.map(|f| f.into_summary()),
    };
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(file) = &mut file {
        file.write_all(&bytes)?;
        file.flush()?;
    }
    println!("{}", String::from_utf8(bytes)?);
    ensure!(
        !failed,
        "session refused, invalid follow target or incomplete local cycle; inspect the report"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_cycle_requires_explicit_pairing_without_breaking_the_legacy_default() {
        let base = ["session_probe", "--identity-file", "own-fixture.id"];
        let legacy = Args::try_parse_from(base).unwrap();
        assert!(legacy.follower().unwrap().is_none());
        assert!(Args::try_parse_from(base.into_iter().chain(["--local-cycle"])).is_err());
        assert!(Args::try_parse_from(base.into_iter().chain(["--follow-player", "1"])).is_err());
        let args = Args::try_parse_from(base.into_iter().chain([
            "--local-cycle",
            "--follow-player",
            "1",
            "--seconds",
            "80",
        ]))
        .unwrap();
        assert!(args.follower().unwrap().is_some());
    }
    #[test]
    fn rejects_lan_endpoint_short_window_and_bad_offsets_before_creating_identity_or_socket() {
        for extra in [
            vec!["--server", "192.0.2.1:39000"],
            vec!["--seconds", "10"],
            vec!["--follow-offset", "NaN", "0", "0"],
            vec!["--follow-player", "1000000000"],
        ] {
            // Override defaults in a single parse rather than accepting repeated flags.
            let mut argv = vec![
                "session_probe",
                "--identity-file",
                "never-created.id",
                "--local-cycle",
            ];
            if !extra.contains(&"--follow-player") {
                argv.extend(["--follow-player", "1"]);
            }
            if !extra.contains(&"--seconds") {
                argv.extend(["--seconds", "80"]);
            }
            argv.extend(extra);
            let args = Args::try_parse_from(argv).unwrap();
            assert!(args.follower().is_err());
        }
    }
}

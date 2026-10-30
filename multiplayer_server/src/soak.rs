//! Bounded real-UDP endurance driver. All peers and the production Tokio room
//! share this process; no game, public endpoint, identity files or injection.
use crate::{
    identity,
    protocol::{MAX_PACKET, Position, Rotation, TICK_HZ},
    resources,
    session_client::{Client, Event, Options, Status},
    session_hub::Settings,
    session_protocol as wire, session_server,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io,
    net::{SocketAddr, UdpSocket},
    time::Duration,
};
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::{self, Instant, MissedTickBehavior},
};

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub clients: usize,
    pub duration_ms: u64,
    pub restart_every_ms: Option<u64>,
    pub outage_ms: u64,
    pub churn_every_ms: Option<u64>,
    pub abrupt_restarts: bool,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (2..=wire::MAX_PLAYERS).contains(&self.clients),
            "clients must be 2..=64"
        );
        ensure!(
            (1000..=3_600_000).contains(&self.duration_ms),
            "duration must be 1..=3600 seconds"
        );
        ensure!(self.outage_ms <= 10000, "outage exceeds 10 seconds");
        ensure!(
            !self.abrupt_restarts || self.restart_every_ms.is_some(),
            "abrupt restarts require a restart interval"
        );
        if let Some(n) = self.restart_every_ms {
            ensure!(
                n >= 1000
                    && n > self.outage_ms + 500
                    && n.checked_add(self.outage_ms)
                        .and_then(|total| total.checked_add(1000))
                        .is_some_and(|total| total < self.duration_ms),
                "restart must allow an outage and at least one second of recovery inside the run"
            );
        }
        ensure!(
            self.churn_every_ms
                .is_none_or(|n| n >= 250 && n < self.duration_ms),
            "invalid churn interval"
        );
        Ok(())
    }
}

#[derive(Default, Debug, Serialize)]
pub struct Counts {
    pub incarnations: u64,
    pub accepted: u64,
    pub sent: u64,
    pub received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub io_drops: u64,
    pub poses: u64,
    pub resets: u64,
    pub graceful_churn: u64,
    pub abrupt_churn: u64,
    pub resumes: u64,
    pub replacement_identities: u64,
    pub confirmed_churn_leaves: u64,
    pub peak_remotes: usize,
    pub unexpected_poses: u64,
    pub invalid_frames: u64,
    pub refusals: u64,
    pub max_datagram: usize,
}
#[derive(Serialize)]
pub struct PeerReport {
    pub slot: usize,
    pub counts: Counts,
    pub before_close: Status,
    pub after_close: Status,
}
#[derive(Serialize)]
pub struct Epoch {
    pub start_ms: u64,
    pub end_ms: u64,
    pub first_full_mesh_ms: Option<u64>,
    pub termination: &'static str,
    /// None for task abort: no final server metrics were captured.
    pub server: Option<session_server::Report>,
}
#[derive(Serialize)]
pub struct ResourcePoint {
    pub elapsed_ms: u64,
    pub process: resources::Sample,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: u8,
    pub game_tested: bool,
    pub lan_tested: bool,
    pub scope: &'static str,
    pub config: Config,
    pub elapsed_ms: u64,
    pub passed: bool,
    pub issues: Vec<String>,
    pub driver_late_ticks: u64,
    /// Due churn ticks postponed until the production clients observe a full
    /// mesh. The requested interval is a minimum spacing, not a promised rate.
    pub churn_wait_ticks: u64,
    pub full_mesh_before_close: bool,
    pub cpu_elapsed_ms: Option<u64>,
    /// 10000 = 100% of one core; can exceed that on a multithreaded runtime.
    pub cpu_basis_points: Option<u64>,
    pub sampled_peak_resident_bytes: Option<u64>,
    pub resources: Vec<ResourcePoint>,
    pub epochs: Vec<Epoch>,
    pub peers: Vec<PeerReport>,
}

struct Server {
    endpoint: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<Result<session_server::Report>>>,
}
impl Server {
    async fn start(endpoint: SocketAddr, clients: usize) -> Result<Self> {
        ensure!(endpoint.ip().is_loopback(), "soak only binds loopback");
        let socket = tokio::net::UdpSocket::bind(endpoint).await?;
        let endpoint = socket.local_addr()?;
        let (stop, stopped) = oneshot::channel();
        let join = tokio::spawn(session_server::run(
            socket,
            Settings {
                room: "soak".into(),
                profile: wire::PROFILE.into(),
                max_clients: clients,
                timeout_ms: 2000,
                echo: false,
            },
            async {
                let _ = stopped.await;
            },
        ));
        Ok(Self {
            endpoint,
            stop: Some(stop),
            join: Some(join),
        })
    }
    async fn finish(mut self, abrupt: bool) -> Result<Option<session_server::Report>> {
        let join = self.join.as_mut().context("server task missing")?;
        if abrupt {
            join.abort();
        } else if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let outcome = time::timeout(Duration::from_secs(3), join)
            .await
            .context("server stop timed out")?;
        self.join.take();
        match outcome {
            Err(e) if abrupt && e.is_cancelled() => Ok(None),
            Err(e) => Err(e.into()),
            Ok(result) => result.map(Some),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(join) = &self.join {
            join.abort();
        }
    }
}

struct Peer {
    slot: usize,
    options: Options,
    client: Client,
    socket: Option<UdpSocket>,
    remotes: BTreeMap<u32, (wire::Pose, u64)>,
    counts: Counts,
    seen_accepts: u64,
    seen_invalid: u64,
    seen_refusal: bool,
    rejoin_at: Option<u64>,
    churn_closing: bool,
    replace_identity: bool,
}
fn client(options: &Options) -> Result<Client> {
    Client::new(
        options.clone(),
        identity::random_nonce().map_err(anyhow::Error::msg)?,
    )
    .map_err(anyhow::Error::msg)
}
fn socket(endpoint: SocketAddr) -> Result<UdpSocket> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.connect(endpoint)?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}
impl Peer {
    fn new(slot: usize, endpoint: SocketAddr) -> Result<Self> {
        let options = Options {
            admission: None,
            room: "soak".into(),
            profile: wire::PROFILE.into(),
            identity: identity::random_id().map_err(anyhow::Error::msg)?,
        };
        Ok(Self {
            slot,
            client: client(&options)?,
            options,
            socket: Some(socket(endpoint)?),
            remotes: BTreeMap::new(),
            counts: Counts {
                incarnations: 1,
                ..Counts::default()
            },
            seen_accepts: 0,
            seen_invalid: 0,
            seen_refusal: false,
            rejoin_at: None,
            churn_closing: false,
            replace_identity: false,
        })
    }
    fn events(&mut self, events: Vec<Event>, now: u64) {
        for event in events {
            match event {
                Event::Reset => {
                    self.remotes.clear();
                    self.counts.resets += 1;
                }
                Event::Roster(ids) => self.remotes.retain(|id, _| ids.contains(id)),
                Event::Pose { pose, .. } => {
                    if pose.player == self.client.status().player || !pose.position.is_valid() {
                        self.counts.unexpected_poses += 1;
                    }
                    self.remotes.insert(pose.player, (pose, now));
                    self.counts.poses += 1;
                    self.counts.peak_remotes = self.counts.peak_remotes.max(self.remotes.len());
                }
            }
        }
    }
    fn transmit(&mut self, frame: &wire::ClientFrame) -> Result<()> {
        let bytes = wire::encode_client(frame).map_err(anyhow::Error::msg)?;
        self.counts.max_datagram = self.counts.max_datagram.max(bytes.len());
        match self
            .socket
            .as_ref()
            .context("offline peer send")?
            .send(&bytes)
        {
            Ok(n) => {
                self.counts.sent += 1;
                self.counts.bytes_sent += n as u64;
            }
            Err(e) if transient(&e) => self.counts.io_drops += 1,
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    fn pump(&mut self, endpoint: SocketAddr, now: u64, send_pose: bool) -> Result<()> {
        if self.rejoin_at.is_some_and(|at| now >= at) {
            if self.replace_identity {
                self.options.identity = identity::random_id().map_err(anyhow::Error::msg)?;
                self.replace_identity = false;
            }
            self.client = client(&self.options)?;
            self.socket = Some(socket(endpoint)?);
            self.seen_accepts = 0;
            self.seen_invalid = 0;
            self.seen_refusal = false;
            self.rejoin_at = None;
            self.counts.incarnations += 1;
        }
        if self.socket.is_none() {
            return Ok(());
        }
        let (frames, events) = self.client.poll(now);
        self.events(events, now);
        for frame in frames {
            self.transmit(&frame)?;
        }
        if send_pose
            && let Some(frame) = self.client.pose(
                Position {
                    x: self.slot as f32 * 1000.0 + (now % 60000) as f32,
                    y: 0.0,
                    z: self.slot as f32 * 100.0,
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
            self.transmit(&frame)?;
        }
        let mut buffer = [0; MAX_PACKET + 1];
        for _ in 0..256 {
            match self.socket.as_ref().unwrap().recv(&mut buffer) {
                Ok(n) => {
                    self.counts.received += 1;
                    self.counts.bytes_received += n as u64;
                    self.counts.max_datagram = self.counts.max_datagram.max(n);
                    let events = self.client.ingest(&buffer[..n], now);
                    self.events(events, now);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if transient(&e) => {
                    self.counts.io_drops += 1;
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
        let status = self.client.status();
        self.counts.accepted += status.accepted.saturating_sub(self.seen_accepts);
        self.seen_accepts = status.accepted;
        self.counts.invalid_frames += status.invalid.saturating_sub(self.seen_invalid);
        self.seen_invalid = status.invalid;
        if status.refusal.is_some() && !self.seen_refusal {
            self.counts.refusals += 1;
            self.seen_refusal = true;
        }
        if self.churn_closing && status.phase == crate::session_client::Phase::Closed {
            self.counts.confirmed_churn_leaves += u64::from(status.leave_confirmed);
            self.socket = None;
            self.remotes.clear();
            self.churn_closing = false;
            self.rejoin_at = Some(now + 200);
        }
        Ok(())
    }
    fn churn(&mut self, kind: usize, now: u64) {
        if self.socket.is_none() || !self.client.connected() || self.churn_closing {
            return;
        }
        match kind % 4 {
            0 | 3 => {
                self.counts.graceful_churn += 1;
                self.replace_identity = kind % 4 == 3;
                self.counts.replacement_identities += u64::from(self.replace_identity);
                self.client.close(now);
                self.churn_closing = true;
            }
            1 => {
                self.counts.abrupt_churn += 1;
                self.socket = None;
                self.remotes.clear();
                self.rejoin_at = Some(now + 2500);
            }
            _ => {
                self.counts.resumes += 1;
                let events = self.client.request_reconnect();
                self.events(events, now);
            }
        }
    }
}
fn transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
    ) || error.raw_os_error() == Some(10054)
}
fn full_mesh(peers: &[Peer], now: u64, previous_session: u64) -> bool {
    let session = peers[0].client.status().session;
    session != 0
        && session != previous_session
        && peers.iter().all(|p| {
            p.socket.is_some()
                && p.client.connected()
                && p.client.status().session == session
                && p.remotes.len() == peers.len() - 1
                && p.remotes
                    .values()
                    .all(|(_, at)| now.saturating_sub(*at) < 2000)
        })
}
/// Rotate both failure mode and peer. Grouping by all peers first can keep
/// someone intentionally offline for an entire epoch, making its witness
/// impossible even when every individual recovery succeeds.
fn churn_target(round: usize, peers: usize) -> (usize, usize) {
    let kind = round % 4;
    ((round / 4 + kind) % peers, kind)
}

pub async fn run(config: Config) -> Result<Report> {
    config.validate()?;
    let start = Instant::now();
    let initial = resources::sample();
    let mut points = vec![ResourcePoint {
        elapsed_ms: 0,
        process: initial,
    }];
    let mut server = Some(Server::start("127.0.0.1:0".parse()?, config.clients).await?);
    let endpoint = server.as_ref().unwrap().endpoint;
    let mut peers = (0..config.clients)
        .map(|n| Peer::new(n, endpoint))
        .collect::<Result<Vec<_>>>()?;
    let mut interval = time::interval(Duration::from_secs_f64(1.0 / f64::from(TICK_HZ)));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let (mut next_restart, mut next_churn) = (
        config.restart_every_ms.unwrap_or(u64::MAX),
        config.churn_every_ms.unwrap_or(u64::MAX),
    );
    let (mut next_sample, mut next_progress) = (1000, 30000);
    let (mut rebind_at, mut previous_session) = (None, 0);
    let (mut epoch_start, mut first_mesh) = (0, None);
    let (mut epochs, mut issues) = (Vec::new(), Vec::new());
    let (mut churn_round, mut late, mut previous_tick) = (0usize, 0u64, 0u64);
    let mut churn_wait_ticks = 0;
    let converged;
    loop {
        interval.tick().await;
        let now = start.elapsed().as_millis() as u64;
        if now.saturating_sub(previous_tick) > 34 {
            late += 1;
        }
        previous_tick = now;
        let settling = now >= config.duration_ms;
        if !settling
            && now >= next_restart
            && server.is_some()
            && now + config.outage_ms + 1000 < config.duration_ms
        {
            let report = server
                .take()
                .unwrap()
                .finish(config.abrupt_restarts)
                .await?;
            previous_session = report.as_ref().map(|r| r.session).unwrap_or_else(|| {
                peers
                    .iter()
                    .map(|p| p.client.status().session)
                    .find(|s| *s != 0)
                    .unwrap_or(0)
            });
            epochs.push(Epoch {
                start_ms: epoch_start,
                end_ms: now,
                first_full_mesh_ms: first_mesh,
                termination: if config.abrupt_restarts {
                    "aborted_restart"
                } else {
                    "graceful_restart"
                },
                server: report,
            });
            rebind_at = Some(now + config.outage_ms);
            next_restart = now + config.restart_every_ms.unwrap();
            first_mesh = None;
        }
        if server.is_none() && (settling || rebind_at.is_some_and(|at| now >= at)) {
            server = Some(Server::start(endpoint, config.clients).await?);
            rebind_at = None;
            epoch_start = now;
        }
        if !settling && now >= next_churn {
            // Allow recovery to be observed before intentionally interrupting
            // another peer, especially in a freshly rebound server epoch.
            if server.is_some() && full_mesh(&peers, now, previous_session) {
                let (index, kind) = churn_target(churn_round, peers.len());
                peers[index].churn(kind, now);
                churn_round += 1;
                next_churn = now + config.churn_every_ms.unwrap();
            } else {
                churn_wait_ticks += 1;
            }
        }
        for peer in &mut peers {
            peer.pump(endpoint, now, true)?;
        }
        let mesh = server.is_some() && full_mesh(&peers, now, previous_session);
        if mesh && first_mesh.is_none() {
            first_mesh = Some(now);
        }
        if now >= next_sample {
            points.push(ResourcePoint {
                elapsed_ms: now,
                process: resources::sample(),
            });
            next_sample = now + 1000;
        }
        if now >= next_progress {
            println!(
                "{}",
                serde_json::json!({"event":"soak_progress","elapsed_ms":now,
                "connected":peers.iter().filter(|p| p.client.connected() && p.socket.is_some()).count(),
                "completed_epochs":epochs.len(),"full_mesh":mesh})
            );
            next_progress = now + 30000;
        }
        if settling && mesh {
            converged = true;
            break;
        }
        if now >= config.duration_ms + 6000 {
            converged = false;
            break;
        }
    }
    if !converged {
        issues.push("final_mesh_did_not_converge".into());
    }
    let before: Vec<_> = peers.iter().map(|p| p.client.status()).collect();
    let now = start.elapsed().as_millis() as u64;
    for peer in &mut peers {
        peer.churn_closing = false;
        peer.rejoin_at = None;
        peer.client.close(now);
    }
    let closing = Instant::now();
    while closing.elapsed() < Duration::from_millis(600) {
        let now = start.elapsed().as_millis() as u64;
        for peer in &mut peers {
            peer.pump(endpoint, now, false)?;
        }
        time::sleep(Duration::from_millis(1)).await;
    }
    let now = start.elapsed().as_millis() as u64;
    let final_server = server
        .take()
        .context("final server missing")?
        .finish(false)
        .await?
        .context("missing final server report")?;
    if final_server.active_on_shutdown != 0 {
        issues.push("server_retained_active_peers".into());
    }
    epochs.push(Epoch {
        start_ms: epoch_start,
        end_ms: now,
        first_full_mesh_ms: first_mesh,
        termination: "final_stop",
        server: Some(final_server),
    });
    for (i, peer) in peers.iter().enumerate() {
        let after = peer.client.status();
        if !after.leave_confirmed {
            issues.push(format!("peer_{i}_leave_unconfirmed"));
        }
        if peer.counts.accepted == 0 || peer.counts.poses == 0 {
            issues.push(format!("peer_{i}_missing_traffic"));
        }
        if peer.counts.unexpected_poses > 0
            || peer.counts.invalid_frames > 0
            || peer.counts.refusals > 0
            || peer.counts.peak_remotes > wire::MAX_PLAYERS
            || peer.counts.max_datagram > MAX_PACKET
            || after.invalid > 0
            || after.refusal.is_some()
        {
            issues.push(format!("peer_{i}_invalid_state"));
        }
    }
    for (i, epoch) in epochs.iter().enumerate() {
        if epoch.first_full_mesh_ms.is_none() {
            issues.push(format!("epoch_{i}_no_full_mesh"));
        }
        if let Some(report) = &epoch.server
            && (report.cached_members > wire::MAX_PLAYERS
                || report.metrics.invalid > 0
                || report.metrics.refused > 0)
        {
            issues.push(format!("epoch_{i}_invalid_server_state"));
        }
    }
    let peers = peers
        .into_iter()
        .zip(before)
        .map(|(p, before_close)| PeerReport {
            slot: p.slot,
            after_close: p.client.status(),
            before_close,
            counts: p.counts,
        })
        .collect();
    let final_sample = resources::sample();
    let elapsed = start.elapsed().as_millis() as u64;
    points.push(ResourcePoint {
        elapsed_ms: elapsed,
        process: final_sample,
    });
    let cpu_elapsed_ms = initial
        .cpu_ms
        .zip(final_sample.cpu_ms)
        .map(|(a, b)| b.saturating_sub(a));
    Ok(Report {
        schema: 1,
        game_tested: false,
        lan_tested: false,
        scope: "single_process_loopback_production_server_and_clients",
        config,
        elapsed_ms: elapsed,
        passed: issues.is_empty(),
        issues,
        driver_late_ticks: late,
        churn_wait_ticks,
        full_mesh_before_close: converged,
        cpu_elapsed_ms,
        cpu_basis_points: cpu_elapsed_ms.map(|ms| ms.saturating_mul(10000) / elapsed.max(1)),
        sampled_peak_resident_bytes: points.iter().filter_map(|p| p.process.resident_bytes).max(),
        resources: points,
        epochs,
        peers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            clients: 3,
            duration_ms: 1000,
            restart_every_ms: None,
            outage_ms: 0,
            churn_every_ms: None,
            abrupt_restarts: false,
        }
    }
    #[test]
    fn invalid_limits_are_rejected_before_binding() {
        let mut c = config();
        c.clients = 65;
        assert!(c.validate().is_err());
        let mut c = config();
        c.restart_every_ms = Some(1000);
        assert!(c.validate().is_err());
        let mut c = config();
        c.churn_every_ms = Some(0);
        assert!(c.validate().is_err());
        let mut c = config();
        c.restart_every_ms = Some(u64::MAX);
        assert!(c.validate().is_err());
        let mut c = config();
        c.abrupt_restarts = true;
        assert!(c.validate().is_err());
    }
    #[test]
    fn churn_rotates_modes_and_covers_every_peer_without_a_long_group_of_overlapping_drops() {
        for peers in [2, 3, 16, 64] {
            let mut observed = vec![[0u32; 4]; peers];
            for round in 0..peers * 4 {
                let (peer, kind) = churn_target(round, peers);
                assert_eq!(kind, round % 4);
                observed[peer][kind] += 1;
            }
            assert!(observed.iter().all(|counts| *counts == [1; 4]));
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_room_mesh_and_confirmed_cleanup_are_observed_with_real_udp() {
        let report = run(config()).await.unwrap();
        assert!(report.passed, "{:?}", report.issues);
        assert_eq!(report.peers.len(), 3);
        assert!(
            report
                .peers
                .iter()
                .all(|p| p.counts.peak_remotes == 2 && p.after_close.leave_confirmed)
        );
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("credentials")
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn aborted_room_and_rotating_churn_recover_without_abandoned_tasks() {
        let mut c = config();
        c.clients = 4;
        c.duration_ms = 10000;
        c.restart_every_ms = Some(5000);
        c.outage_ms = 2200;
        c.churn_every_ms = Some(300);
        c.abrupt_restarts = true;
        let report = run(c).await.unwrap();
        assert!(report.passed, "{:?}", report.issues);
        assert!(
            report
                .epochs
                .iter()
                .any(|e| e.termination == "aborted_restart" && e.server.is_none())
        );
        assert!(
            report
                .peers
                .iter()
                .map(|p| p.counts.abrupt_churn)
                .sum::<u64>()
                > 0
        );
        assert!(report.peers.iter().map(|p| p.counts.resumes).sum::<u64>() > 0);
        assert!(report.churn_wait_ticks > 0);
    }
}

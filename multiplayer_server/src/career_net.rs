//! Explicit LAN bootstrap control plane, separate from the proven UDP pose wire.
//! Bounded frames/peers/time and a frozen blob; no engine access or live-save writes.
pub use crate::career_entry::Receipt;
use crate::career_entry::{Launch, Registry};
use crate::{
    career::{self, Compatibility, Manifest},
    career_store as store,
    career_world::{Snapshot, WorldStore},
    identity, session_protocol,
};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    future::Future,
    net::SocketAddr,
    path::Path,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};

const MAX_FRAME: usize = 256 * 1024;
const DEADLINE: Duration = Duration::from_secs(30);
pub fn inspect_import(path: &Path) -> Result<Receipt> {
    crate::career_entry::inspect_import(path).map_err(anyhow::Error::msg)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invite {
    pub schema: u32,
    pub career: String,
    pub manifest_sha256: String,
    pub secret: String,
}
impl Invite {
    pub fn new(manifest: &Manifest) -> Result<Self> {
        Ok(Self {
            schema: 1,
            career: manifest.career.clone(),
            manifest_sha256: career::hash(&store::json(manifest).map_err(anyhow::Error::msg)?),
            secret: identity::random_id().map_err(anyhow::Error::msg)?,
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == 1
                && session_protocol::identity_valid(&self.career)
                && session_protocol::identity_valid(&self.secret)
                && career::hash_valid(&self.manifest_sha256),
            "invalid invitation"
        );
        Ok(())
    }
    pub fn check(&self, manifest: &Manifest) -> Result<()> {
        self.validate()?;
        manifest.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            manifest.career == self.career
                && career::hash(&store::json(manifest).map_err(anyhow::Error::msg)?)
                    == self.manifest_sha256,
            "host manifest differs from pinned invitation"
        );
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u32,
    career: String,
    secret: String,
    compatibility: Compatibility,
    #[serde(default)]
    action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    admission: Option<AdmissionRequest>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionRequest {
    identity: String,
    receipt: Receipt,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    #[default]
    Bootstrap,
    Status,
    Admit,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Response {
    Admission {
        manifest: Manifest,
        launch: Box<Launch>,
    },
    Bootstrap {
        manifest: Manifest,
    },
    World {
        manifest: Manifest,
        snapshot: Box<Snapshot>,
    },
    Refused {
        reason: String,
    },
}

async fn send<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_FRAME, "control frame exceeds limit");
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}
async fn receive<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    let len = stream.read_u32().await? as usize;
    ensure!(len > 0 && len <= MAX_FRAME, "invalid control frame size");
    let mut bytes = vec![0; len];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid control frame"))
}
fn secret_matches(left: &str, right: &str) -> bool {
    // Fixed-size valid hex secrets. Avoid early byte mismatch returns.
    if left.len() != 32 || right.len() != 32 {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
async fn handle(
    mut stream: TcpStream,
    invite: &Invite,
    manifest: &Manifest,
    save: &[u8],
    world: &RwLock<Snapshot>,
    admission: Option<&Registry>,
) -> Result<()> {
    let request: Request = receive(&mut stream).await?;
    let refusal = if request.schema != 1
        || request.career != invite.career
        || !secret_matches(&request.secret, &invite.secret)
    {
        Some("invalid career invitation".to_owned())
    } else {
        manifest.compatibility.compare(&request.compatibility).err()
    };
    if let Some(reason) = refusal {
        send(&mut stream, &Response::Refused { reason }).await?;
        return Ok(());
    }
    if matches!(request.action, Action::Admit) {
        let result = match (admission, request.admission) {
            (Some(registry), Some(claim)) => registry.issue(&claim.identity, &claim.receipt),
            _ => Err("career relay admission unavailable".into()),
        };
        match result {
            Ok(launch) => {
                send(
                    &mut stream,
                    &Response::Admission {
                        manifest: manifest.clone(),
                        launch: Box::new(launch),
                    },
                )
                .await?
            }
            Err(reason) => send(&mut stream, &Response::Refused { reason }).await?,
        }
        stream.shutdown().await?;
        return Ok(());
    }
    if matches!(request.action, Action::Status) {
        let snapshot = world
            .read()
            .map_err(|_| anyhow::anyhow!("world snapshot unavailable"))?
            .clone();
        send(
            &mut stream,
            &Response::World {
                manifest: manifest.clone(),
                snapshot: Box::new(snapshot),
            },
        )
        .await?;
        stream.shutdown().await?;
        return Ok(());
    }
    send(
        &mut stream,
        &Response::Bootstrap {
            manifest: manifest.clone(),
        },
    )
    .await?;
    stream.write_all(save).await?;
    stream.shutdown().await?;
    Ok(())
}
#[derive(Default, Serialize)]
pub struct Report {
    pub schema: u32,
    pub connections: u64,
    pub completed_connections: u64,
    pub failed_connections: u64,
    pub busy: u64,
    pub game_tested: bool,
}
pub async fn serve(
    listener: TcpListener,
    data: &Path,
    shutdown: impl Future<Output = ()>,
) -> Result<Report> {
    serve_with_admission(listener, data, None, shutdown).await
}
pub async fn serve_with_admission(
    listener: TcpListener,
    data: &Path,
    admission: Option<Registry>,
    shutdown: impl Future<Output = ()>,
) -> Result<Report> {
    let mut authority = WorldStore::open(data).map_err(anyhow::Error::msg)?;
    let (manifest, save) = store::load(data).map_err(anyhow::Error::msg)?;
    let invite: Invite = store::read_json(&data.join("invite.json")).map_err(anyhow::Error::msg)?;
    invite.check(&manifest)?;
    if let Some(registry) = &admission {
        registry
            .check_context(&manifest, None, None)
            .map_err(anyhow::Error::msg)?;
    }
    let manifest = Arc::new(manifest);
    let save = Arc::new(save);
    let invite = Arc::new(invite);
    let world = Arc::new(RwLock::new(authority.snapshot()));
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut advanced = tokio::time::Instant::now();
    let mut persisted = advanced;
    let slots = Arc::new(Semaphore::new(8));
    let mut tasks = JoinSet::new();
    let mut report = Report {
        schema: 1,
        ..Report::default()
    };
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tasks.abort_all(); while tasks.join_next().await.is_some() {}
                authority.advance(u64::try_from(advanced.elapsed().as_millis())?).map_err(anyhow::Error::msg)?;
                authority.flush().map_err(anyhow::Error::msg)?;
                return Ok(report);
            }
            _ = tick.tick() => {
                let now = tokio::time::Instant::now();
                let elapsed = u64::try_from(now.duration_since(advanced).as_millis())?;
                advanced += Duration::from_millis(elapsed);
                authority.advance(elapsed).map_err(anyhow::Error::msg)?;
                if now.duration_since(persisted) >= Duration::from_secs(30) { authority.flush().map_err(anyhow::Error::msg)?; persisted = now; }
                *world.write().map_err(|_| anyhow::anyhow!("world snapshot unavailable"))? = authority.snapshot();
            }
            result = tasks.join_next(), if !tasks.is_empty() => {
                match result { Some(Ok(Ok(Ok(())))) => report.completed_connections += 1, _ => report.failed_connections += 1 }
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?; report.connections += 1;
                let Ok(slot) = slots.clone().try_acquire_owned() else { report.busy += 1; continue; };
                let invite = invite.clone(); let manifest = manifest.clone(); let save = save.clone(); let world = world.clone();
                let admission = admission.clone();
                tasks.spawn(async move { let _slot = slot; tokio::time::timeout(DEADLINE, handle(stream, &invite, &manifest, &save, &world, admission.as_ref())).await });
            }
        }
    }
}
/// Validates host/rules before creating output. A completed receipt is written
/// last; interrupted output is explicitly incomplete and never auto-loaded.
pub async fn fetch(
    endpoint: SocketAddr,
    invite: &Invite,
    compatibility: Compatibility,
    output: &Path,
) -> Result<Receipt> {
    invite.validate()?;
    compatibility.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        endpoint.port() != 0 && !endpoint.ip().is_unspecified() && !endpoint.ip().is_multicast(),
        "invalid host endpoint"
    );
    ensure!(
        !output.try_exists()?,
        "destination already exists; choose a fresh folder"
    );
    tokio::time::timeout(DEADLINE, async {
        let mut stream = TcpStream::connect(endpoint).await?;
        send(&mut stream, &Request { schema: 1, career: invite.career.clone(), secret: invite.secret.clone(), compatibility: compatibility.clone(), action: Action::Bootstrap, admission: None }).await?;
        let manifest = match receive(&mut stream).await? {
            Response::Bootstrap { manifest } => manifest,
            Response::Refused { reason } => bail!("host refused bootstrap: {reason}"),
            Response::World { .. } => bail!("unexpected world response to bootstrap"),
            Response::Admission { .. } => bail!("unexpected admission response to bootstrap"),
        };
        invite.check(&manifest)?;
        manifest.compatibility.compare(&compatibility).map_err(anyhow::Error::msg)?;
        std::fs::create_dir(output)?;
        store::write_new(&output.join("INCOMPLETE"), b"Do not load: transfer has not been committed.\n").map_err(anyhow::Error::msg)?;
        // Bounded to 64 MiB by Manifest; no archive entries or remote filenames.
        let mut bytes = vec![0; manifest.bootstrap.bytes as usize];
        stream.read_exact(&mut bytes).await?;
        let mut extra = [0; 1];
        ensure!(stream.read(&mut extra).await? == 0, "extra bytes after host save");
        manifest.verify_save(&bytes).map_err(anyhow::Error::msg)?;
        store::write_new(&output.join("bootstrap.sav"), &bytes).map_err(anyhow::Error::msg)?;
        store::write_new(&output.join("manifest.json"), &store::json(&manifest).map_err(anyhow::Error::msg)?).map_err(anyhow::Error::msg)?;
        let receipt = Receipt { schema: 1, career: manifest.career, manifest_sha256: invite.manifest_sha256.clone(), save_sha256: manifest.bootstrap.sha256, bytes: bytes.len() as u64, bootstrap_verified: true, engine_loaded: false, world_synchronized: false };
        store::write_new(&output.join("receipt.json"), &store::json(&receipt).map_err(anyhow::Error::msg)?).map_err(anyhow::Error::msg)?;
        std::fs::remove_file(output.join("INCOMPLETE"))?;
        store::sync_dir(output).map_err(anyhow::Error::msg)?;
        Ok(receipt)
    }).await.map_err(|_| anyhow::anyhow!("bootstrap timed out; any destination without a complete verified receipt must not be loaded"))?
}
pub async fn world_status(
    endpoint: SocketAddr,
    invite: &Invite,
    compatibility: Compatibility,
) -> Result<Snapshot> {
    invite.validate()?;
    compatibility.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        endpoint.port() != 0 && !endpoint.ip().is_unspecified() && !endpoint.ip().is_multicast(),
        "invalid host endpoint"
    );
    tokio::time::timeout(DEADLINE, async {
        let mut stream = TcpStream::connect(endpoint).await?;
        send(
            &mut stream,
            &Request {
                schema: 1,
                career: invite.career.clone(),
                secret: invite.secret.clone(),
                compatibility: compatibility.clone(),
                action: Action::Status,
                admission: None,
            },
        )
        .await?;
        let (manifest, snapshot) = match receive(&mut stream).await? {
            Response::World { manifest, snapshot } => (manifest, *snapshot),
            Response::Refused { reason } => bail!("host refused status: {reason}"),
            _ => bail!("unexpected bootstrap response to world status"),
        };
        invite.check(&manifest)?;
        manifest
            .compatibility
            .compare(&compatibility)
            .map_err(anyhow::Error::msg)?;
        snapshot.validate(&manifest).map_err(anyhow::Error::msg)?;
        let mut extra = [0; 1];
        ensure!(
            stream.read(&mut extra).await? == 0,
            "extra bytes after world snapshot"
        );
        Ok(snapshot)
    })
    .await
    .map_err(|_| anyhow::anyhow!("world status timed out"))?
}

/// Issues only after local receipt/bytes and manifest pinning have passed.
/// The server sees a claim; this cannot attest which save the engine has loaded.
pub async fn prepare_launch(
    endpoint: SocketAddr,
    invite: &Invite,
    compatibility: Compatibility,
    import: &Path,
    identity: &str,
) -> Result<Launch> {
    invite.validate()?;
    compatibility.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        crate::career_entry::endpoint_valid(endpoint) && session_protocol::identity_valid(identity),
        "invalid admission endpoint/identity"
    );
    let receipt = inspect_import(import)?;
    let (manifest, _) = store::load(import).map_err(anyhow::Error::msg)?;
    invite.check(&manifest)?;
    manifest
        .compatibility
        .compare(&compatibility)
        .map_err(anyhow::Error::msg)?;
    tokio::time::timeout(DEADLINE, async {
        let mut stream = TcpStream::connect(endpoint).await?;
        send(
            &mut stream,
            &Request {
                schema: 1,
                career: invite.career.clone(),
                secret: invite.secret.clone(),
                compatibility: compatibility.clone(),
                action: Action::Admit,
                admission: Some(AdmissionRequest {
                    identity: identity.into(),
                    receipt,
                }),
            },
        )
        .await?;
        let (manifest, launch) = match receive(&mut stream).await? {
            Response::Admission { manifest, launch } => (manifest, *launch),
            Response::Refused { reason } => bail!("host refused admission: {reason}"),
            _ => bail!("unexpected response to career admission"),
        };
        invite.check(&manifest)?;
        ensure!(
            launch.career == invite.career && launch.manifest_sha256 == invite.manifest_sha256,
            "admission manifest mismatch"
        );
        launch
            .preflight(import, &compatibility, identity, &launch.room, launch.relay)
            .map_err(anyhow::Error::msg)?;
        let mut extra = [0; 1];
        ensure!(
            stream.read(&mut extra).await? == 0,
            "extra bytes after admission"
        );
        Ok(launch)
    })
    .await
    .map_err(|_| anyhow::anyhow!("career admission timed out"))?
}

#[derive(Serialize)]
pub struct RoomReport {
    pub schema: u32,
    pub control: Report,
    pub relay: crate::session_server::Report,
    pub game_tested: bool,
}
/// Owns both services. Completion/failure of either shuts down and joins the other;
/// no detached relay is left serving a stale registry after control failure.
pub async fn serve_room(
    listener: TcpListener,
    socket: tokio::net::UdpSocket,
    data: &Path,
    settings: crate::session_hub::Settings,
    advertised: SocketAddr,
    shutdown: impl Future<Output = ()>,
) -> Result<RoomReport> {
    ensure!(
        advertised.port() == socket.local_addr()?.port()
            && advertised.is_ipv4() == socket.local_addr()?.is_ipv4(),
        "advertised relay port/family differs from bind"
    );
    settings.validate().map_err(anyhow::Error::msg)?;
    let (manifest, _) = store::load(data).map_err(anyhow::Error::msg)?;
    let registry =
        Registry::new(manifest, settings.room.clone(), advertised).map_err(anyhow::Error::msg)?;
    registry
        .check_settings(&settings.room, &settings.profile)
        .map_err(anyhow::Error::msg)?;
    let (tcp_stop, tcp_stopped) = tokio::sync::oneshot::channel();
    let (udp_stop, udp_stopped) = tokio::sync::oneshot::channel();
    let tcp = serve_with_admission(listener, data, Some(registry.clone()), async {
        let _ = tcp_stopped.await;
    });
    let udp = crate::session_server::run_with_admission(socket, settings, Some(registry), async {
        let _ = udp_stopped.await;
    });
    tokio::pin!(tcp, udp, shutdown);
    let (mut control, mut relay) = (None, None);
    tokio::select! { _=&mut shutdown=>{}, result=&mut tcp=>control=Some(result), result=&mut udp=>relay=Some(result) }
    let _ = tcp_stop.send(());
    let _ = udp_stop.send(());
    if control.is_none() {
        control = Some((&mut tcp).await);
    }
    if relay.is_none() {
        relay = Some((&mut udp).await);
    }
    Ok(RoomReport {
        schema: 1,
        control: control.unwrap()?,
        relay: relay.unwrap()?,
        game_tested: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{career::Goal, career_store::tests::Temp};
    async fn udp_reply(socket: &tokio::net::UdpSocket) -> session_protocol::ServerMessage {
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut bytes = [0; crate::protocol::MAX_PACKET + 1];
            loop {
                let n = socket.recv(&mut bytes).await.unwrap();
                let frame = session_protocol::decode_server(&bytes[..n]).unwrap();
                if matches!(
                    frame.message,
                    session_protocol::ServerMessage::Accepted { .. }
                        | session_protocol::ServerMessage::Rejected { .. }
                        | session_protocol::ServerMessage::Left { .. }
                        | session_protocol::ServerMessage::Snapshot { .. }
                ) {
                    break frame.message;
                }
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn career_control_issues_identity_bound_tickets_and_real_udp_requires_them() {
        use crate::{
            session_client::{Client, Options},
            session_protocol::{ClientMessage as C, Refusal, ServerMessage as S},
        };
        let dir = Temp::new();
        let (data, invite, compatibility) = fixture(&dir);
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control = tcp.local_addr().unwrap();
        let udp = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let relay = udp.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let host_data = data.clone();
        let server = tokio::spawn(async move {
            serve_room(
                tcp,
                udp,
                &host_data,
                crate::session_hub::Settings {
                    room: "career".into(),
                    profile: session_protocol::PROFILE.into(),
                    max_clients: 2,
                    timeout_ms: 2000,
                    echo: false,
                },
                relay,
                async {
                    let _ = stopped.await;
                },
            )
            .await
        });
        let mut peers = Vec::new();
        for id in 1..=2 {
            let import = dir.0.join(format!("guest-{id}"));
            fetch(control, &invite, compatibility.clone(), &import)
                .await
                .unwrap();
            let identity = format!("{id:032x}");
            let launch =
                prepare_launch(control, &invite, compatibility.clone(), &import, &identity)
                    .await
                    .unwrap();
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            socket.connect(relay).await.unwrap();
            let mut client = Client::new(
                Options {
                    room: launch.room.clone(),
                    identity: identity.clone(),
                    profile: compatibility.profile.clone(),
                    admission: Some(launch.proof.clone()),
                },
                id,
            )
            .unwrap();
            let join = client.poll(0).0.remove(0);
            for variant in 0..3 {
                let mut bad = join.clone();
                if let C::Join {
                    admission,
                    identity,
                    ..
                } = &mut bad.message
                {
                    match variant {
                        0 => *admission = None,
                        1 => admission.as_mut().unwrap().ticket = "0".repeat(32),
                        _ => *identity = "f".repeat(32),
                    }
                }
                socket
                    .send(&session_protocol::encode_client(&bad).unwrap())
                    .await
                    .unwrap();
                assert!(matches!(
                    udp_reply(&socket).await,
                    S::Rejected {
                        reason: Refusal::Admission,
                        ..
                    }
                ));
            }
            socket
                .send(&session_protocol::encode_client(&join).unwrap())
                .await
                .unwrap();
            let accepted = udp_reply(&socket).await;
            assert!(matches!(accepted, S::Accepted { .. }));
            client.ingest(
                &session_protocol::encode_server(&session_protocol::ServerFrame::new(accepted))
                    .unwrap(),
                0,
            );
            let pose = client
                .pose(
                    crate::protocol::Position {
                        x: id as f32,
                        y: 0.0,
                        z: 0.0,
                    },
                    None,
                    1,
                )
                .unwrap();
            socket
                .send(&session_protocol::encode_client(&pose).unwrap())
                .await
                .unwrap();
            peers.push((client, socket));
        }
        assert!(
            matches!(udp_reply(&peers[0].1).await,S::Snapshot{ref poses,..} if poses.iter().any(|p|p.position.x==2.0))
        );
        // A valid career ticket must also be refused by an unguarded/open hub.
        let manifest = store::load(&data).unwrap().0;
        let mut open = crate::session_hub::Hub::new(
            crate::session_hub::Settings {
                room: "career".into(),
                profile: manifest.compatibility.profile,
                max_clients: 2,
                timeout_ms: 2000,
                echo: false,
            },
            1,
        )
        .unwrap();
        peers[0].0.request_reconnect();
        let reconnect = peers[0].0.poll(2).0.remove(0);
        let refused = open
            .ingest(
                "127.0.0.1:12345".parse().unwrap(),
                &session_protocol::encode_client(&reconnect).unwrap(),
                0,
                identity::random_id,
            )
            .unwrap();
        assert!(matches!(
            refused[0].1.message,
            S::Rejected {
                reason: Refusal::Admission,
                ..
            }
        ));
        peers[0]
            .1
            .send(&session_protocol::encode_client(&reconnect).unwrap())
            .await
            .unwrap();
        // Roster/snapshot may precede acceptance during a live reconnect.
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if matches!(udp_reply(&peers[0].1).await, S::Accepted { .. }) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        stop.send(()).unwrap();
        let report = server.await.unwrap().unwrap();
        assert_eq!(report.relay.metrics.accepted, 3);
        assert_eq!(report.relay.metrics.refused, 6);
        assert!(!report.game_tested);
        assert!(tokio::net::UdpSocket::bind(relay).await.is_ok());
        assert!(TcpListener::bind(control).await.is_ok());
        WorldStore::open(&data).unwrap();
    }
    #[tokio::test]
    async fn bootstrap_only_service_cannot_issue_relay_tickets() {
        let dir = Temp::new();
        let (data, invite, c) = fixture(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            serve(listener, &data, async {
                let _ = stopped.await;
            })
            .await
        });
        let import = dir.0.join("import");
        fetch(endpoint, &invite, c.clone(), &import).await.unwrap();
        assert!(
            prepare_launch(endpoint, &invite, c, &import, &"1".repeat(32))
                .await
                .unwrap_err()
                .to_string()
                .contains("unavailable")
        );
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn control_start_failure_does_not_leave_a_guarded_relay_running() {
        let dir = Temp::new();
        let (data, _, _) = fixture(&dir);
        let lock = WorldStore::open(&data).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp = listener.local_addr().unwrap();
        let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp = socket.local_addr().unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            serve_room(
                listener,
                socket,
                &data,
                crate::session_hub::Settings {
                    room: "career".into(),
                    profile: session_protocol::PROFILE.into(),
                    max_clients: 2,
                    timeout_ms: 2000,
                    echo: false,
                },
                udp,
                std::future::pending(),
            ),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        drop(lock);
        assert!(TcpListener::bind(tcp).await.is_ok());
        assert!(tokio::net::UdpSocket::bind(udp).await.is_ok());
        WorldStore::open(&data).unwrap();
    }
    fn fixture(dir: &Temp) -> (std::path::PathBuf, Invite, Compatibility) {
        let save = dir.0.join("input.sav");
        store::write_new(&save, &vec![27; 100_000]).unwrap();
        let c = Compatibility {
            profile: session_protocol::PROFILE.into(),
            map_sha256: career::hash(b"own map"),
            mods_sha256: vec![],
        };
        let data = dir.0.join("career");
        let manifest = store::create(&data, &save, Goal::Unlimited, c.clone()).unwrap();
        let invite = Invite::new(&manifest).unwrap();
        store::write_new(&data.join("invite.json"), &store::json(&invite).unwrap()).unwrap();
        (data, invite, c)
    }
    #[tokio::test]
    async fn real_tcp_bootstrap_refusals_integrity_and_isolation() {
        let dir = Temp::new();
        let (data, invite, c) = fixture(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            serve(listener, &data, async {
                let _ = stopped.await;
            })
            .await
        });
        let mut wrong = invite.clone();
        wrong.secret = "0".repeat(32);
        let out = dir.0.join("guest");
        assert!(fetch(address, &wrong, c.clone(), &out).await.is_err());
        assert!(!out.exists());
        let mut different = c.clone();
        different.map_sha256 = career::hash(b"different map");
        assert!(fetch(address, &invite, different, &out).await.is_err());
        assert!(!out.exists());
        let mut unpinned = invite.clone();
        unpinned.manifest_sha256 = "0".repeat(64);
        assert!(fetch(address, &unpinned, c.clone(), &out).await.is_err());
        assert!(!out.exists());
        let receipt = fetch(address, &invite, c.clone(), &out).await.unwrap();
        assert!(receipt.bootstrap_verified && !receipt.world_synchronized);
        inspect_import(&out).unwrap();
        assert!(fetch(address, &invite, c, &out).await.is_err());
        std::fs::write(out.join("bootstrap.sav"), b"corrupt").unwrap();
        assert!(inspect_import(&out).is_err());
        assert_eq!(
            store::read_bounded(&dir.0.join("input.sav"), 100_000).unwrap(),
            vec![27; 100_000]
        );
        stop.send(()).unwrap();
        let report = server.await.unwrap().unwrap();
        assert_eq!(report.connections, 4);
    }
    #[tokio::test]
    async fn truncated_or_corrupt_transfer_cannot_commit_a_receipt() {
        for corrupt in [false, true] {
            let dir = Temp::new();
            let (data, invite, c) = fixture(&dir);
            let (manifest, mut bytes) = store::load(&data).unwrap();
            if corrupt {
                bytes[0] ^= 1;
            } else {
                bytes.truncate(10);
            }
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let _: Request = receive(&mut socket).await.unwrap();
                send(&mut socket, &Response::Bootstrap { manifest })
                    .await
                    .unwrap();
                socket.write_all(&bytes).await.unwrap();
            });
            let output = dir.0.join("guest");
            assert!(fetch(endpoint, &invite, c, &output).await.is_err());
            server.await.unwrap();
            assert!(output.join("INCOMPLETE").exists());
            assert!(!output.join("receipt.json").exists());
            assert!(inspect_import(&output).is_err());
        }
    }
    #[tokio::test]
    async fn framing_rejects_oversized_before_allocating_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let sender = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            s.write_u32(u32::MAX).await.unwrap();
        });
        let mut socket = TcpStream::connect(endpoint).await.unwrap();
        assert!(receive::<Request>(&mut socket).await.is_err());
        sender.await.unwrap();
    }
    #[tokio::test]
    async fn running_sidecar_clock_is_shared_and_checkpointed_on_shutdown() {
        let dir = Temp::new();
        let (data, invite, compatibility) = fixture(&dir);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_data = data.clone();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            serve(listener, &server_data, async {
                let _ = stopped.await;
            })
            .await
        });
        let first = world_status(address, &invite, compatibility.clone())
            .await
            .unwrap();
        let later = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let snapshot = world_status(address, &invite, compatibility.clone())
                    .await
                    .unwrap();
                if snapshot.clock.world_ms >= first.clock.world_ms + 1000 {
                    break snapshot;
                }
            }
        })
        .await
        .unwrap();
        assert!(!later.engine_adapters && !later.original_save_updated);
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
        let recovered = WorldStore::open(&data).unwrap();
        assert!(recovered.snapshot().clock_checkpoint_world_ms >= later.clock.world_ms);
    }
}

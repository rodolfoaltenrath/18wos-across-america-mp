//! Explicit LAN bootstrap control plane, separate from the proven UDP pose wire.
//! Bounded frames/peers/time and a frozen blob; no engine access or live-save writes.
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
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    #[default]
    Bootstrap,
    Status,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Response {
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
    let mut authority = WorldStore::open(data).map_err(anyhow::Error::msg)?;
    let (manifest, save) = store::load(data).map_err(anyhow::Error::msg)?;
    let invite: Invite = store::read_json(&data.join("invite.json")).map_err(anyhow::Error::msg)?;
    invite.check(&manifest)?;
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
                tasks.spawn(async move { let _slot = slot; tokio::time::timeout(DEADLINE, handle(stream, &invite, &manifest, &save, &world)).await });
            }
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: u32,
    pub career: String,
    pub manifest_sha256: String,
    pub save_sha256: String,
    pub bytes: u64,
    pub bootstrap_verified: bool,
    pub engine_loaded: bool,
    pub world_synchronized: bool,
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
        send(&mut stream, &Request { schema: 1, career: invite.career.clone(), secret: invite.secret.clone(), compatibility: compatibility.clone(), action: Action::Bootstrap }).await?;
        let manifest = match receive(&mut stream).await? {
            Response::Bootstrap { manifest } => manifest,
            Response::Refused { reason } => bail!("host refused bootstrap: {reason}"),
            Response::World { .. } => bail!("unexpected world response to bootstrap"),
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
pub fn inspect_import(path: &Path) -> Result<Receipt> {
    ensure!(!path.join("INCOMPLETE").try_exists()?, "incomplete import");
    let (manifest, _) = store::load(path).map_err(anyhow::Error::msg)?;
    let receipt: Receipt =
        store::read_json(&path.join("receipt.json")).map_err(anyhow::Error::msg)?;
    ensure!(
        receipt.schema == 1
            && receipt.bootstrap_verified
            && !receipt.engine_loaded
            && !receipt.world_synchronized
            && receipt.career == manifest.career
            && receipt.manifest_sha256
                == career::hash(&store::json(&manifest).map_err(anyhow::Error::msg)?)
            && receipt.save_sha256 == manifest.bootstrap.sha256
            && receipt.bytes == manifest.bootstrap.bytes,
        "inconsistent import receipt"
    );
    Ok(receipt)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{career::Goal, career_store::tests::Temp};
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

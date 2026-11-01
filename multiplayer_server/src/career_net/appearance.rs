//! Ticket-checked RPC over the EXISTING career TCP framing. No UDP/engine work.
use super::*;
use crate::{appearance_stream as stream, career_entry::Proof, world_join::Context};
use std::{collections::BTreeMap, sync::Mutex, time::Instant};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub(super) identity: String,
    pub(super) room: String,
    pub(super) proof: Proof,
    pub(super) pin: stream::Pin,
    pub(super) operation: stream::Operation,
}
/// Counts only requests with a current valid registry proof. No identities/paths.
#[derive(Clone, Default, Serialize)]
pub struct Summary {
    pub schema: u32,
    pub requests: u32,
    pub refused: u32,
    pub publications: u32,
    pub duplicates: u32,
    pub stale: u32,
    pub snapshot_replies: u32,
    pub event_pages: u32,
    pub head: u64,
    pub retained: usize,
    pub truncated: bool,
    pub seed_sidecar_frozen: bool,
    pub engine_verified: bool,
    pub world_synchronized: bool,
}
fn inc(value: &mut u32, truncated: &mut bool) {
    if *value >= 999_999_999 {
        *truncated = true;
    } else {
        *value += 1;
    }
}
struct State {
    authority: stream::Authority,
    start: Instant,
    requests: u32,
    per_identity: BTreeMap<String, u32>,
    summary: Summary,
}
pub(super) struct Service(Mutex<State>);
impl Service {
    pub fn new(authority: stream::Authority) -> Self {
        Self(Mutex::new(State {
            authority,
            start: Instant::now(),
            requests: 0,
            per_identity: BTreeMap::new(),
            summary: Summary {
                schema: 1,
                seed_sidecar_frozen: true,
                ..Summary::default()
            },
        }))
    }
    pub fn execute(
        &self,
        registry: &Registry,
        profile: &str,
        request: Request,
    ) -> std::result::Result<stream::Reply, String> {
        if !registry.allows(
            &request.identity,
            &request.room,
            profile,
            Some(&request.proof),
        ) {
            return Err("appearance admission ticket unavailable or expired".into());
        }
        let mut state = self
            .0
            .lock()
            .map_err(|_| "appearance authority unavailable")?;
        let state = &mut *state;
        inc(&mut state.summary.requests, &mut state.summary.truncated);
        let now = Instant::now();
        if now.duration_since(state.start) >= Duration::from_secs(1) {
            state.start = now;
            state.requests = 0;
            state.per_identity.clear();
        }
        let count = state
            .per_identity
            .get(&request.identity)
            .copied()
            .unwrap_or(0);
        if state.requests >= 64 || count >= 8 {
            inc(&mut state.summary.refused, &mut state.summary.truncated);
            return Err("appearance request budget exceeded".into());
        }
        state.requests += 1;
        state
            .per_identity
            .insert(request.identity.clone(), count + 1);
        let result = state
            .authority
            .execute(&request.identity, &request.pin, &request.operation);
        match &result {
            Err(_) => inc(&mut state.summary.refused, &mut state.summary.truncated),
            Ok(reply) => {
                let counter = match reply.payload {
                    stream::Payload::Snapshot { .. } => &mut state.summary.snapshot_replies,
                    stream::Payload::Events { .. } => &mut state.summary.event_pages,
                    stream::Payload::Accepted {
                        disposition: stream::Disposition::Published,
                        ..
                    } => &mut state.summary.publications,
                    stream::Payload::Accepted {
                        disposition: stream::Disposition::Duplicate,
                        ..
                    } => &mut state.summary.duplicates,
                    stream::Payload::Accepted {
                        disposition: stream::Disposition::Stale,
                        ..
                    } => &mut state.summary.stale,
                };
                inc(counter, &mut state.summary.truncated);
            }
        }
        result
    }
    pub fn summary(&self) -> std::result::Result<Summary, String> {
        let state = self
            .0
            .lock()
            .map_err(|_| "appearance authority unavailable")?;
        let mut summary = state.summary.clone();
        summary.head = state.authority.head();
        summary.retained = state.authority.retained();
        Ok(summary)
    }
}
/// New TCP connection per RPC: retry a lost publication with its SAME revision.
/// Validates a reply before returning it; never marks an engine player ready.
pub async fn exchange(
    endpoint: SocketAddr,
    launch: &Launch,
    context: Context<'_>,
    pin: &stream::Pin,
    operation: stream::Operation,
) -> Result<stream::Reply> {
    launch.validate().map_err(anyhow::Error::msg)?;
    ensure!(
        crate::career_entry::endpoint_valid(endpoint),
        "invalid appearance endpoint"
    );
    ensure!(
        launch.career == context.career.career
            && launch.manifest_sha256
                == crate::career_entry::manifest_hash(context.career)
                    .map_err(anyhow::Error::msg)?
            && pin.career_manifest_sha256 == launch.manifest_sha256,
        "appearance launch career differs"
    );
    launch
        .compatibility
        .compare(&context.career.compatibility)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        stream::Pin::new(&context, pin.world_epoch, pin.authority_generation)
            .map_err(anyhow::Error::msg)?
            == *pin,
        "appearance local context differs"
    );
    tokio::time::timeout(DEADLINE, async {
        let mut socket = TcpStream::connect(endpoint).await?;
        let claim = Request {
            identity: launch.identity.clone(),
            room: launch.room.clone(),
            proof: launch.proof.clone(),
            pin: pin.clone(),
            operation: operation.clone(),
        };
        send(
            &mut socket,
            &super::Request {
                schema: 1,
                career: launch.career.clone(),
                secret: String::new(),
                compatibility: launch.compatibility.clone(),
                action: Action::Appearance,
                admission: None,
                appearance: Some(claim),
            },
        )
        .await?;
        let reply = match receive(&mut socket).await? {
            Response::Appearance { reply } => *reply,
            Response::Refused { reason } => bail!("host refused appearance: {reason}"),
            _ => bail!("unexpected response to appearance request"),
        };
        reply
            .validate(context, pin, &operation)
            .map_err(anyhow::Error::msg)?;
        let mut extra = [0; 1];
        ensure!(
            socket.read(&mut extra).await? == 0,
            "extra bytes after appearance reply"
        );
        Ok(reply)
    })
    .await
    .map_err(|_| anyhow::anyhow!("appearance request timed out; retry SAME generation/revision"))?
}

#[cfg(test)]
mod tests;

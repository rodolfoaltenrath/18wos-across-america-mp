//! Opt-in session wire v2. v1 remains a separate, unchanged codec.
use crate::protocol::{MAX_PACKET, Position, Rotation};
use serde::{Deserialize, Serialize};
use std::fmt;
pub const VERSION: u8 = 2;
pub const PROFILE: &str = "steam-aa-1.10-ccb327c2";
pub const MAX_PLAYERS: usize = 64;
pub const BATCH: usize = 4;
pub const ECHO_BASE: u32 = 1_000_000_000;
pub const HEARTBEAT_MS: u64 = 500;
pub const RETRY_MS: u64 = 500;
pub fn room_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
pub fn identity_valid(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn profile_valid(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|b| b.is_ascii_graphic())
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub session: u64,
    pub connection: u64,
    pub token: String,
}
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("session", &self.session)
            .field("connection", &self.connection)
            .field("token", &"[redacted]")
            .finish()
    }
}
impl Credentials {
    pub fn valid(&self) -> bool {
        self.session != 0 && self.connection != 0 && identity_valid(&self.token)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientFrame {
    pub version: u8,
    pub message: ClientMessage,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Join {
        request: u64,
        room: String,
        identity: String,
        profile: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume: Option<Credentials>,
    },
    Pose {
        credentials: Credentials,
        sequence: u64,
        client_ms: u64,
        position: Position,
        rotation: Option<Rotation>,
    },
    Ping {
        credentials: Credentials,
        sequence: u64,
        client_ms: u64,
    },
    Leave {
        credentials: Credentials,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Version,
    Room,
    Profile,
    Full,
    IdentityInUse,
    EndpointInUse,
    Closing,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerFrame {
    pub version: u8,
    pub message: ServerMessage,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerMessage {
    Accepted {
        request: u64,
        credentials: Credentials,
        player: u32,
        revision: u64,
        timeout_ms: u64,
        server_ms: u64,
    },
    Rejected {
        request: u64,
        reason: Refusal,
        supported: u8,
    },
    Pong {
        session: u64,
        connection: u64,
        sequence: u64,
        client_ms: u64,
        server_ms: u64,
    },
    Roster {
        session: u64,
        revision: u64,
        players: Vec<u32>,
    },
    Snapshot {
        session: u64,
        revision: u64,
        tick: u64,
        server_ms: u64,
        poses: Vec<Pose>,
    },
    Left {
        session: u64,
        connection: u64,
    },
    Closing {
        session: u64,
        connection: u64,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub player: u32,
    pub generation: u64,
    pub sequence: u64,
    /// Server receipt time, not a proved physics tick or the sender's clock.
    pub received_ms: u64,
    pub position: Position,
    pub rotation: Option<Rotation>,
}
impl Pose {
    pub fn valid(&self) -> bool {
        self.player > 0
            && self.player < ECHO_BASE * 2
            && self.generation > 0
            && self.position.is_valid()
            && self.rotation.is_none_or(Rotation::is_valid)
    }
}
impl ClientFrame {
    pub fn new(message: ClientMessage) -> Self {
        Self {
            version: VERSION,
            message,
        }
    }
    fn valid(&self) -> bool {
        match &self.message {
            // Keep structurally valid version-mismatched joins for explicit refusal.
            ClientMessage::Join {
                request,
                room,
                identity,
                profile,
                resume,
            } => {
                *request != 0
                    && room_valid(room)
                    && identity_valid(identity)
                    && profile_valid(profile)
                    && resume.as_ref().is_none_or(Credentials::valid)
            }
            ClientMessage::Pose {
                credentials,
                position,
                rotation,
                ..
            } => {
                self.version == VERSION
                    && credentials.valid()
                    && position.is_valid()
                    && rotation.is_none_or(Rotation::is_valid)
            }
            ClientMessage::Ping { credentials, .. } | ClientMessage::Leave { credentials } => {
                self.version == VERSION && credentials.valid()
            }
        }
    }
}
impl ServerFrame {
    pub fn new(message: ServerMessage) -> Self {
        Self {
            version: VERSION,
            message,
        }
    }
    fn valid(&self) -> bool {
        if self.version != VERSION {
            return false;
        }
        match &self.message {
            ServerMessage::Accepted {
                request,
                credentials,
                player,
                timeout_ms,
                revision,
                ..
            } => {
                *request != 0
                    && credentials.valid()
                    && *player > 0
                    && *player < ECHO_BASE
                    && (2000..=60000).contains(timeout_ms)
                    && *revision > 0
            }
            ServerMessage::Rejected {
                request, supported, ..
            } => *request != 0 && *supported > 0,
            ServerMessage::Pong {
                session,
                connection,
                ..
            }
            | ServerMessage::Left {
                session,
                connection,
            }
            | ServerMessage::Closing {
                session,
                connection,
            } => *session != 0 && *connection != 0,
            ServerMessage::Roster {
                session,
                revision,
                players,
            } => {
                *session != 0
                    && *revision > 0
                    && players.len() <= MAX_PLAYERS
                    && players.iter().all(|p| *p > 0 && *p < ECHO_BASE * 2)
                    && players
                        .iter()
                        .enumerate()
                        .all(|(i, p)| !players[..i].contains(p))
            }
            ServerMessage::Snapshot {
                session,
                revision,
                poses,
                server_ms,
                ..
            } => {
                *session != 0
                    && *revision > 0
                    && !poses.is_empty()
                    && poses.len() <= BATCH
                    && poses
                        .iter()
                        .all(|p| p.valid() && p.received_ms <= *server_ms)
                    && poses
                        .iter()
                        .enumerate()
                        .all(|(i, p)| !poses[..i].iter().any(|old| old.player == p.player))
            }
        }
    }
}
pub fn encode_client(value: &ClientFrame) -> Result<Vec<u8>, String> {
    if !value.valid() {
        return Err("invalid session client frame".into());
    }
    crate::protocol::encode(value)
}
pub fn encode_server(value: &ServerFrame) -> Result<Vec<u8>, String> {
    if !value.valid() {
        return Err("invalid session server frame".into());
    }
    crate::protocol::encode(value)
}
pub fn decode_client(bytes: &[u8]) -> Option<ClientFrame> {
    if bytes.len() > MAX_PACKET {
        return None;
    }
    let value: ClientFrame = serde_json::from_slice(bytes).ok()?;
    value.valid().then_some(value)
}
pub fn decode_server(bytes: &[u8]) -> Option<ServerFrame> {
    if bytes.len() > MAX_PACKET {
        return None;
    }
    let value: ServerFrame = serde_json::from_slice(bytes).ok()?;
    value.valid().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_batches_fit_with_extreme_valid_values_and_reject_unknown_fields() {
        let poses: Vec<_> = (1..=BATCH)
            .map(|player| Pose {
                player: ECHO_BASE + player as u32,
                generation: u64::MAX,
                sequence: u64::MAX,
                received_ms: u64::MAX,
                position: Position {
                    x: f32::MAX,
                    y: f32::MIN,
                    z: f32::MIN_POSITIVE,
                },
                rotation: Some(Rotation {
                    x: 0.12345678,
                    y: 0.0,
                    z: 0.0,
                    w: 0.992_349_9,
                }),
            })
            .collect();
        let frame = ServerFrame::new(ServerMessage::Snapshot {
            session: u64::MAX,
            revision: u64::MAX,
            tick: u64::MAX,
            server_ms: u64::MAX,
            poses,
        });
        let bytes = encode_server(&frame).unwrap();
        assert!(bytes.len() <= MAX_PACKET);
        assert!(decode_server(&bytes).is_some());
        let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["extra"] = true.into();
        assert!(decode_server(&serde_json::to_vec(&json).unwrap()).is_none());
        json.as_object_mut().unwrap().remove("extra");
        json["message"]["extra"] = true.into();
        assert!(decode_server(&serde_json::to_vec(&json).unwrap()).is_none());
        json["message"].as_object_mut().unwrap().remove("extra");
        json["message"]["poses"][1] = json["message"]["poses"][0].clone();
        assert!(decode_server(&serde_json::to_vec(&json).unwrap()).is_none());
    }
    #[test]
    fn version_mismatch_join_can_be_refused_without_admitting_v1_or_invalid_data() {
        let frame = ClientFrame {
            version: 255,
            message: ClientMessage::Join {
                request: 1,
                room: "lan".into(),
                identity: "a".repeat(32),
                profile: PROFILE.into(),
                resume: None,
            },
        };
        assert!(decode_client(&crate::protocol::encode(&frame).unwrap()).is_some());
        assert!(
            decode_client(br#"{"version":1,"sequence":1,"position":{"x":0,"y":0,"z":0}}"#)
                .is_none()
        );
        assert!(!room_valid("../save"));
        assert!(!identity_valid(&"A".repeat(32)));
        assert!(decode_client(&[b' '; MAX_PACKET + 1]).is_none());
        let credentials = Credentials {
            session: 1,
            connection: 2,
            token: "c".repeat(32),
        };
        assert!(!format!("{credentials:?}").contains(&credentials.token));
    }
}

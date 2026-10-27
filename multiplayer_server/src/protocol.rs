use serde::{Deserialize, Serialize};

pub const VERSION: u8 = 1;
pub const MAX_PACKET: usize = 1200;
pub const TICK_HZ: u32 = 60;
pub const DEFAULT_SERVER: &str = "127.0.0.1:39000";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Position {
    pub fn is_valid(self) -> bool {
        [self.x, self.y, self.z].iter().all(|v| v.is_finite())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientUpdate {
    pub version: u8,
    pub sequence: u64,
    pub position: Position,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerUpdate {
    pub version: u8,
    pub player_id: u64,
    pub sequence: u64,
    pub position: Position,
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_PACKET {
        return Err("packet exceeds 1200 bytes".into());
    }
    Ok(bytes)
}

pub fn decode_client(bytes: &[u8]) -> Option<ClientUpdate> {
    if bytes.len() > MAX_PACKET {
        return None;
    }
    let value: ClientUpdate = serde_json::from_slice(bytes).ok()?;
    (value.version == VERSION && value.position.is_valid()).then_some(value)
}

pub fn decode_server(bytes: &[u8]) -> Option<ServerUpdate> {
    if bytes.len() > MAX_PACKET {
        return None;
    }
    let value: ServerUpdate = serde_json::from_slice(bytes).ok()?;
    (value.version == VERSION && value.player_id != 0 && value.position.is_valid()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_reject_invalid_datagrams() {
        let update = ClientUpdate {
            version: VERSION,
            sequence: 42,
            position: Position {
                x: 1.0,
                y: -2.0,
                z: 3.0,
            },
        };
        let decoded = decode_client(&encode(&update).unwrap()).unwrap();
        assert_eq!(decoded.position, update.position);
        assert_eq!(decoded.sequence, 42);
        let wrong_version = ClientUpdate {
            version: 255,
            ..update
        };
        assert!(decode_client(&encode(&wrong_version).unwrap()).is_none());
        assert!(decode_client(&vec![b' '; MAX_PACKET + 1]).is_none());
        assert!(
            decode_client(br#"{"version":1,"sequence":0,"position":{"x":null,"y":0,"z":0}}"#)
                .is_none()
        );
        assert!(decode_client(b"garbage").is_none());
        assert!(
            !Position {
                x: f32::NAN,
                y: 0.0,
                z: 0.0
            }
            .is_valid()
        );
    }
}

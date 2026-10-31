//! The production decoder with slice/duplex transports. Own data only: no
//! invitations, saves, sockets, credentials or paths from an actual player.
use super::*;
use crate::{
    career::{Goal, SaveBlob},
    career_world::{Clock, Snapshot},
    fuzz::{Rng, mutate},
};
use std::{collections::BTreeMap, time::Duration};

fn compatibility() -> Compatibility {
    Compatibility {
        profile: session_protocol::PROFILE.into(),
        map_sha256: career::hash(b"own map"),
        mods_sha256: vec![],
    }
}
fn manifest() -> Manifest {
    Manifest {
        schema: 1,
        career: "a".repeat(32),
        scenario: "become_millionaire".into(),
        rules_revision: 1,
        goal: Goal::Unlimited,
        compatibility: compatibility(),
        bootstrap: SaveBlob {
            bytes: 8,
            sha256: career::hash(b"own save"),
        },
    }
}
fn framed(payload: &[u8]) -> Vec<u8> {
    let mut bytes = (payload.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(payload);
    bytes
}
fn request() -> Request {
    Request {
        schema: 1,
        career: "a".repeat(32),
        secret: "b".repeat(32),
        compatibility: compatibility(),
        action: Action::Bootstrap,
        admission: None,
    }
}
fn requests() -> Vec<Request> {
    let mut status = request();
    status.action = Action::Status;
    let mut admit = request();
    admit.action = Action::Admit;
    admit.admission = Some(AdmissionRequest {
        identity: "c".repeat(32),
        receipt: Receipt {
            schema: 1,
            career: "a".repeat(32),
            manifest_sha256: career::hash(b"own manifest"),
            save_sha256: career::hash(b"own save"),
            bytes: 8,
            bootstrap_verified: true,
            engine_loaded: false,
            world_synchronized: false,
        },
    });
    vec![request(), status, admit]
}
fn responses() -> Vec<Response> {
    vec![
        Response::Refused {
            reason: "own refusal".into(),
        },
        Response::Bootstrap {
            manifest: manifest(),
        },
        Response::World {
            manifest: manifest(),
            snapshot: Box::new(Snapshot {
                schema: 1,
                career: "a".repeat(32),
                checkpoint_revision: 0,
                clock: Clock {
                    world_ms: 0,
                    paused: false,
                    scale: 1,
                },
                clock_checkpoint_world_ms: 0,
                ranking: vec![],
                contracts: BTreeMap::new(),
                engine_adapters: false,
                original_save_updated: false,
            }),
        },
        Response::Admission {
            manifest: manifest(),
            launch: Box::new(Launch {
                schema: 1,
                career: "a".repeat(32),
                manifest_sha256: career::hash(b"own manifest"),
                identity: "c".repeat(32),
                room: "lan".into(),
                relay: "127.0.0.1:39000".parse().unwrap(),
                compatibility: compatibility(),
                proof: crate::career_entry::Proof {
                    ticket: "d".repeat(32),
                },
                valid_for_secs: 3600,
            }),
        },
    ]
}
async fn stable<T: Serialize + DeserializeOwned>(raw: &[u8]) -> bool {
    let mut reader = raw;
    let Ok(value) = receive::<T>(&mut reader).await else {
        return false;
    };
    let canonical = serde_json::to_vec(&value).unwrap();
    let frame = framed(&canonical);
    let again: T = receive(&mut frame.as_slice()).await.unwrap();
    assert_eq!(
        serde_json::to_value(value).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    true
}
#[tokio::test]
async fn mutated_request_and_response_frames_are_bounded_and_round_trip_when_accepted() {
    let requests: Vec<_> = requests()
        .iter()
        .map(|v| serde_json::to_vec(v).unwrap())
        .collect();
    let replies: Vec<_> = responses()
        .iter()
        .map(|v| serde_json::to_vec(v).unwrap())
        .collect();
    let mut accepted = [0usize; 2];
    let mut rejected = [0usize; 2];
    for raw in &requests {
        assert!(stable::<Request>(&framed(raw)).await);
    }
    for raw in &replies {
        assert!(stable::<Response>(&framed(raw)).await);
    }
    for seed in [19, 41, 97] {
        let mut rng = Rng::new(seed);
        for iteration in 0..2000 {
            let request = &requests[iteration % requests.len()];
            let reply = &replies[iteration % replies.len()];
            // Exercise payload bytes and framing words independently: random
            // lengths alone would mostly reject before reaching the JSON parser.
            let inputs = if iteration == 0 {
                [framed(request), framed(reply)]
            } else if iteration % 2 == 0 {
                [
                    framed(&mutate(&mut rng, request)),
                    framed(&mutate(&mut rng, reply)),
                ]
            } else {
                [
                    mutate(&mut rng, &framed(request)),
                    mutate(&mut rng, &framed(reply)),
                ]
            };
            for (index, ok) in [
                stable::<Request>(&inputs[0]).await,
                stable::<Response>(&inputs[1]).await,
            ]
            .into_iter()
            .enumerate()
            {
                if ok {
                    accepted[index] += 1;
                } else {
                    rejected[index] += 1;
                }
            }
        }
    }
    assert!(accepted.into_iter().all(|n| n > 0) && rejected.into_iter().all(|n| n > 0));
    eprintln!(
        "career_frame own inputs: accepted={accepted:?}, rejected={rejected:?}, seed=19/41/97"
    );
}
#[tokio::test]
async fn byte_fragmentation_and_duplicate_fields_use_the_real_decoder() {
    let raw = framed(&serde_json::to_vec(&request()).unwrap());
    let (mut writer, mut reader) = tokio::io::duplex(8);
    let task = tokio::spawn(async move {
        for byte in raw {
            writer.write_all(&[byte]).await.unwrap();
            tokio::task::yield_now().await;
        }
    });
    let decoded = tokio::time::timeout(Duration::from_secs(3), receive::<Request>(&mut reader))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(decoded.career, "a".repeat(32));
    task.await.unwrap();
    let raw = serde_json::to_string(&request()).unwrap();
    let duplicated = raw.replacen('{', "{\"schema\":1,", 1);
    assert!(!stable::<Request>(&framed(duplicated.as_bytes())).await);
    assert!(!stable::<Response>(&framed(br#"{"type":"refused","reason":"a","reason":"b"}"#)).await);
}
#[tokio::test]
async fn invalid_lengths_eof_and_cancelled_partial_reads_do_not_wait_or_keep_the_stream() {
    for size in [0, MAX_FRAME as u32 + 1, u32::MAX] {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        writer.write_u32(size).await.unwrap();
        // Writer stays open: a bad length must be refused before awaiting payload.
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receive::<Request>(&mut reader))
                .await
                .unwrap()
                .is_err()
        );
    }
    let frame = framed(&serde_json::to_vec(&request()).unwrap());
    for count in [0, 1, 3, 4, frame.len() - 1] {
        assert!(!stable::<Request>(&frame[..count]).await);
    }
    let (mut writer, mut reader) = tokio::io::duplex(8);
    writer.write_u32(32).await.unwrap();
    let timed = tokio::time::timeout(Duration::from_millis(50), async move {
        receive::<Request>(&mut reader).await
    })
    .await;
    assert!(timed.is_err());
    assert!(
        writer.write_all(b"x").await.is_err(),
        "cancelled owned request must drop its stream"
    );
}

use crate::protocol::{self, ClientUpdate, MAX_PACKET, ServerUpdate, TICK_HZ, VERSION};
use anyhow::{Result, ensure};
use std::{collections::HashMap, future::Future, net::SocketAddr, time::Duration};
use tokio::{
    net::UdpSocket,
    time::{self, Instant, MissedTickBehavior},
};

struct Peer {
    id: u64,
    last_seen: Instant,
    latest: ClientUpdate,
}

/// Player ids at or above this value are echoes of the receiving client itself.
pub const ECHO_ID_BASE: u64 = 1_000_000_000;

pub async fn run(
    socket: UdpSocket,
    max_clients: usize,
    timeout: Duration,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    run_with_echo(socket, max_clients, timeout, false, shutdown).await
}

/// `echo` also returns each client's own update under `ECHO_ID_BASE + id`, so a
/// single PC can exercise relay, interpolation and drawing of a remote.
pub async fn run_with_echo(
    socket: UdpSocket,
    max_clients: usize,
    timeout: Duration,
    echo: bool,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    ensure!(
        (1..=64).contains(&max_clients),
        "max_clients must be 1..=64"
    );
    ensure!(!timeout.is_zero(), "timeout must be positive");
    let mut peers: HashMap<SocketAddr, Peer> = HashMap::new();
    let mut next_id = 1u64;
    let mut interval = time::interval(Duration::from_secs_f64(1.0 / f64::from(TICK_HZ)));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut buffer = [0u8; MAX_PACKET + 1];
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            received = socket.recv_from(&mut buffer) => {
                let (len, addr) = match received {
                    Ok(value) => value,
                    // Oversized UDP datagrams on Windows may return WSAEMSGSIZE.
                    Err(error) if matches!(error.raw_os_error(), Some(10040 | 10054)) => continue,
                    Err(error) => return Err(error.into()),
                };
                let Some(update) = protocol::decode_client(&buffer[..len]) else { continue };
                if let Some(peer) = peers.get_mut(&addr) {
                    if update.sequence > peer.latest.sequence {
                        peer.latest = update;
                        peer.last_seen = Instant::now();
                    }
                } else if peers.len() < max_clients {
                    println!("Client {next_id}: {addr}");
                    peers.insert(addr, Peer { id: next_id, last_seen: Instant::now(), latest: update });
                    next_id = next_id.checked_add(1).ok_or_else(|| anyhow::anyhow!("player ID exhausted"))?;
                }
            }
            _ = interval.tick() => {
                peers.retain(|addr, peer| {
                    let keep = peer.last_seen.elapsed() < timeout;
                    if !keep { println!("Client {} expired: {addr}", peer.id); }
                    keep
                });
                for (source, peer) in &peers {
                    let update = ServerUpdate {
                        version: VERSION,
                        player_id: peer.id,
                        sequence: peer.latest.sequence,
                        position: peer.latest.position,
                        rotation: peer.latest.rotation,
                    };
                    let bytes = protocol::encode(&update).map_err(anyhow::Error::msg)?;
                    for destination in peers.keys().filter(|addr| *addr != source) {
                        // A failed recipient must not stop relaying to other clients.
                        let _ = socket.send_to(&bytes, destination).await;
                    }
                    if echo {
                        let mirrored = ServerUpdate { player_id: ECHO_ID_BASE + peer.id, ..update };
                        let bytes = protocol::encode(&mirrored).map_err(anyhow::Error::msg)?;
                        let _ = socket.send_to(&bytes, source).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Position, Rotation};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn relays_between_two_clients_without_echo_and_ignores_stale_updates() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(run(socket, 2, Duration::from_secs(5), async {
            let _ = stopped.await;
        }));
        let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let make = |sequence, x| {
            protocol::encode(&ClientUpdate {
                version: VERSION,
                sequence,
                position: Position { x, y: 0.0, z: 0.0 },
                rotation: (x == 22.0).then_some(Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                    w: 0.0,
                }),
            })
            .unwrap()
        };
        a.send_to(&make(10, 11.0), address).await.unwrap();
        b.send_to(&make(20, 22.0), address).await.unwrap();
        a.send_to(&make(9, 999.0), address).await.unwrap();
        b.send_to(b"invalid packet", address).await.unwrap();
        let mut buffer = [0; MAX_PACKET];
        let (len, _) = time::timeout(Duration::from_secs(2), a.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        let from_b = protocol::decode_server(&buffer[..len]).unwrap();
        assert_eq!(from_b.position.x, 22.0);
        assert_eq!(from_b.rotation.map(|r| r.z), Some(1.0));
        let (len, _) = time::timeout(Duration::from_secs(2), b.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        let from_a = protocol::decode_server(&buffer[..len]).unwrap();
        assert_eq!(from_a.position.x, 11.0);
        assert!(from_a.rotation.is_none());
        assert_eq!(from_a.sequence, 10);
        assert_ne!(from_a.player_id, from_b.player_id);
        stop.send(()).unwrap();
        task.await.unwrap().unwrap();
    }
}

#[cfg(test)]
mod echo_tests {
    use super::*;
    use crate::protocol::Position;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn echo_returns_own_pose_under_a_distinct_id() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(run_with_echo(
            socket,
            2,
            Duration::from_secs(5),
            true,
            async {
                let _ = stopped.await;
            },
        ));
        let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let packet = protocol::encode(&ClientUpdate {
            version: VERSION,
            sequence: 4,
            position: Position {
                x: 7.0,
                y: 8.0,
                z: 9.0,
            },
            rotation: None,
        })
        .unwrap();
        a.send_to(&packet, address).await.unwrap();
        let mut buffer = [0; MAX_PACKET];
        let (len, _) = time::timeout(Duration::from_secs(2), a.recv_from(&mut buffer))
            .await
            .unwrap()
            .unwrap();
        let echoed = protocol::decode_server(&buffer[..len]).unwrap();
        assert_eq!(echoed.player_id, ECHO_ID_BASE + 1);
        assert_eq!((echoed.sequence, echoed.position.x), (4, 7.0));
        assert!(u32::try_from(echoed.player_id).is_ok());
        stop.send(()).unwrap();
        task.await.unwrap().unwrap();
    }
}

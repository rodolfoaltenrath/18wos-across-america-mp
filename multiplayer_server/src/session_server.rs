//! Tokio adapter for the pure room authority. All sends are nonblocking;
//! congestion drops datagrams, never queues an unbounded broadcast backlog.
use crate::{
    identity,
    protocol::{MAX_PACKET, TICK_HZ},
    session_hub::{Datagram, Hub, Metrics, Settings},
    session_protocol as wire,
};
use anyhow::Result;
use serde::Serialize;
use std::{future::Future, time::Duration};
use tokio::{
    net::UdpSocket,
    time::{self, Instant, MissedTickBehavior},
};

#[derive(Debug, Serialize)]
pub struct Report {
    pub protocol: u8,
    pub session: u64,
    pub settings: Settings,
    pub metrics: Metrics,
    pub active_on_shutdown: usize,
    pub cached_members: usize,
}
fn send(socket: &UdpSocket, hub: &mut Hub, datagrams: Vec<Datagram>) -> Result<()> {
    for (address, frame) in datagrams {
        let bytes = wire::encode_server(&frame).map_err(anyhow::Error::msg)?;
        match socket.try_send_to(&bytes, address) {
            Ok(size) => {
                hub.metrics.sent += 1;
                hub.metrics.bytes_sent += size as u64;
            }
            Err(_) => hub.metrics.send_errors += 1,
        }
    }
    Ok(())
}
pub async fn run(
    socket: UdpSocket,
    settings: Settings,
    shutdown: impl Future<Output = ()>,
) -> Result<Report> {
    let mut hub = Hub::new(
        settings,
        identity::random_nonce().map_err(anyhow::Error::msg)?,
    )
    .map_err(anyhow::Error::msg)?;
    let start = Instant::now();
    let tick = Duration::from_secs_f64(1.0 / f64::from(TICK_HZ));
    let mut interval = time::interval(tick);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_tick = start;
    let mut buffer = [0; MAX_PACKET + 1];
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                let closing = hub.closing(); send(&socket, &mut hub, closing)?;
                return Ok(Report { protocol: wire::VERSION, session: hub.session,
                    active_on_shutdown: hub.clients(), cached_members: hub.members(),
                    settings: hub.settings, metrics: hub.metrics });
            }
            received = socket.recv_from(&mut buffer) => {
                let (len, address) = match received {
                    Ok(value) => value,
                    Err(error) if error.raw_os_error() == Some(10040) => {
                        hub.metrics.received += 1; hub.metrics.oversized += 1; continue;
                    }
                    Err(error) if error.raw_os_error() == Some(10054) || error.kind() == std::io::ErrorKind::ConnectionRefused => continue,
                    Err(error) => return Err(error.into()),
                };
                let output = hub.ingest(address, &buffer[..len], start.elapsed().as_millis() as u64, identity::random_id)
                    .map_err(anyhow::Error::msg)?;
                send(&socket, &mut hub, output)?;
            }
            _ = interval.tick() => {
                let now = Instant::now();
                if now.duration_since(last_tick) > tick * 2 { hub.metrics.late_ticks += 1; }
                last_tick = now;
                let output = hub.tick(start.elapsed().as_millis() as u64);
                send(&socket, &mut hub, output)?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        protocol::Position,
        session_client::{Client, Event, Options},
        session_protocol::{ClientMessage as C, ServerMessage as S},
    };
    async fn next(
        socket: &UdpSocket,
        wanted: impl Fn(&wire::ServerFrame) -> bool,
    ) -> wire::ServerFrame {
        time::timeout(Duration::from_secs(3), async {
            let mut bytes = [0; MAX_PACKET + 1];
            loop {
                let len = socket.recv(&mut bytes).await.unwrap();
                if let Some(frame) = wire::decode_server(&bytes[..len])
                    && wanted(&frame)
                {
                    return frame;
                }
            }
        })
        .await
        .expect("timed out waiting for session frame")
    }
    #[tokio::test]
    async fn tokio_room_accepts_two_clients_relays_and_converges_after_idempotent_leave() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let endpoint = server.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(run(
            server,
            Settings {
                room: "lan".into(),
                profile: wire::PROFILE.into(),
                max_clients: 2,
                timeout_ms: 2000,
                echo: false,
            },
            async {
                let _ = stopped.await;
            },
        ));
        let mut sockets = Vec::new();
        let mut clients = Vec::new();
        for id in 1..=2 {
            let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            socket.connect(endpoint).await.unwrap();
            let mut client = Client::new(
                Options {
                    room: "lan".into(),
                    identity: format!("{id:032x}"),
                    profile: wire::PROFILE.into(),
                },
                id,
            )
            .unwrap();
            let join = client.poll(0).0.remove(0);
            socket
                .send(&wire::encode_client(&join).unwrap())
                .await
                .unwrap();
            let accepted = next(&socket, |f| matches!(f.message, S::Accepted { .. })).await;
            client.ingest(&wire::encode_server(&accepted).unwrap(), 0);
            let frame = client
                .pose(
                    Position {
                        x: id as f32 * 100.0,
                        y: 0.0,
                        z: 0.0,
                    },
                    None,
                    1,
                )
                .unwrap();
            let bytes = wire::encode_client(&frame).unwrap();
            socket.send(&bytes).await.unwrap();
            socket.send(&bytes).await.unwrap();
            sockets.push(socket);
            clients.push(client);
        }
        for index in 0..2 {
            let frame = next(&sockets[index], |f| matches!(f.message, S::Snapshot { .. })).await;
            let events = clients[index].ingest(&wire::encode_server(&frame).unwrap(), 10);
            assert!(events.iter().any(
                |e| matches!(e,Event::Pose{pose,..}if pose.position.x==(2-index)as f32*100.0)
            ));
        }
        clients[0].close(11);
        let bye = clients[0].poll(11).0.remove(0);
        assert!(matches!(bye.message, C::Leave { .. }));
        for _ in 0..2 {
            sockets[0]
                .send(&wire::encode_client(&bye).unwrap())
                .await
                .unwrap();
            let left = next(&sockets[0], |f| matches!(f.message, S::Left { .. })).await;
            clients[0].ingest(&wire::encode_server(&left).unwrap(), 12);
        }
        assert!(clients[0].status().leave_confirmed);
        let roster = next(
            &sockets[1],
            |f| matches!(&f.message,S::Roster{players,..}if players.is_empty()),
        )
        .await;
        let events = clients[1].ingest(&wire::encode_server(&roster).unwrap(), 13);
        assert!(
            events
                .iter()
                .any(|e| matches!(e,Event::Roster(ids)if ids.is_empty()))
        );
        stop.send(()).unwrap();
        let report = server.await.unwrap().unwrap();
        assert_eq!(report.metrics.accepted, 2);
        assert_eq!(report.metrics.poses, 2);
        assert!(report.metrics.stale >= 2);
        assert_eq!(report.metrics.left, 1);
        assert_eq!(report.active_on_shutdown, 1);
    }
}

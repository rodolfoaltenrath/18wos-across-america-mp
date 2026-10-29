//! One-client, two-socket UDP impairment proxy for local/LAN diagnostics.
//! No packet contents are logged and no engine or worker is called here.
use crate::network_conditions::{Conditions, Counters, Link};
use multiplayer_server::protocol::{self, MAX_PACKET};
use serde::Serialize;
use std::{
    io,
    net::{SocketAddr, UdpSocket},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const WORK_BUDGET: usize = 256;
#[derive(Clone, Debug, Serialize)]
pub struct Options {
    pub listen: SocketAddr,
    pub server: SocketAddr,
    pub duration_ms: u64,
    pub blackout_after_ms: u64,
    pub blackout_for_ms: u64,
    pub conditions: Conditions,
}
#[derive(Default)]
pub struct Controls {
    pub stop: AtomicBool,
    /// Optional in-process lab control, in addition to the scheduled outage.
    pub blackout: AtomicBool,
}
#[derive(Debug, Default, Serialize)]
pub struct Direction {
    pub received: u64,
    pub oversized: u64,
    pub transport_errors: u64,
    pub foreign: u64,
    pub invalid_initial: u64,
    pub blackout_dropped: u64,
    pub sent: u64,
    pub send_errors: u64,
    pub pending_on_stop: usize,
    pub network: Counters,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u32,
    pub real_sockets: bool,
    pub validates_game: bool,
    pub validates_lan: bool,
    pub options: Options,
    pub listen: SocketAddr,
    pub client: Option<SocketAddr>,
    /// Relative to proxy start; scheduled outages start counting at this point.
    pub client_started_ms: Option<u64>,
    pub downlink_seed: u64,
    pub elapsed_ms: u64,
    pub stopped: bool,
    pub external_blackout_used: bool,
    pub uplink: Direction,
    pub downlink: Direction,
}
pub struct Proxy {
    options: Options,
    front: UdpSocket,
    back: UdpSocket,
    uplink: Link,
    downlink: Link,
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
fn transient(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::ConnectionRefused || error.raw_os_error() == Some(10054)
}
fn scheduled_outage(options: &Options, now: u64, client_started: Option<u64>) -> bool {
    client_started.is_some_and(|started| {
        let age = now.saturating_sub(started);
        age >= options.blackout_after_ms
            && age < options.blackout_after_ms + options.blackout_for_ms
    })
}
impl Proxy {
    pub fn bind(options: Options) -> io::Result<Self> {
        options.conditions.validate().map_err(invalid)?;
        if !(1..=3_600_000).contains(&options.duration_ms)
            || options
                .blackout_after_ms
                .checked_add(options.blackout_for_ms)
                .is_none()
            || options.server.port() == 0
            || options.server.ip().is_unspecified()
            || options.server.ip().is_multicast()
            || options.listen.ip().is_multicast()
            || matches!(options.server.ip(), std::net::IpAddr::V4(ip) if ip.is_broadcast())
        {
            return Err(invalid("invalid proxy endpoint, duration or outage"));
        }
        let front = UdpSocket::bind(options.listen)?;
        let bound = front.local_addr()?;
        if bound.port() == options.server.port()
            && (bound.ip() == options.server.ip()
                || (bound.ip().is_unspecified() && options.server.ip().is_loopback()))
        {
            return Err(invalid("proxy cannot forward to its own listen endpoint"));
        }
        let back = UdpSocket::bind(if options.server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })?;
        back.connect(options.server)?;
        front.set_nonblocking(true)?;
        back.set_nonblocking(true)?;
        let uplink = Link::new(options.conditions.clone()).map_err(invalid)?;
        let downlink = Link::new(options.conditions.reverse()).map_err(invalid)?;
        Ok(Self {
            options,
            front,
            back,
            uplink,
            downlink,
        })
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.front.local_addr()
    }

    /// Finite run. Stop drops pending copies instead of sending late traffic.
    /// Each pass bounds receives and sends, including under continuous input.
    pub fn run(mut self, controls: &Controls) -> io::Result<Report> {
        let mut report = Report {
            schema: 1,
            real_sockets: true,
            validates_game: false,
            validates_lan: false,
            options: self.options.clone(),
            listen: self.local_addr()?,
            client: None,
            client_started_ms: None,
            downlink_seed: self.options.conditions.reverse().seed,
            elapsed_ms: 0,
            stopped: false,
            external_blackout_used: false,
            uplink: Direction::default(),
            downlink: Direction::default(),
        };
        let start = Instant::now();
        let mut buffer = [0u8; MAX_PACKET + 1];
        loop {
            let now = start.elapsed().as_millis() as u64;
            if controls.stop.load(Ordering::Acquire) || now >= self.options.duration_ms {
                break;
            }
            let external = controls.blackout.load(Ordering::Acquire);
            report.external_blackout_used |= external;
            let mut blocked =
                external || scheduled_outage(&self.options, now, report.client_started_ms);
            for _ in 0..WORK_BUDGET {
                let (len, source) = match self.front.recv_from(&mut buffer) {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.raw_os_error() == Some(10040) => {
                        report.uplink.oversized += 1;
                        continue;
                    }
                    Err(error) if transient(&error) => {
                        report.uplink.transport_errors += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                report.uplink.received += 1;
                if len > MAX_PACKET {
                    report.uplink.oversized += 1;
                    continue;
                }
                if let Some(client) = report.client {
                    if client != source {
                        report.uplink.foreign += 1;
                        continue;
                    }
                } else if protocol::decode_client(&buffer[..len]).is_some() {
                    report.client = Some(source);
                    report.client_started_ms = Some(now);
                    blocked =
                        external || scheduled_outage(&self.options, now, report.client_started_ms);
                } else {
                    report.uplink.invalid_initial += 1;
                    continue;
                }
                if blocked {
                    report.uplink.blackout_dropped += 1;
                    continue;
                }
                self.uplink.submit(now, &buffer[..len]).map_err(invalid)?;
            }
            for _ in 0..WORK_BUDGET {
                let len = match self.back.recv(&mut buffer) {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.raw_os_error() == Some(10040) => {
                        report.downlink.oversized += 1;
                        continue;
                    }
                    Err(error) if transient(&error) => {
                        report.downlink.transport_errors += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                report.downlink.received += 1;
                if len > MAX_PACKET {
                    report.downlink.oversized += 1;
                    continue;
                }
                if blocked {
                    report.downlink.blackout_dropped += 1;
                    continue;
                }
                self.downlink.submit(now, &buffer[..len]).map_err(invalid)?;
            }
            for _ in 0..WORK_BUDGET {
                let Some(bytes) = self.uplink.pop(now) else {
                    break;
                };
                if blocked {
                    report.uplink.blackout_dropped += 1;
                    continue;
                }
                match self.back.send(&bytes) {
                    Ok(_) => report.uplink.sent += 1,
                    Err(error)
                        if transient(&error) || error.kind() == io::ErrorKind::WouldBlock =>
                    {
                        report.uplink.send_errors += 1
                    }
                    Err(error) => return Err(error),
                }
            }
            for _ in 0..WORK_BUDGET {
                let Some(bytes) = self.downlink.pop(now) else {
                    break;
                };
                if blocked {
                    report.downlink.blackout_dropped += 1;
                    continue;
                }
                if let Some(client) = report.client {
                    match self.front.send_to(&bytes, client) {
                        Ok(_) => report.downlink.sent += 1,
                        Err(error)
                            if transient(&error) || error.kind() == io::ErrorKind::WouldBlock =>
                        {
                            report.downlink.send_errors += 1
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        report.elapsed_ms = start.elapsed().as_millis() as u64;
        report.stopped = controls.stop.load(Ordering::Acquire);
        report.uplink.network = self.uplink.counters();
        report.downlink.network = self.downlink.counters();
        report.uplink.pending_on_stop = self.uplink.pending();
        report.downlink.pending_on_stop = self.downlink.pending();
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn options(server: SocketAddr) -> Options {
        Options {
            listen: "127.0.0.1:0".parse().unwrap(),
            server,
            duration_ms: 3000,
            blackout_after_ms: 0,
            blackout_for_ms: 0,
            conditions: Conditions {
                seed: 42,
                loss_permille: 0,
                duplicate_permille: 0,
                delay_ms: 0,
                jitter_ms: 0,
                queue_capacity: 8,
            },
        }
    }
    #[test]
    fn real_sockets_pin_only_a_valid_first_client_and_keep_other_senders_out() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let proxy = Proxy::bind(options(server.local_addr().unwrap())).unwrap();
        let destination = proxy.local_addr().unwrap();
        let controls = Arc::new(Controls::default());
        let run = controls.clone();
        let worker = thread::spawn(move || proxy.run(&run).unwrap());
        let foreign = UdpSocket::bind("127.0.0.1:0").unwrap();
        foreign.send_to(b"invalid first", destination).unwrap();
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let valid = protocol::encode(&protocol::ClientUpdate {
            version: protocol::VERSION,
            sequence: 7,
            position: protocol::Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation: None,
        })
        .unwrap();
        let mut bytes = [0; MAX_PACKET + 1];
        client.send_to(&valid, destination).unwrap();
        let (len, back) = server.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..len], valid);
        foreign.send_to(&valid, destination).unwrap();
        client.send_to(&[0; 2000], destination).unwrap();
        server.send_to(b"reply", back).unwrap();
        let (len, sender) = client.recv_from(&mut bytes).unwrap();
        assert_eq!(sender, destination);
        assert_eq!(&bytes[..len], b"reply");
        // FIFO from the pinned client ensures the oversized packet was handled.
        client.send_to(&valid, destination).unwrap();
        let (len, _) = server.recv_from(&mut bytes).unwrap();
        assert_eq!(&bytes[..len], valid);
        // Wait for the foreign packet to be observed without relying on a sleep.
        controls.stop.store(true, Ordering::Release);
        let report = worker.join().unwrap();
        assert_eq!(report.client, Some(client.local_addr().unwrap()));
        assert!(report.uplink.invalid_initial + report.uplink.foreign >= 2);
        assert_eq!(report.uplink.oversized, 1);
        assert_eq!(report.uplink.sent, 2);
        assert_eq!(report.downlink.sent, 1);
        assert!(report.stopped);
    }
    #[test]
    fn invalid_options_do_not_bind_or_start_a_forwarding_loop() {
        let server: SocketAddr = "127.0.0.1:39000".parse().unwrap();
        let mut invalid_options = options(server);
        invalid_options.duration_ms = 0;
        assert!(Proxy::bind(invalid_options).is_err());
        let mut invalid_options = options(server);
        invalid_options.conditions.queue_capacity = 0;
        assert!(Proxy::bind(invalid_options).is_err());
        let mut invalid_options = options("0.0.0.0:39000".parse().unwrap());
        assert!(Proxy::bind(invalid_options.clone()).is_err());
        invalid_options.server = server;
        invalid_options.blackout_after_ms = u64::MAX;
        invalid_options.blackout_for_ms = 1;
        assert!(Proxy::bind(invalid_options).is_err());
    }

    #[test]
    fn temporarily_missing_upstream_does_not_terminate_the_proxy() {
        let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server_address = reservation.local_addr().unwrap();
        drop(reservation);
        let proxy = Proxy::bind(options(server_address)).unwrap();
        let destination = proxy.local_addr().unwrap();
        let controls = Arc::new(Controls::default());
        let run = controls.clone();
        let worker = thread::spawn(move || proxy.run(&run).unwrap());
        let client = UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(80)))
            .unwrap();
        let bytes = protocol::encode(&protocol::ClientUpdate {
            version: protocol::VERSION,
            sequence: 1,
            position: protocol::Position {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            },
            rotation: None,
        })
        .unwrap();
        client.send_to(&bytes, destination).unwrap();
        let mut buffer = [0; MAX_PACKET];
        let error = client.recv_from(&mut buffer).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
        let server = UdpSocket::bind(server_address).unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        let mut replied = false;
        for _ in 0..8 {
            client.send_to(&bytes, destination).unwrap();
            match server.recv_from(&mut buffer) {
                Ok((len, back)) => {
                    server.send_to(&buffer[..len], back).unwrap();
                    replied = true;
                    break;
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => panic!("upstream recovery: {error}"),
            }
        }
        assert!(
            replied,
            "proxy did not recover after upstream became available"
        );
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (len, _) = client.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..len], bytes);
        controls.stop.store(true, Ordering::Release);
        assert!(worker.join().unwrap().stopped);
    }
    #[test]
    fn scheduled_blackout_waits_for_first_valid_client_and_has_exact_boundaries() {
        let mut options = options("127.0.0.1:39000".parse().unwrap());
        options.blackout_after_ms = 1000;
        options.blackout_for_ms = 1500;
        assert!(!scheduled_outage(&options, 100_000, None));
        assert!(!scheduled_outage(&options, 100_999, Some(100_000)));
        assert!(scheduled_outage(&options, 101_000, Some(100_000)));
        assert!(scheduled_outage(&options, 102_499, Some(100_000)));
        assert!(!scheduled_outage(&options, 102_500, Some(100_000)));
        options.blackout_for_ms = 0;
        assert!(!scheduled_outage(&options, 101_000, Some(100_000)));
    }
}

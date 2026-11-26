//! LAN server discovery for Assetto Corsa dedicated servers.
//!
//! Combines two mechanisms:
//! 1. **ACXS LAN Beacon (`UDP 50960`):** ACXS-hosted dedicated servers can broadcast
//!    a small JSON beacon announcing their HTTP and race ports.
//! 2. **Local HTTP Probe:** Probes `127.0.0.1` (and optional LAN hosts) on common
//!    Assetto Corsa HTTP ports (`8081..=8090`) via [`crate::server_status::fetch`].

use crate::server_status::{self, ServerStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// UDP port used by ACXS LAN server beacons.
pub const BEACON_PORT: u16 = 50960;

/// Maximum beacon datagram size accepted.
const MAX_BEACON_BYTES: usize = 512;

/// Small JSON payload broadcast over UDP on the LAN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanBeacon {
    pub v: u8,
    pub name: String,
    pub http_port: u16,
    pub race_port: u16,
}

impl LanBeacon {
    pub fn new(name: impl Into<String>, http_port: u16, race_port: u16) -> Self {
        Self {
            v: 1,
            name: name.into(),
            http_port,
            race_port,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > MAX_BEACON_BYTES {
            return None;
        }
        let beacon: Self = serde_json::from_slice(bytes).ok()?;
        (beacon.v == 1 && beacon.http_port != 0).then_some(beacon)
    }
}

/// One server discovered on the local machine or LAN.
#[derive(Debug, Clone)]
pub struct DiscoveredServer {
    pub host: String,
    pub http_port: u16,
    pub race_port: Option<u16>,
    pub name: String,
    pub status: Option<ServerStatus>,
    pub latency_ms: Option<u128>,
}

/// Periodically broadcasts a [`LanBeacon`] on a background thread until dropped.
pub struct BeaconBroadcaster {
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl BeaconBroadcaster {
    /// Starts broadcasting `beacon` to `target` every `interval`.
    pub fn start(beacon: LanBeacon, target: SocketAddr, interval: Duration) -> io::Result<Self> {
        let payload = beacon
            .encode()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&shutdown);
        let handle = thread::Builder::new()
            .name("acxs-lan-beacon".into())
            .spawn(move || {
                let step = Duration::from_millis(100);
                while !stop.load(Ordering::Acquire) {
                    let _ = socket.send_to(&payload, target);
                    let mut slept = Duration::ZERO;
                    while slept < interval && !stop.load(Ordering::Acquire) {
                        thread::sleep(step.min(interval.saturating_sub(slept)));
                        slept += step;
                    }
                }
            })?;

        Ok(Self {
            shutdown,
            handle: Some(handle),
        })
    }

    pub fn stop(self) {
        // Drop handles shutdown.
    }
}

impl Drop for BeaconBroadcaster {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Listens on `bind_addr` for [`LanBeacon`] packets for `duration` and returns unique servers.
pub fn listen_beacons(bind_addr: SocketAddr, duration: Duration) -> io::Result<Vec<DiscoveredServer>> {
    let socket = UdpSocket::bind(bind_addr)?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    listen_on_socket(&socket, duration)
}

/// Reads beacons from an already-bound socket until `duration` elapses.
pub fn listen_on_socket(socket: &UdpSocket, duration: Duration) -> io::Result<Vec<DiscoveredServer>> {
    let deadline = Instant::now() + duration;
    let mut seen: HashSet<(IpAddr, u16)> = HashSet::new();
    let mut found = Vec::new();
    let mut buf = [0u8; MAX_BEACON_BYTES];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buf) {
            Ok((n, peer)) => {
                if let Some(beacon) = LanBeacon::decode(&buf[..n])
                    && seen.insert((peer.ip(), beacon.http_port))
                {
                    found.push(DiscoveredServer {
                        host: peer.ip().to_string(),
                        http_port: beacon.http_port,
                        race_port: (beacon.race_port != 0).then_some(beacon.race_port),
                        name: beacon.name,
                        status: None,
                        latency_ms: None,
                    });
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(found)
}

/// Probes `host` across `ports` via `/INFO` + `/JSON|-1` with a tight per-port timeout.
pub fn probe_host_ports(host: &str, ports: &[u16], timeout: Duration) -> Vec<DiscoveredServer> {
    let mut out = Vec::new();
    for &port in ports {
        let started = Instant::now();
        if let Ok(status) = server_status::fetch(host, port, timeout) {
            let latency_ms = started.elapsed().as_millis();
            let name = status
                .name
                .clone()
                .unwrap_or_else(|| format!("{host}:{port}"));
            let race_port = status.port;
            out.push(DiscoveredServer {
                host: host.to_string(),
                http_port: port,
                race_port,
                name,
                status: Some(status),
                latency_ms: Some(latency_ms),
            });
        }
    }
    out
}

/// Runs a combined LAN scan: listens for UDP beacons on [`BEACON_PORT`] (best-effort)
/// and probes `127.0.0.1` on ports `8081..=8090`.
pub fn scan_lan(listen_duration: Duration) -> Vec<DiscoveredServer> {
    let mut results = Vec::new();
    let mut seen: HashSet<(String, u16)> = HashSet::new();

    if let Ok(beacons) = listen_beacons(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), BEACON_PORT),
        listen_duration,
    ) {
        for srv in beacons {
            if seen.insert((srv.host.clone(), srv.http_port)) {
                results.push(srv);
            }
        }
    }

    let local_ports: Vec<u16> = (8081..=8090).collect();
    for srv in probe_host_ports("127.0.0.1", &local_ports, Duration::from_millis(250)) {
        if seen.insert((srv.host.clone(), srv.http_port)) {
            results.push(srv);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_beacon_encode_decode_and_loopback_udp_round_trip() {
        let beacon = LanBeacon::new("Home GT3 Server", 8081, 9600);
        let bytes = beacon.encode().unwrap();
        assert_eq!(LanBeacon::decode(&bytes), Some(beacon.clone()));
        assert_eq!(LanBeacon::decode(b"{}"), None);
        assert_eq!(
            LanBeacon::decode(br#"{"v":2,"name":"x","http_port":8081,"race_port":9600}"#),
            None
        );

        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        let target = receiver.local_addr().unwrap();

        let broadcaster =
            BeaconBroadcaster::start(beacon, target, Duration::from_millis(40)).unwrap();
        let found = listen_on_socket(&receiver, Duration::from_millis(250)).unwrap();
        broadcaster.stop();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].host, "127.0.0.1");
        assert_eq!(found[0].http_port, 8081);
        assert_eq!(found[0].race_port, Some(9600));
        assert_eq!(found[0].name, "Home GT3 Server");
    }
}

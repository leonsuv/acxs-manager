//! Network side of the Assetto Corsa built-in UDP telemetry protocol.
//!
//! The game listens on UDP port 9996 by default. A session runs like this:
//!   * The client sends a 12-byte handshake request (operation 0). The game
//!     answers with a 208-byte handshake response on the same socket.
//!   * [`TelemetryClient::subscribe_updates`] sends operation 1, which asks the
//!     game to push car updates.
//!   * [`TelemetryClient::send_dismiss`] sends operation 3, which ends the session.
//!
//! Every request is three little-endian `i32` values: a fixed identifier (1), a
//! protocol version (1), and the operation code. See [`telemetry::encode_request`].
//!
//! The client binds an ephemeral local port and connects to the game address, so
//! replies from any other address are dropped by the kernel.

use std::fmt;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use crate::telemetry::{self, DecodeError, Packet};

/// Default UDP port the game listens on.
pub const DEFAULT_GAME_PORT: u16 = 9996;

/// Largest datagram this client reads. Known packets are at most 328 bytes.
const RECV_BUF_LEN: usize = 1024;

/// Errors returned by [`TelemetryClient`] receive operations.
#[derive(Debug)]
pub enum ClientError {
    /// Socket-level failure other than a timeout.
    Io(io::Error),
    /// No datagram arrived within the configured timeout.
    Timeout,
    /// A datagram arrived but did not decode as a known packet.
    Decode(DecodeError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "telemetry socket error: {e}"),
            ClientError::Timeout => write!(f, "timed out waiting for telemetry from the game"),
            ClientError::Decode(e) => write!(f, "invalid telemetry packet: {e}"),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ClientError::Io(e) => Some(e),
            ClientError::Decode(e) => Some(e),
            ClientError::Timeout => None,
        }
    }
}

impl From<DecodeError> for ClientError {
    fn from(e: DecodeError) -> Self {
        ClientError::Decode(e)
    }
}

/// A UDP connection to one running Assetto Corsa instance.
///
/// The socket is connected to the game address, so the kernel drops datagrams
/// from other sources. Every receive uses the timeout given to
/// [`TelemetryClient::connect`], so no call blocks indefinitely.
#[derive(Debug)]
pub struct TelemetryClient {
    socket: UdpSocket,
}

impl TelemetryClient {
    /// Binds an ephemeral local UDP socket, connects it to `game_addr`, sets
    /// read and write timeouts to `timeout`, and sends the handshake request.
    ///
    /// Returns `InvalidInput` if `timeout` is zero, because a zero timeout
    /// would mean "block forever" to the OS.
    pub fn connect(game_addr: SocketAddr, timeout: Duration) -> io::Result<TelemetryClient> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "telemetry timeout must be greater than zero",
            ));
        }
        let bind_addr: SocketAddr = if game_addr.is_ipv4() {
            (std::net::Ipv4Addr::UNSPECIFIED, 0).into()
        } else {
            (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
        };
        let socket = UdpSocket::bind(bind_addr)?;
        socket.connect(game_addr)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;

        let client = TelemetryClient { socket };
        client.socket.send(&telemetry::encode_handshake_request())?;
        Ok(client)
    }

    /// Waits for one datagram from the game and decodes it.
    ///
    /// Returns [`ClientError::Timeout`] if nothing arrives in time, and
    /// [`ClientError::Decode`] if the datagram is not a known packet.
    pub fn recv_packet(&self) -> Result<Packet, ClientError> {
        let mut buf = [0u8; RECV_BUF_LEN];
        let n = match self.socket.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if is_timeout(&e) => return Err(ClientError::Timeout),
            Err(e) => return Err(ClientError::Io(e)),
        };
        Ok(telemetry::decode(&buf[..n])?)
    }

    /// Sends the handshake request (operation 0) again, for example when the game
    /// was not running yet when [`TelemetryClient::connect`] sent the first one.
    pub fn send_handshake(&self) -> io::Result<()> {
        self.send_request(telemetry::OP_HANDSHAKE)
    }

    /// Asks the game to send car updates (operation 1).
    ///
    /// The request layout has no field for a rate, so `interval_ms` is not sent
    /// and has no effect on the wire. The game chooses how often it sends car
    /// updates. The parameter is kept so callers can state the rate they want;
    /// the value is ignored.
    pub fn subscribe_updates(&self, interval_ms: u32) -> io::Result<()> {
        let _ = interval_ms;
        self.send_request(telemetry::OP_SUBSCRIBE_UPDATE)
    }

    /// Sends the dismiss request (operation 3), which ends the telemetry session.
    pub fn send_dismiss(&self) -> io::Result<()> {
        self.send_request(telemetry::OP_DISMISS)
    }

    /// Sends one 12-byte request with the given operation code.
    fn send_request(&self, operation: i32) -> io::Result<()> {
        let request = telemetry::encode_request(operation);
        let sent = self.socket.send(&request)?;
        if sent != request.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short write sending telemetry request",
            ));
        }
        Ok(())
    }
}

/// Read timeouts surface as `WouldBlock` on Unix and `TimedOut` on Windows.
fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
}

/// True for the errors a connected UDP socket reports when nothing listens on the
/// game port yet (an ICMP port-unreachable reply). Windows reports it as
/// `ConnectionReset` on the next receive, Linux as `ConnectionRefused`. The game
/// may simply not be running yet, so callers retry instead of giving up.
pub fn is_unreachable(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused)
}

impl ClientError {
    /// True when the game port did not answer yet; see [`is_unreachable`].
    pub fn is_unreachable(&self) -> bool {
        matches!(self, ClientError::Io(e) if is_unreachable(e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    /// A fake game socket on loopback with a bounded read timeout, so a
    /// broken test fails instead of hanging.
    fn fake_game() -> (UdpSocket, SocketAddr) {
        let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let addr = sock.local_addr().unwrap();
        (sock, addr)
    }

    /// A 208-byte handshake response with a few name fields set.
    fn handshake_response() -> Vec<u8> {
        // wchar_t[50] names (UTF-16LE): car at 0, driver at 100, track at 208, layout at 308.
        let mut b = vec![0u8; telemetry::HANDSHAKE_RESPONSE_LEN];
        for (off, text) in [(0, "ks_ferrari_488gt3"), (100, "Leon"), (208, "monza"), (308, "gp")] {
            for (i, unit) in text.encode_utf16().enumerate() {
                b[off + i * 2..off + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn connect_sends_handshake_request() {
        let (game, addr) = fake_game();
        let _client = TelemetryClient::connect(addr, Duration::from_secs(1)).unwrap();

        let mut buf = [0u8; 64];
        let (n, _) = game.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], telemetry::encode_handshake_request().as_slice());
    }

    #[test]
    fn recv_packet_decodes_handshake_response() {
        let (game, addr) = fake_game();
        let client = TelemetryClient::connect(addr, Duration::from_secs(1)).unwrap();

        let mut buf = [0u8; 64];
        let (_, from) = game.recv_from(&mut buf).unwrap();
        game.send_to(&handshake_response(), from).unwrap();

        match client.recv_packet() {
            Ok(Packet::Handshake(h)) => {
                assert_eq!(h.car, "ks_ferrari_488gt3");
                assert_eq!(h.driver, "Leon");
                assert_eq!(h.track, "monza");
                assert_eq!(h.track_config, "gp");
            }
            other => panic!("expected handshake, got {other:?}"),
        }
    }

    #[test]
    fn recv_packet_times_out_when_game_is_silent() {
        let (_game, addr) = fake_game();
        let client = TelemetryClient::connect(addr, Duration::from_millis(100)).unwrap();

        assert!(matches!(client.recv_packet(), Err(ClientError::Timeout)));
    }

    #[test]
    fn recv_packet_reports_decode_error() {
        let (game, addr) = fake_game();
        let client = TelemetryClient::connect(addr, Duration::from_secs(1)).unwrap();

        let mut buf = [0u8; 64];
        let (_, from) = game.recv_from(&mut buf).unwrap();
        game.send_to(&[0xAB; 50], from).unwrap();

        assert!(matches!(
            client.recv_packet(),
            Err(ClientError::Decode(DecodeError::Truncated))
        ));
    }

    #[test]
    fn send_dismiss_sends_operation_three() {
        let (game, addr) = fake_game();
        let client = TelemetryClient::connect(addr, Duration::from_secs(1)).unwrap();

        let mut buf = [0u8; 64];
        let _ = game.recv_from(&mut buf).unwrap(); // handshake
        client.send_dismiss().unwrap();

        let (n, _) = game.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[1, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0]);
    }

    #[test]
    fn subscribe_updates_sends_operation_one() {
        let (game, addr) = fake_game();
        let client = TelemetryClient::connect(addr, Duration::from_secs(1)).unwrap();

        let mut buf = [0u8; 64];
        let _ = game.recv_from(&mut buf).unwrap(); // handshake
        client.subscribe_updates(100).unwrap();

        let (n, _) = game.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
    }

    #[test]
    fn subscribe_updates_does_not_send_interval() {
        let (game, addr) = fake_game();
        let client = TelemetryClient::connect(addr, Duration::from_secs(1)).unwrap();

        let mut buf = [0u8; 64];
        let _ = game.recv_from(&mut buf).unwrap(); // handshake
        for interval in [0u32, 250, u32::MAX] {
            client.subscribe_updates(interval).unwrap();
            let (n, _) = game.recv_from(&mut buf).unwrap();
            assert_eq!(
                &buf[..n],
                &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0],
                "interval {interval} must not change the request"
            );
        }
    }

    #[test]
    fn connect_rejects_zero_timeout() {
        let (_game, addr) = fake_game();
        let err = TelemetryClient::connect(addr, Duration::ZERO).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}

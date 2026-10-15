//! Small local HTTP server that exposes the latest telemetry snapshot to other
//! devices, such as a phone or tablet on the same network.
//!
//! Routes:
//! - `GET /` returns an HTML page that polls `/api/latest` once per second.
//! - `GET /api/latest` returns the most recent [`Snapshot`] as JSON, or
//!   `204 No Content` before the first publish.
//! - Any other path returns `404`. Any method other than `GET` returns `405`.
//!
//! # Network exposure
//!
//! The server has no authentication and no TLS. Binding to `0.0.0.0` (or any
//! LAN address) makes live telemetry readable by every device that can reach
//! the port, and the traffic is plain HTTP. [`DEFAULT_BIND`] is loopback for
//! that reason. Callers that opt into LAN access should do so deliberately and
//! keep the port behind a firewall.
//!
//! Each request head (request line plus headers) is capped at 8 KiB, each
//! connection has read and write timeouts, and the number of concurrent
//! connections is capped.

use std::fmt::Write as _;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

/// Bind address to use unless the caller explicitly asks for LAN access.
pub const DEFAULT_BIND: &str = "127.0.0.1";

const MAX_HEAD_BYTES: usize = 8 * 1024;
const MAX_CONNECTIONS: usize = 16;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const BUSY_WRITE_TIMEOUT: Duration = Duration::from_millis(200);
const DRAIN_TIMEOUT: Duration = Duration::from_millis(250);
const DRAIN_LIMIT: usize = 64 * 1024;
const ACCEPT_POLL: Duration = Duration::from_millis(20);

const TEXT: &[(&str, &str)] = &[("Content-Type", "text/plain; charset=utf-8")];

const DASHBOARD_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Live telemetry</title>
<style>
:root {
  color-scheme: light dark;
  --bg: #f4f5f7;
  --fg: #15171a;
  --muted: #6b7280;
  --card: #ffffff;
  --accent: #d9480f;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #0f1114;
    --fg: #e8eaed;
    --muted: #9aa1ab;
    --card: #191c21;
    --accent: #ff8a4c;
  }
}
:root[data-theme="dark"] {
  --bg: #0f1114;
  --fg: #e8eaed;
  --muted: #9aa1ab;
  --card: #191c21;
  --accent: #ff8a4c;
}
body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
  font: 16px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif;
}
main { max-width: 640px; margin: 0 auto; padding: 16px; }
header {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: baseline;
  gap: 8px;
}
h1 { font-size: 1.1rem; margin: 0; }
#status { color: var(--muted); font-size: 0.9rem; }
#status.live { color: var(--accent); }
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 12px;
  margin-top: 16px;
}
.tile { background: var(--card); border-radius: 12px; padding: 12px 14px; }
.label {
  color: var(--muted);
  font-size: 0.8rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.value {
  font-size: 1.8rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  margin-top: 4px;
  overflow-wrap: anywhere;
}
.value small { font-size: 0.9rem; font-weight: 400; color: var(--muted); margin-left: 4px; }
</style>
</head>
<body>
<main>
<header>
  <h1>Live telemetry</h1>
  <span id="status">connecting...</span>
</header>
<div class="grid">
  <div class="tile"><div class="label">Speed</div><div class="value"><span id="speed">-</span><small>km/h</small></div></div>
  <div class="tile"><div class="label">Gear</div><div class="value"><span id="gear">-</span></div></div>
  <div class="tile"><div class="label">RPM</div><div class="value"><span id="rpm">-</span></div></div>
  <div class="tile"><div class="label">Throttle</div><div class="value"><span id="throttle">-</span></div></div>
  <div class="tile"><div class="label">Brake</div><div class="value"><span id="brake">-</span></div></div>
  <div class="tile"><div class="label">Lap</div><div class="value"><span id="lap">-</span></div></div>
  <div class="tile"><div class="label">Lap time</div><div class="value"><span id="lap_time">-</span></div></div>
  <div class="tile"><div class="label">Position</div><div class="value"><span id="position">-</span></div></div>
</div>
</main>
<script>
(function () {
  var statusEl = document.getElementById('status');

  function set(id, text) {
    document.getElementById(id).textContent = text;
  }
  function isNum(v) {
    return typeof v === 'number' && isFinite(v);
  }
  function pad(n, width) {
    var s = String(n);
    while (s.length < width) s = '0' + s;
    return s;
  }
  function gear(g) {
    if (!isNum(g)) return '-';
    if (g === -1) return 'R';
    if (g === 0) return 'N';
    return String(g);
  }
  function percent(v) {
    return isNum(v) ? Math.round(v * 100) + '%' : '-';
  }
  function lapTime(ms) {
    if (!isNum(ms)) return '-';
    var min = Math.floor(ms / 60000);
    var sec = Math.floor((ms % 60000) / 1000);
    var milli = Math.floor(ms % 1000);
    return min + ':' + pad(sec, 2) + '.' + pad(milli, 3);
  }
  function render(s) {
    set('speed', isNum(s.speed_kmh) ? Math.round(s.speed_kmh) : '-');
    set('gear', gear(s.gear));
    set('rpm', isNum(s.rpm) ? Math.round(s.rpm) : '-');
    set('throttle', percent(s.throttle));
    set('brake', percent(s.brake));
    set('lap', isNum(s.lap) ? s.lap : '-');
    set('lap_time', lapTime(s.lap_time_ms));
    set('position', isNum(s.position) ? s.position : '-');
  }
  function setStatus(text, live) {
    statusEl.textContent = text;
    statusEl.className = live ? 'live' : '';
  }

  async function poll() {
    try {
      var res = await fetch('/api/latest', { cache: 'no-store' });
      if (res.status === 204) {
        setStatus('waiting for data', false);
        return;
      }
      if (!res.ok) throw new Error('HTTP ' + res.status);
      render(await res.json());
      setStatus('live', true);
    } catch (err) {
      setStatus('no connection', false);
    }
  }

  poll();
  setInterval(poll, 1000);
})();
</script>
</body>
</html>
"##;

/// One telemetry sample.
///
/// Units: speed in km/h, rpm in revolutions per minute, throttle and brake
/// normalised to `0.0..=1.0`, lap time in milliseconds. Gear is `-1` for
/// reverse, `0` for neutral and `1` and up for forward gears.
#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    pub speed_kmh: f32,
    pub gear: i32,
    pub rpm: f32,
    pub throttle: f32,
    pub brake: f32,
    pub lap: u32,
    pub lap_time_ms: u32,
    pub position: u32,
}

/// A running telemetry server. Dropping it or calling [`Dashboard::stop`]
/// shuts the accept loop down and releases the port.
pub struct Dashboard {
    latest: Arc<Mutex<Option<Snapshot>>>,
    shutdown: Arc<AtomicBool>,
    local_addr: SocketAddr,
    accept_thread: Option<JoinHandle<()>>,
}

impl Dashboard {
    /// Binds `bind:port` and starts serving on a background thread.
    ///
    /// Pass [`DEFAULT_BIND`] to keep the server on this machine. Passing
    /// `"0.0.0.0"` exposes the unauthenticated, unencrypted telemetry to the
    /// whole local network; see the module docs. Port `0` picks a free port,
    /// which [`Dashboard::local_addr`] reports.
    pub fn start(bind: &str, port: u16) -> io::Result<Dashboard> {
        let listener = TcpListener::bind((bind, port))?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;

        let latest = Arc::new(Mutex::new(None));
        let shutdown = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));

        let accept_thread = thread::Builder::new()
            .name("web-dashboard-accept".into())
            .spawn({
                let latest = Arc::clone(&latest);
                let shutdown = Arc::clone(&shutdown);
                move || accept_loop(listener, latest, shutdown, active)
            })?;

        Ok(Dashboard {
            latest,
            shutdown,
            local_addr,
            accept_thread: Some(accept_thread),
        })
    }

    /// The address the server is actually bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Replaces the snapshot served by `/api/latest`.
    pub fn publish(&self, snapshot: Snapshot) {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(snapshot);
    }

    /// Stops the server. The accept loop exits and the listening port is
    /// released. Connections already being handled finish within their
    /// timeouts on their own threads.
    pub fn stop(self) {
        // Dropping the dashboard performs the shutdown.
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
    }
}

/// Counts one live connection; the count is released when the guard drops,
/// including when the handler thread fails to spawn.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn accept_loop(
    listener: TcpListener,
    latest: Arc<Mutex<Option<Snapshot>>>,
    shutdown: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
) {
    while !shutdown.load(Ordering::Acquire) {
        let stream = match listener.accept() {
            Ok((stream, _peer)) => stream,
            Err(_) => {
                thread::sleep(ACCEPT_POLL);
                continue;
            }
        };

        if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::AcqRel);
            refuse_busy(stream);
            continue;
        }

        let slot = Slot(Arc::clone(&active));
        let latest = Arc::clone(&latest);
        let _ = thread::Builder::new()
            .name("web-dashboard-conn".into())
            .spawn(move || {
                let _slot = slot;
                handle_connection(stream, &latest);
            });
    }
}

/// Answers over-limit connections with 503 without reading the request.
fn refuse_busy(mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_write_timeout(Some(BUSY_WRITE_TIMEOUT));
    let _ = write_response(
        &mut stream,
        503,
        "Service Unavailable",
        &[("Content-Type", "text/plain; charset=utf-8"), ("Retry-After", "1")],
        b"Too many connections\n",
    );
    let _ = stream.shutdown(Shutdown::Both);
}

fn handle_connection(stream: TcpStream, latest: &Mutex<Option<Snapshot>>) {
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(READ_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(WRITE_TIMEOUT)).is_err()
    {
        return;
    }

    let head = match read_head(&stream) {
        Ok(head) => head,
        Err(HeadError::Closed | HeadError::Io) => return,
        Err(HeadError::TooLarge) => {
            return reply(
                stream,
                431,
                "Request Header Fields Too Large",
                TEXT,
                b"Request head too large\n",
            );
        }
        Err(HeadError::Timeout) => {
            return reply(stream, 408, "Request Timeout", TEXT, b"Request timed out\n");
        }
        Err(HeadError::Incomplete) => {
            return reply(stream, 400, "Bad Request", TEXT, b"Incomplete request\n");
        }
    };

    let text = String::from_utf8_lossy(&head);
    let mut parts = text.lines().next().unwrap_or("").split_ascii_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return reply(stream, 400, "Bad Request", TEXT, b"Malformed request line\n");
    };
    if !version.starts_with("HTTP/") {
        return reply(stream, 400, "Bad Request", TEXT, b"Malformed request line\n");
    }
    if method != "GET" {
        return reply(
            stream,
            405,
            "Method Not Allowed",
            &[("Allow", "GET"), ("Content-Type", "text/plain; charset=utf-8")],
            b"Only GET is supported\n",
        );
    }

    let path = target.split_once('?').map_or(target, |(path, _)| path);
    match path {
        "/" => reply(
            stream,
            200,
            "OK",
            &[("Content-Type", "text/html; charset=utf-8")],
            DASHBOARD_HTML.as_bytes(),
        ),
        "/api/latest" => {
            let snapshot = *latest.lock().unwrap_or_else(PoisonError::into_inner);
            match snapshot {
                None => reply(stream, 204, "No Content", &[], b""),
                Some(snapshot) => match serde_json::to_vec(&snapshot) {
                    Ok(body) => reply(
                        stream,
                        200,
                        "OK",
                        &[("Content-Type", "application/json")],
                        &body,
                    ),
                    Err(_) => reply(
                        stream,
                        500,
                        "Internal Server Error",
                        TEXT,
                        b"Failed to encode snapshot\n",
                    ),
                },
            }
        }
        _ => reply(stream, 404, "Not Found", TEXT, b"Not found\n"),
    }
}

enum HeadError {
    /// The peer closed without sending anything.
    Closed,
    /// The peer closed before the blank line that ends the head.
    Incomplete,
    /// The head reached the 8 KiB cap without ending.
    TooLarge,
    /// A read timed out.
    Timeout,
    /// Any other read failure; the connection is dropped without a reply.
    Io,
}

/// Reads the request line and headers up to the blank line. The reader is
/// capped at [`MAX_HEAD_BYTES`], so the head can never grow past the limit.
fn read_head(stream: &TcpStream) -> Result<Vec<u8>, HeadError> {
    let mut reader = BufReader::new(Read::take(stream, MAX_HEAD_BYTES as u64));
    let mut head = Vec::new();
    loop {
        let read = match reader.read_until(b'\n', &mut head) {
            Ok(read) => read,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                return Err(HeadError::Timeout);
            }
            Err(_) => return Err(HeadError::Io),
        };
        if read == 0 {
            return Err(if head.len() >= MAX_HEAD_BYTES {
                HeadError::TooLarge
            } else if head.is_empty() {
                HeadError::Closed
            } else {
                HeadError::Incomplete
            });
        }
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            return Ok(head);
        }
    }
}

/// Writes a complete response, then closes the write side and discards any
/// unread request bytes for a short while. Without the drain, closing with
/// unread data can reset the connection before the client reads the reply.
fn reply(mut stream: TcpStream, status: u16, reason: &str, headers: &[(&str, &str)], body: &[u8]) {
    let _ = write_response(&mut stream, status, reason, headers, body);
    let _ = stream.shutdown(Shutdown::Write);
    drain(&mut stream);
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    head.push_str("Cache-Control: no-store\r\n");
    head.push_str("X-Content-Type-Options: nosniff\r\n");
    head.push_str("Connection: close\r\n");
    if status != 204 {
        let _ = write!(head, "Content-Length: {}\r\n", body.len());
    }
    head.push_str("\r\n");

    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn drain(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(DRAIN_TIMEOUT));
    let mut sink = [0u8; 4096];
    let mut total = 0usize;
    while total < DRAIN_LIMIT {
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(n) => total += n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(speed: f32, lap_time_ms: u32) -> Snapshot {
        Snapshot {
            speed_kmh: speed,
            gear: 4,
            rpm: 7350.0,
            throttle: 0.9,
            brake: 0.0,
            lap: 3,
            lap_time_ms,
            position: 2,
        }
    }

    fn loopback_server() -> Dashboard {
        Dashboard::start("127.0.0.1", 0).expect("bind loopback ephemeral port")
    }

    fn get(path: &str) -> Vec<u8> {
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").into_bytes()
    }

    /// Sends raw bytes and returns (status code, response head, body).
    fn exchange(dash: &Dashboard, request: &[u8]) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(dash.local_addr()).expect("connect to loopback");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set read timeout");
        // The server may answer and close before the whole request is written
        // (oversized case), so write errors are not fatal here.
        let _ = stream.write_all(request);
        let mut raw = Vec::new();
        // A reset after the response is fully received is not a failure either.
        let _ = stream.read_to_end(&mut raw);

        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("response has a head terminator");
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .expect("status code");
        (status, head, raw[split + 4..].to_vec())
    }

    fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
        head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim().eq_ignore_ascii_case(name).then(|| value.trim())
        })
    }

    #[test]
    fn json_route_returns_latest_snapshot() {
        let dash = loopback_server();

        dash.publish(sample(100.0, 1_000));
        dash.publish(sample(212.5, 92_417));

        let (status, head, body) = exchange(&dash, &get("/api/latest"));
        assert_eq!(status, 200);
        assert_eq!(header(&head, "Content-Type"), Some("application/json"));

        let json: serde_json::Value = serde_json::from_slice(&body).expect("valid JSON");
        assert_eq!(json["speed_kmh"], 212.5);
        assert_eq!(json["gear"], 4);
        assert_eq!(json["lap"], 3);
        assert_eq!(json["lap_time_ms"], 92_417);
        assert_eq!(json["position"], 2);

        // Query strings are ignored for routing.
        let (status, _, _) = exchange(&dash, &get("/api/latest?since=0"));
        assert_eq!(status, 200);

        dash.stop();
    }

    #[test]
    fn json_route_is_no_content_before_first_publish() {
        let dash = loopback_server();

        let (status, _, body) = exchange(&dash, &get("/api/latest"));
        assert_eq!(status, 204);
        assert!(body.is_empty());

        dash.stop();
    }

    #[test]
    fn html_route_serves_polling_page() {
        let dash = loopback_server();

        let (status, head, body) = exchange(&dash, &get("/"));
        assert_eq!(status, 200);
        assert_eq!(header(&head, "Content-Type"), Some("text/html; charset=utf-8"));

        let html = String::from_utf8(body).expect("utf-8 page");
        assert!(html.contains("/api/latest"));
        assert!(html.contains("setInterval(poll, 1000)"));

        dash.stop();
    }

    #[test]
    fn unknown_path_is_not_found() {
        let dash = loopback_server();

        let (status, _, _) = exchange(&dash, &get("/index.html"));
        assert_eq!(status, 404);

        dash.stop();
    }

    #[test]
    fn non_get_methods_are_rejected() {
        let dash = loopback_server();

        let post = b"POST /api/latest HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{}";
        let (status, head, _) = exchange(&dash, post);
        assert_eq!(status, 405);
        assert_eq!(header(&head, "Allow"), Some("GET"));

        let head_req = b"HEAD / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let (status, _, _) = exchange(&dash, head_req);
        assert_eq!(status, 405);

        dash.stop();
    }

    #[test]
    fn oversized_request_is_rejected() {
        let dash = loopback_server();

        let padding = "a".repeat(9 * 1024);
        let request = format!("GET / HTTP/1.1\r\nHost: localhost\r\nX-Pad: {padding}\r\n\r\n");
        let (status, _, _) = exchange(&dash, request.as_bytes());
        assert_eq!(status, 431);

        dash.stop();
    }

    #[test]
    fn malformed_request_line_is_rejected() {
        let dash = loopback_server();

        let (status, _, _) = exchange(&dash, b"NOT A REQUEST\r\n\r\n");
        assert_eq!(status, 400);

        dash.stop();
    }

    #[test]
    fn stop_releases_the_port() {
        let dash = loopback_server();
        let addr = dash.local_addr();

        dash.stop();

        assert!(TcpStream::connect(addr).is_err(), "port should be closed after stop");
    }
}

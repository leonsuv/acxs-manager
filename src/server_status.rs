//! Status queries for a running Assetto Corsa server.
//!
//! The server serves plain HTTP GET endpoints on its HTTP port:
//!   * `/INFO` returns a JSON object with the server name, track, session and
//!     client counts.
//!   * `/JSON|-1` returns the entry list as `{"Cars": [...]}`. Each car may carry
//!     a driver name and a connected flag.
//!
//! [`fetch`] makes both requests within one time budget. It speaks HTTP/1.1
//! directly over `std::net::TcpStream` and sends `Connection: close`. A response
//! ends at its Content-Length, at the end of a chunked body, or at EOF.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

/// Largest raw response accepted from a server, counting status line, headers and body.
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

const INFO_PATH: &str = "/INFO";
const ENTRY_LIST_PATH: &str = "/JSON|-1";

/// One slot of the server's entry list.
#[derive(Debug, Clone, Default)]
pub struct ServerCar {
    /// Car folder name, e.g. `ks_audi_r8_lms`.
    pub model: String,
    pub skin: String,
    /// Driver in the slot, empty when the slot is free.
    pub driver: String,
    pub connected: bool,
}

/// Snapshot of a server's status as reported over HTTP.
#[derive(Debug, Clone, Default)]
pub struct ServerStatus {
    /// True when the server answered the status request.
    pub online: bool,
    /// Server name as configured on the server.
    pub name: Option<String>,
    /// Track identifier currently loaded.
    pub track: Option<String>,
    /// Number of connected clients.
    pub clients: Option<u32>,
    /// Configured client limit.
    pub max_clients: Option<u32>,
    /// Session type code as reported by the server, kept as text (for example `"3"`).
    /// A server that reports the session as a string has that string kept as is.
    pub session: Option<String>,
    /// Driver names of connected cars, in entry-list order.
    pub players: Vec<String>,
    /// The port the race itself runs on (`port` in `/INFO`), as `acs.exe` needs it.
    pub port: Option<u16>,
    /// The entry list: every slot with its car, skin and driver.
    pub cars: Vec<ServerCar>,
    /// Feature flags the server announces in its entry list (`WEATHERFX_V1`, `STEAM_TICKET`, ...).
    pub features: Vec<String>,
    /// The `/INFO` reply as the server sent it.
    pub raw_info: String,
    /// The entry-list reply as the server sent it.
    pub raw_entries: String,
}

/// Errors returned by [`fetch`] and [`parse_status`].
#[derive(Debug)]
pub enum StatusError {
    /// The host is empty or contains characters that are not allowed in a host name.
    InvalidHost(String),
    /// Connecting, reading or writing failed for a reason other than a timeout.
    Io(io::Error),
    /// The overall time budget ran out.
    Timeout,
    /// The server answered with a status other than 200.
    HttpStatus(u16),
    /// The response could not be understood as HTTP/1.1.
    MalformedResponse(&'static str),
    /// The response was larger than the size cap.
    ResponseTooLarge,
    /// The body was not valid JSON.
    Json(serde_json::Error),
    /// A JSON field had an unexpected type or an out-of-range value.
    InvalidField(&'static str),
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatusError::InvalidHost(host) => write!(f, "invalid server host {host:?}"),
            StatusError::Io(e) => write!(f, "server connection failed: {e}"),
            StatusError::Timeout => write!(f, "server did not respond in time"),
            StatusError::HttpStatus(code) => write!(f, "server returned HTTP status {code}"),
            StatusError::MalformedResponse(why) => write!(f, "malformed HTTP response: {why}"),
            StatusError::ResponseTooLarge => {
                write!(f, "server response exceeds {MAX_RESPONSE_BYTES} bytes")
            }
            StatusError::Json(e) => write!(f, "server returned invalid JSON: {e}"),
            StatusError::InvalidField(field) => {
                write!(f, "status field `{field}` has an unexpected value")
            }
        }
    }
}

impl std::error::Error for StatusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StatusError::Io(e) => Some(e),
            StatusError::Json(e) => Some(e),
            _ => None,
        }
    }
}

/// Queries a server's `/INFO` and entry-list endpoints and combines them.
///
/// Both requests share one budget of `timeout`, so the call never blocks longer
/// than that. Any non-200 response, malformed HTTP, oversized body or invalid JSON
/// is returned as an error.
pub fn fetch(host: &str, port: u16, timeout: Duration) -> Result<ServerStatus, StatusError> {
    validate_host(host)?;
    let budget = Budget { started: Instant::now(), total: timeout };

    let info = utf8_body(get(host, port, INFO_PATH, &budget)?)?;
    let mut status = parse_status(&info)?;
    status.raw_info = info;

    let list = utf8_body(get(host, port, ENTRY_LIST_PATH, &budget)?)?;
    status.players = parse_players(&list)?;
    status.cars = parse_cars(&list);
    status.features = parse_features(&list);
    status.raw_entries = list;
    Ok(status)
}

/// Parses the JSON returned by `/INFO` into a [`ServerStatus`].
///
/// Missing or `null` fields become `None`. A field with the wrong JSON type, or a
/// count that does not fit in `u32`, is an error. `players` is left empty because
/// `/INFO` does not list drivers. The result has `online` set to `true`.
pub fn parse_status(json: &str) -> Result<ServerStatus, StatusError> {
    let value: Value = serde_json::from_str(json).map_err(StatusError::Json)?;
    let obj = value.as_object().ok_or(StatusError::InvalidField("root"))?;

    Ok(ServerStatus {
        online: true,
        name: opt_string(obj, "name")?,
        track: opt_string(obj, "track")?,
        clients: opt_u32(obj, "clients")?,
        max_clients: opt_u32(obj, "maxclients")?,
        session: opt_session(obj, "session")?,
        port: opt_u32(obj, "port")?.and_then(|p| u16::try_from(p).ok()),
        ..ServerStatus::default()
    })
}

/// Every slot of the entry list. Slots that do not parse are skipped; this is display only.
fn parse_cars(json: &str) -> Vec<ServerCar> {
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let Some(Value::Array(cars)) = obj.get("Cars") else { return Vec::new() };
    let text = |car: &Map<String, Value>, key: &str| car.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    cars.iter()
        .filter_map(Value::as_object)
        .map(|car| ServerCar {
            model: text(car, "Model"),
            // "black/ADAn": the part after the slash is a marker added by the server, not a folder.
            skin: text(car, "Skin").split('/').next().unwrap_or("").to_string(),
            driver: text(car, "DriverName"),
            connected: car.get("IsConnected").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect()
}

/// Parses the entry-list JSON and returns names of connected, named drivers.
fn parse_players(json: &str) -> Result<Vec<String>, StatusError> {
    let value: Value = serde_json::from_str(json).map_err(StatusError::Json)?;
    let obj = value.as_object().ok_or(StatusError::InvalidField("root"))?;

    let cars = match obj.get("Cars") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(cars)) => cars,
        Some(_) => return Err(StatusError::InvalidField("Cars")),
    };

    let mut players = Vec::new();
    for car in cars {
        let car = car.as_object().ok_or(StatusError::InvalidField("Cars"))?;
        let connected = match car.get("IsConnected") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(flag)) => *flag,
            Some(_) => return Err(StatusError::InvalidField("IsConnected")),
        };
        if !connected {
            continue;
        }
        if let Some(name) = opt_string(car, "DriverName")? && !name.is_empty() {
            players.push(name);
        }
    }
    Ok(players)
}

fn opt_string(obj: &Map<String, Value>, key: &'static str) -> Result<Option<String>, StatusError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(StatusError::InvalidField(key)),
    }
}

fn opt_u32(obj: &Map<String, Value>, key: &'static str) -> Result<Option<u32>, StatusError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or(StatusError::InvalidField(key)),
    }
}

/// Session may arrive as an integer code or as a string. Either is kept as text.
fn opt_session(obj: &Map<String, Value>, key: &'static str) -> Result<Option<String>, StatusError> {
    match obj.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) if n.is_u64() => Ok(Some(n.to_string())),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(StatusError::InvalidField(key)),
    }
}

fn utf8_body(body: Vec<u8>) -> Result<String, StatusError> {
    String::from_utf8(body).map_err(|_| StatusError::MalformedResponse("body is not UTF-8"))
}

/// Host names are restricted to characters that cannot break the request line or headers.
fn validate_host(host: &str) -> Result<(), StatusError> {
    let bare = bare_host(host);
    let allowed = !bare.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'[' | b']'));
    if allowed {
        Ok(())
    } else {
        Err(StatusError::InvalidHost(host.to_owned()))
    }
}

/// Strips the brackets from an IPv6 literal such as `[::1]`.
fn bare_host(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

fn host_header(bare: &str, port: u16) -> String {
    if bare.contains(':') {
        format!("[{bare}]:{port}")
    } else {
        format!("{bare}:{port}")
    }
}

/// Deadline shared by every step of one [`fetch`] call.
struct Budget {
    started: Instant,
    total: Duration,
}

impl Budget {
    /// Time left, or `Timeout` once the budget is spent. Never returns zero.
    fn remaining(&self) -> Result<Duration, StatusError> {
        self.total
            .checked_sub(self.started.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or(StatusError::Timeout)
    }
}

fn map_io(e: io::Error) -> StatusError {
    match e.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => StatusError::Timeout,
        _ => StatusError::Io(e),
    }
}

/// Performs one GET and returns the decoded body of a 200 response.
fn get(host: &str, port: u16, path: &str, budget: &Budget) -> Result<Vec<u8>, StatusError> {
    let bare = bare_host(host);
    let addrs = resolve(bare, port, budget.remaining()?)?;
    let mut stream = connect(&addrs, budget)?;

    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        host_header(bare, port)
    );
    stream.set_write_timeout(Some(budget.remaining()?)).map_err(map_io)?;
    stream.write_all(request.as_bytes()).map_err(map_io)?;

    let raw = read_response(&mut stream, budget)?;
    parse_response(&raw)
}

/// Resolves a host name or IP literal. The standard resolver has no timeout, so
/// name lookups run on a helper thread. A lookup that overruns the budget is
/// abandoned, and the thread ends when the resolver returns.
fn resolve(host: &str, port: u16, budget: Duration) -> Result<Vec<SocketAddr>, StatusError> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    let (tx, rx) = mpsc::channel::<io::Result<Vec<SocketAddr>>>();
    let owned = host.to_owned();
    thread::Builder::new()
        .name("server-status-resolve".into())
        .spawn(move || {
            let result = (owned.as_str(), port)
                .to_socket_addrs()
                .map(|addrs| addrs.collect::<Vec<SocketAddr>>());
            let _ = tx.send(result);
        })
        .map_err(StatusError::Io)?;

    match rx.recv_timeout(budget) {
        Ok(Ok(addrs)) if !addrs.is_empty() => Ok(addrs),
        Ok(Ok(_)) => Err(StatusError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "host resolved to no addresses",
        ))),
        Ok(Err(e)) => Err(map_io(e)),
        Err(RecvTimeoutError::Timeout) => Err(StatusError::Timeout),
        Err(RecvTimeoutError::Disconnected) => {
            Err(StatusError::Io(io::Error::other("resolver thread exited")))
        }
    }
}

fn connect(addrs: &[SocketAddr], budget: &Budget) -> Result<TcpStream, StatusError> {
    let mut last_error = io::Error::other("no addresses to connect to");
    for addr in addrs {
        match TcpStream::connect_timeout(addr, budget.remaining()?) {
            Ok(stream) => return Ok(stream),
            Err(e) => last_error = e,
        }
    }
    Err(map_io(last_error))
}

/// Reads until EOF, the declared Content-Length, or the size cap. Each read waits
/// at most the time left in the budget.
fn read_response(stream: &mut TcpStream, budget: &Budget) -> Result<Vec<u8>, StatusError> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        stream.set_read_timeout(Some(budget.remaining()?)).map_err(map_io)?;
        match stream.read(&mut buf) {
            Ok(0) => return Ok(raw),
            Ok(n) => {
                if raw.len() + n > MAX_RESPONSE_BYTES {
                    return Err(StatusError::ResponseTooLarge);
                }
                raw.extend_from_slice(&buf[..n]);
                if declared_length_reached(&raw) {
                    return Ok(raw);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(map_io(e)),
        }
    }
}

/// True when the headers are complete and the body has reached a Content-Length.
fn declared_length_reached(raw: &[u8]) -> bool {
    let Some(head_end) = find_subslice(raw, b"\r\n\r\n") else {
        return false;
    };
    match parse_head(&raw[..head_end]) {
        Ok(framing) if !framing.chunked => framing
            .content_length
            .is_some_and(|len| raw.len() - (head_end + 4) >= len),
        _ => false,
    }
}

/// Framing information taken from the status line and headers.
struct Framing {
    status: u16,
    content_length: Option<usize>,
    chunked: bool,
    /// Set when a Content-Encoding or Transfer-Encoding other than identity or
    /// chunked is present. Such bodies are not decoded.
    unsupported_encoding: bool,
}

fn parse_head(head: &[u8]) -> Result<Framing, StatusError> {
    let text = std::str::from_utf8(head)
        .map_err(|_| StatusError::MalformedResponse("headers are not UTF-8"))?;
    let mut lines = text.split("\r\n");

    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(StatusError::MalformedResponse("unsupported HTTP version"));
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(StatusError::MalformedResponse("invalid status code"))?;

    let mut framing = Framing {
        status,
        content_length: None,
        chunked: false,
        unsupported_encoding: false,
    };
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or(StatusError::MalformedResponse("invalid header line"))?;
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                let len = value
                    .parse::<usize>()
                    .map_err(|_| StatusError::MalformedResponse("invalid Content-Length"))?;
                framing.content_length = Some(len);
            }
            "transfer-encoding" => {
                if value.eq_ignore_ascii_case("chunked") {
                    framing.chunked = true;
                } else if !value.eq_ignore_ascii_case("identity") {
                    framing.unsupported_encoding = true;
                }
            }
            "content-encoding" if !value.eq_ignore_ascii_case("identity") => {
                framing.unsupported_encoding = true;
            }
            _ => {}
        }
    }
    Ok(framing)
}

/// Splits a complete raw response and returns its body. Only 200 is accepted.
fn parse_response(raw: &[u8]) -> Result<Vec<u8>, StatusError> {
    let head_end = find_subslice(raw, b"\r\n\r\n")
        .ok_or(StatusError::MalformedResponse("missing end of headers"))?;
    let framing = parse_head(&raw[..head_end])?;
    if framing.status != 200 {
        return Err(StatusError::HttpStatus(framing.status));
    }
    if framing.unsupported_encoding {
        return Err(StatusError::MalformedResponse("unsupported body encoding"));
    }

    let body = &raw[head_end + 4..];
    if framing.chunked {
        return decode_chunked(body);
    }
    match framing.content_length {
        Some(len) if body.len() < len => {
            Err(StatusError::MalformedResponse("body shorter than Content-Length"))
        }
        Some(len) => Ok(body[..len].to_vec()),
        None => Ok(body.to_vec()),
    }
}

/// Decodes a chunked body. Chunk extensions are ignored and trailers are skipped.
fn decode_chunked(mut body: &[u8]) -> Result<Vec<u8>, StatusError> {
    let mut out = Vec::new();
    loop {
        let line_end = find_subslice(body, b"\r\n")
            .ok_or(StatusError::MalformedResponse("truncated chunk size"))?;
        let size_line = std::str::from_utf8(&body[..line_end])
            .map_err(|_| StatusError::MalformedResponse("chunk size is not UTF-8"))?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| StatusError::MalformedResponse("invalid chunk size"))?;
        body = &body[line_end + 2..];

        if size == 0 {
            return Ok(out);
        }
        if size > body.len() || body.len() - size < 2 {
            return Err(StatusError::MalformedResponse("truncated chunk"));
        }
        if out.len() + size > MAX_RESPONSE_BYTES {
            return Err(StatusError::ResponseTooLarge);
        }
        out.extend_from_slice(&body[..size]);
        if &body[size..size + 2] != b"\r\n" {
            return Err(StatusError::MalformedResponse("chunk not followed by CRLF"));
        }
        body = &body[size + 2..];
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    const INFO_JSON: &str = r#"{"cars":["ks_ferrari_488_gt3"],"clients":2,"country":[],
        "cport":9600,"durations":[],"extra":false,"inverted":0,"ip":"","json":null,"l":false,
        "maxclients":20,"name":"Test Server","pass":false,"pickup":true,"pit":false,"port":9600,
        "session":3,"sessiontypes":[3],"timed":false,"timeleft":0,"timeofday":0,"timestamp":0,
        "tport":9600,"track":"monza"}"#;

    const LIST_JSON: &str = r#"{"Features":[],"Cars":[
        {"Model":"ks_ferrari_488_gt3","Skin":"red","DriverName":"Ada Lovelace","IsConnected":true},
        {"Model":"ks_ferrari_488_gt3","Skin":"blue","DriverName":"Grace Hopper","IsConnected":false},
        {"Model":"ks_ferrari_488_gt3","Skin":"white","DriverName":"","IsConnected":true},
        {"Model":"ks_ferrari_488_gt3","Skin":"black","DriverName":"Alan Turing","IsConnected":true}
    ]}"#;

    fn ok_response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .into_bytes()
    }

    fn chunked_response(body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            body.len(),
            body
        )
        .into_bytes()
    }

    fn request_path(stream: &mut TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(n) if n > 2 => continue,
                _ => break,
            }
        }
        line.split_whitespace().nth(1).unwrap_or("").to_owned()
    }

    /// Serves `count` connections on loopback. `respond` maps the request path to raw bytes.
    fn serve<F>(count: usize, respond: F) -> u16
    where
        F: Fn(&str) -> Vec<u8> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || {
            for _ in 0..count {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let path = request_path(&mut stream);
                let _ = stream.write_all(&respond(&path));
            }
        });
        port
    }

    // parse_status

    #[test]
    fn default_status_is_offline_and_empty() {
        let status = ServerStatus::default();
        assert!(!status.online);
        assert!(status.name.is_none());
        assert!(status.players.is_empty());
    }

    #[test]
    fn parse_status_reads_all_fields() {
        let status = parse_status(INFO_JSON).expect("valid info");
        assert!(status.online);
        assert_eq!(status.name.as_deref(), Some("Test Server"));
        assert_eq!(status.track.as_deref(), Some("monza"));
        assert_eq!(status.clients, Some(2));
        assert_eq!(status.max_clients, Some(20));
        assert_eq!(status.session.as_deref(), Some("3"));
        assert!(status.players.is_empty());
    }

    #[test]
    fn parse_status_missing_and_null_fields_are_none() {
        let status = parse_status(r#"{"name":null}"#).expect("sparse info");
        assert!(status.online);
        assert!(status.name.is_none());
        assert!(status.track.is_none());
        assert!(status.clients.is_none());
        assert!(status.max_clients.is_none());
        assert!(status.session.is_none());
    }

    #[test]
    fn parse_status_accepts_string_session() {
        let status = parse_status(r#"{"session":"Race"}"#).expect("string session");
        assert_eq!(status.session.as_deref(), Some("Race"));
    }

    #[test]
    fn parse_status_rejects_invalid_json() {
        assert!(matches!(parse_status("{"), Err(StatusError::Json(_))));
    }

    #[test]
    fn parse_status_rejects_wrong_types_and_ranges() {
        assert!(matches!(
            parse_status(r#"{"clients":"3"}"#),
            Err(StatusError::InvalidField("clients"))
        ));
        assert!(matches!(
            parse_status(r#"{"clients":-1}"#),
            Err(StatusError::InvalidField("clients"))
        ));
        assert!(matches!(
            parse_status(r#"{"maxclients":4294967296}"#),
            Err(StatusError::InvalidField("maxclients"))
        ));
        assert!(matches!(
            parse_status(r#"{"name":5}"#),
            Err(StatusError::InvalidField("name"))
        ));
        assert!(matches!(parse_status("[]"), Err(StatusError::InvalidField("root"))));
    }

    // parse_players

    #[test]
    fn parse_players_keeps_connected_named_drivers_in_order() {
        let players = parse_players(LIST_JSON).expect("valid list");
        assert_eq!(players, vec!["Ada Lovelace".to_owned(), "Alan Turing".to_owned()]);
    }

    #[test]
    fn parse_players_missing_or_null_cars_is_empty() {
        assert!(parse_players(r#"{"Cars":null}"#).expect("null").is_empty());
        assert!(parse_players("{}").expect("missing").is_empty());
    }

    #[test]
    fn parse_players_rejects_non_object_car() {
        assert!(matches!(
            parse_players(r#"{"Cars":[1]}"#),
            Err(StatusError::InvalidField("Cars"))
        ));
    }

    #[test]
    fn parse_players_rejects_non_boolean_connected_flag() {
        assert!(matches!(
            parse_players(r#"{"Cars":[{"IsConnected":"yes","DriverName":"x"}]}"#),
            Err(StatusError::InvalidField("IsConnected"))
        ));
    }

    // decode_chunked

    #[test]
    fn decode_chunked_joins_chunks_and_ignores_extensions() {
        let body = b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n";
        assert_eq!(decode_chunked(body).expect("chunked"), b"Wikipedia");
    }

    #[test]
    fn decode_chunked_rejects_bad_size() {
        assert!(matches!(
            decode_chunked(b"zz\r\nab\r\n0\r\n\r\n"),
            Err(StatusError::MalformedResponse(_))
        ));
    }

    #[test]
    fn decode_chunked_rejects_truncated_chunk() {
        assert!(matches!(
            decode_chunked(b"5\r\nab"),
            Err(StatusError::MalformedResponse(_))
        ));
    }

    #[test]
    fn decode_chunked_enforces_size_cap() {
        let mut body = format!("{:x}\r\n", MAX_RESPONSE_BYTES).into_bytes();
        body.extend(std::iter::repeat_n(b'a', MAX_RESPONSE_BYTES));
        body.extend_from_slice(b"\r\n1\r\nx\r\n0\r\n\r\n");
        assert!(matches!(decode_chunked(&body), Err(StatusError::ResponseTooLarge)));
    }

    // parse_response

    #[test]
    fn parse_response_returns_body_up_to_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabcJUNK";
        assert_eq!(parse_response(raw).expect("200"), b"abc");
    }

    #[test]
    fn parse_response_non_200_is_http_status_error() {
        let raw = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
        assert!(matches!(parse_response(raw), Err(StatusError::HttpStatus(404))));
        let raw = b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
        assert!(matches!(parse_response(raw), Err(StatusError::HttpStatus(500))));
    }

    #[test]
    fn parse_response_short_body_is_malformed() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc";
        assert!(matches!(parse_response(raw), Err(StatusError::MalformedResponse(_))));
    }

    #[test]
    fn parse_response_rejects_non_http_and_compressed_bodies() {
        assert!(matches!(
            parse_response(b"SSH-2.0-OpenSSH\r\n\r\n"),
            Err(StatusError::MalformedResponse(_))
        ));
        let raw = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 1\r\n\r\nx";
        assert!(matches!(parse_response(raw), Err(StatusError::MalformedResponse(_))));
    }

    #[test]
    fn host_header_brackets_ipv6_literals() {
        assert_eq!(host_header("127.0.0.1", 9600), "127.0.0.1:9600");
        assert_eq!(host_header("::1", 80), "[::1]:80");
    }

    #[test]
    fn fetch_rejects_invalid_host_without_connecting() {
        assert!(matches!(
            fetch("bad host\r\nX: y", 9600, Duration::from_secs(1)),
            Err(StatusError::InvalidHost(_))
        ));
        assert!(matches!(
            fetch("", 9600, Duration::from_secs(1)),
            Err(StatusError::InvalidHost(_))
        ));
    }

    // fetch over loopback

    #[test]
    fn fetch_reads_info_and_players_over_loopback() {
        let port = serve(2, |path| match path {
            "/INFO" => ok_response(INFO_JSON),
            "/JSON|-1" => ok_response(LIST_JSON),
            _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
        });
        let status = fetch("127.0.0.1", port, Duration::from_secs(5)).expect("fetch");
        assert!(status.online);
        assert_eq!(status.name.as_deref(), Some("Test Server"));
        assert_eq!(status.clients, Some(2));
        assert_eq!(status.max_clients, Some(20));
        assert_eq!(status.players, vec!["Ada Lovelace".to_owned(), "Alan Turing".to_owned()]);
    }

    #[test]
    fn fetch_maps_non_200_to_error() {
        let port = serve(1, |_| b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec());
        assert!(matches!(
            fetch("127.0.0.1", port, Duration::from_secs(5)),
            Err(StatusError::HttpStatus(503))
        ));
    }

    #[test]
    fn fetch_accepts_chunked_bodies() {
        let port = serve(2, |path| match path {
            "/INFO" => chunked_response(INFO_JSON),
            _ => chunked_response(LIST_JSON),
        });
        let status = fetch("127.0.0.1", port, Duration::from_secs(5)).expect("chunked fetch");
        assert_eq!(status.track.as_deref(), Some("monza"));
        assert_eq!(status.players.len(), 2);
    }

    #[test]
    fn fetch_rejects_oversized_response() {
        let port = serve(1, |_| {
            let mut out = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
            out.extend(std::iter::repeat_n(b'a', MAX_RESPONSE_BYTES + 4096));
            out
        });
        assert!(matches!(
            fetch("127.0.0.1", port, Duration::from_secs(5)),
            Err(StatusError::ResponseTooLarge)
        ));
    }

    #[test]
    fn fetch_times_out_on_silent_server() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || {
            // Accept and hold the connection open without answering.
            if let Ok((_stream, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(5));
            }
        });

        let started = Instant::now();
        let result = fetch("127.0.0.1", port, Duration::from_millis(300));
        let elapsed = started.elapsed();
        assert!(matches!(result, Err(StatusError::Timeout)), "got {result:?}");
        assert!(elapsed < Duration::from_secs(3), "blocked for {elapsed:?}");
    }

    #[test]
    fn fetch_refuses_when_nothing_listens() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
            listener.local_addr().expect("local addr").port()
        };
        let result = fetch("127.0.0.1", port, Duration::from_secs(2));
        assert!(matches!(result, Err(StatusError::Io(_)) | Err(StatusError::Timeout)));
    }
}

/// The `Features` list of the entry-list reply; servers with Custom Shaders Patch support send it.
fn parse_features(json: &str) -> Vec<String> {
    let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    match obj.get("Features") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .filter(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Parsed server track identifier, including any CSP minimum build prefix (`csp/<build>/../`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTrack {
    pub track_id: String,
    pub layout: Option<String>,
    pub min_csp_build: Option<u32>,
}

/// Splits a raw server track string such as `csp/2651/../la_canyons-freeroam` into
/// its track folder id, optional layout, and optional required CSP build number.
///
/// Pass `installed_track_ids` (or `&[]`) so a track whose folder name itself contains a dash
/// (e.g. `my-track`) is kept intact when installed.
pub fn resolve_track_id(raw: &str, installed_track_ids: &[&str]) -> ResolvedTrack {
    let mut name = raw.trim();
    let mut min_csp_build: Option<u32> = None;

    while let Some(rest) = name.strip_prefix("csp/") {
        match rest.split_once("/../") {
            Some((version, tail))
                if !version.is_empty() && version.chars().all(|c| c.is_ascii_digit()) =>
            {
                if let Ok(build) = version.parse::<u32>()
                    && build > 0
                {
                    min_csp_build = Some(min_csp_build.map_or(build, |prev| prev.max(build)));
                }
                name = tail;
            }
            _ => break,
        }
    }

    let name = name.rsplit('/').next().unwrap_or(name);
    if installed_track_ids.contains(&name) {
        return ResolvedTrack {
            track_id: name.to_string(),
            layout: None,
            min_csp_build,
        };
    }
    match name.split_once('-') {
        Some((id, layout)) => ResolvedTrack {
            track_id: id.to_string(),
            layout: Some(layout.to_string()),
            min_csp_build,
        },
        None => ResolvedTrack {
            track_id: name.to_string(),
            layout: None,
            min_csp_build,
        },
    }
}

/// Parameters for an Assetto Corsa HTTP `/SUB` booking registration request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookingRequest {
    pub driver_name: String,
    pub steam_guid: String,
    pub car_model: String,
    pub car_skin: String,
    pub password: String,
}

/// Outcome of a successful `/SUB` booking request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookingReply {
    pub accepted: bool,
    pub message: String,
    pub booking_time_sec: Option<u32>,
}

/// Booking-specific rejection or transport error.
#[derive(Debug)]
pub enum BookingError {
    Closed,
    CarUnavailable,
    WrongPassword,
    AlreadyBooked,
    Rejected(String),
    Transport(StatusError),
}

impl fmt::Display for BookingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookingError::Closed => write!(f, "booking session is closed"),
            BookingError::CarUnavailable => write!(f, "requested car slot is unavailable"),
            BookingError::WrongPassword => write!(f, "wrong server password"),
            BookingError::AlreadyBooked => write!(f, "already booked on this server"),
            BookingError::Rejected(msg) => write!(f, "booking rejected: {msg}"),
            BookingError::Transport(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BookingError {}

/// Sends an HTTP `/SUB` booking registration request to `host:port`.
pub fn book(
    host: &str,
    port: u16,
    req: &BookingRequest,
    timeout: Duration,
) -> Result<BookingReply, BookingError> {
    validate_host(host).map_err(BookingError::Transport)?;
    let budget = Budget {
        started: Instant::now(),
        total: timeout,
    };
    let path = format!(
        "/SUB?name={}&guid={}&car={}&skin={}&pass={}",
        crate::uri::percent_encode(&req.driver_name),
        crate::uri::percent_encode(&req.steam_guid),
        crate::uri::percent_encode(&req.car_model),
        crate::uri::percent_encode(&req.car_skin),
        crate::uri::percent_encode(&req.password),
    );
    let raw = get(host, port, &path, &budget).map_err(BookingError::Transport)?;
    let text = utf8_body(raw).map_err(BookingError::Transport)?;
    parse_booking_reply(&text)
}

/// Parses a `/SUB` reply (either plain text such as `OK` / `BOOKING_CLOSED` or JSON).
pub fn parse_booking_reply(body: &str) -> Result<BookingReply, BookingError> {
    let trimmed = body.trim();
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(trimmed) {
        let status = obj
            .get("status")
            .or_else(|| obj.get("result"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let msg = obj
            .get("message")
            .or_else(|| obj.get("error"))
            .and_then(Value::as_str)
            .unwrap_or(status)
            .to_string();
        let time = obj
            .get("time")
            .or_else(|| obj.get("timeleft"))
            .and_then(Value::as_u64)
            .and_then(|t| u32::try_from(t).ok());
        if status.eq_ignore_ascii_case("OK") || obj.get("ok").and_then(Value::as_bool) == Some(true)
        {
            return Ok(BookingReply {
                accepted: true,
                message: if msg.is_empty() { "OK".into() } else { msg },
                booking_time_sec: time,
            });
        }
        return Err(classify_booking_error(&msg));
    }

    let upper = trimmed.to_ascii_uppercase();
    if upper == "OK" || upper.starts_with("OK|") || upper.starts_with("BOOKED") {
        let time = trimmed
            .split('|')
            .nth(1)
            .and_then(|s| s.trim().parse::<u32>().ok());
        return Ok(BookingReply {
            accepted: true,
            message: trimmed.to_string(),
            booking_time_sec: time,
        });
    }
    Err(classify_booking_error(trimmed))
}

fn classify_booking_error(msg: &str) -> BookingError {
    let upper = msg.to_ascii_uppercase();
    if upper.contains("CLOSED") || upper.contains("NOT_BOOKING") {
        BookingError::Closed
    } else if upper.contains("FULL") || upper.contains("UNAVAILABLE") || upper.contains("NO_SLOT") {
        BookingError::CarUnavailable
    } else if upper.contains("PASS") || upper.contains("DENIED") {
        BookingError::WrongPassword
    } else if upper.contains("ALREADY") {
        BookingError::AlreadyBooked
    } else {
        BookingError::Rejected(msg.to_string())
    }
}

#[cfg(test)]
mod booking_and_track_tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    fn serve_once(body: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                loop {
                    let mut h = String::new();
                    match reader.read_line(&mut h) {
                        Ok(n) if n > 2 => continue,
                        _ => break,
                    }
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        port
    }

    #[test]
    fn resolve_track_id_extracts_min_csp_build_and_layout() {
        let r = resolve_track_id("csp/2651/../la_canyons-freeroam", &["la_canyons"]);
        assert_eq!(r.track_id, "la_canyons");
        assert_eq!(r.layout.as_deref(), Some("freeroam"));
        assert_eq!(r.min_csp_build, Some(2651));

        let dashed = resolve_track_id("my-track", &["my-track"]);
        assert_eq!(dashed.track_id, "my-track");
        assert_eq!(dashed.layout, None);
        assert_eq!(dashed.min_csp_build, None);
    }

    #[test]
    fn mock_http_booking_sub_handles_ok_closed_full_and_wrong_password() {
        let req = BookingRequest {
            driver_name: "Ada Lovelace".into(),
            steam_guid: "76561198000000001".into(),
            car_model: "ks_ferrari_488_gt3".into(),
            car_skin: "red".into(),
            password: "secret".into(),
        };

        let p_ok = serve_once(r#"{"status":"OK","time":180}"#);
        let reply = book("127.0.0.1", p_ok, &req, Duration::from_secs(3)).unwrap();
        assert!(reply.accepted);
        assert_eq!(reply.booking_time_sec, Some(180));

        let p_closed = serve_once("BOOKING_CLOSED");
        assert!(matches!(
            book("127.0.0.1", p_closed, &req, Duration::from_secs(3)),
            Err(BookingError::Closed)
        ));

        let p_full = serve_once(r#"{"status":"ERROR","error":"CAR_UNAVAILABLE"}"#);
        assert!(matches!(
            book("127.0.0.1", p_full, &req, Duration::from_secs(3)),
            Err(BookingError::CarUnavailable)
        ));

        let p_pw = serve_once("WRONG_PASSWORD");
        assert!(matches!(
            book("127.0.0.1", p_pw, &req, Duration::from_secs(3)),
            Err(BookingError::WrongPassword)
        ));
    }
}

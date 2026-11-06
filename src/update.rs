//! Opt-in update check against a JSON feed served over plain HTTP.
//!
//! Nothing in this module runs on its own. The caller decides when to ask for
//! a check, and `check` then makes one `GET` request to the feed URL it was given.
//!
//! `https://` feeds go through `ureq` with certificate checks (the Windows TLS
//! stack and certificate store on Windows, rustls elsewhere), so the feed cannot
//! be altered on the way. Redirects are followed
//! only while they stay on HTTPS. `http://` feeds use a small `std::net` client;
//! their reply is not protected, so the UI recommends HTTPS.
//!
//! Limits: compressed responses are refused, DNS lookup is not covered by the
//! timeout of the plain-HTTP client, and the feed and its notes are capped in size.
//!
//! Feed format:
//!
//! ```json
//! {"version": "1.2.0", "url": "https://example.com/download", "notes": "Fixes."}
//! ```

use std::cmp::Ordering;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde::Deserialize;

const MAX_VERSION_LEN: usize = 64;
const MAX_URL_LEN: usize = 2048;
const MAX_NOTES_LEN: usize = 16 * 1024;
const MAX_FEED_BYTES: usize = 64 * 1024;
/// Raw bytes accepted from the socket, headers and chunk framing included.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// A semantic version: `major.minor.patch` with an optional pre-release tag.
///
/// Build metadata (`+...`) is accepted by [`Version::parse`] and dropped, since
/// it does not take part in precedence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Option<String>,
}

impl Version {
    /// Parse `1.2.3`, `v1.2.3` or `1.2.3-rc.1`, surrounding whitespace allowed.
    pub fn parse(text: &str) -> Result<Version, UpdateError> {
        let text = text.trim();
        let invalid = || UpdateError::InvalidVersion(text.to_string());
        if text.len() > MAX_VERSION_LEN {
            return Err(invalid());
        }
        let text = text.strip_prefix('v').unwrap_or(text);
        let without_build = match text.split_once('+') {
            Some((core, build)) => {
                if !build.split('.').all(is_identifier) {
                    return Err(invalid());
                }
                core
            }
            None => text,
        };
        let (core, pre) = match without_build.split_once('-') {
            Some((core, pre)) => {
                if !is_pre_release(pre) {
                    return Err(invalid());
                }
                (core, Some(pre.to_string()))
            }
            None => (without_build, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        let &[major, minor, patch] = parts.as_slice() else {
            return Err(invalid());
        };
        match (number(major), number(minor), number(patch)) {
            (Some(major), Some(minor), Some(patch)) => Ok(Version { major, minor, patch, pre }),
            _ => Err(invalid()),
        }
    }
}

/// A non-empty dot-separated identifier made of ASCII letters, digits and hyphens.
fn is_identifier(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn is_pre_release(text: &str) -> bool {
    text.split('.').all(|id| {
        let numeric = id.bytes().all(|b| b.is_ascii_digit());
        is_identifier(id) && !(numeric && id.len() > 1 && id.starts_with('0'))
    })
}

/// A version component: digits only, no leading zeros, fits in `u64`.
fn number(part: &str) -> Option<u64> {
    let digits = !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits || (part.len() > 1 && part.starts_with('0')) {
        return None;
    }
    part.parse().ok()
}

/// Precedence between two pre-release tags, per semantic versioning.
fn cmp_pre(a: &str, b: &str) -> Ordering {
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => match cmp_identifier(x, y) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

fn cmp_identifier(a: &str, b: &str) -> Ordering {
    let a_numeric = a.bytes().all(|c| c.is_ascii_digit());
    let b_numeric = b.bytes().all(|c| c.is_ascii_digit());
    match (a_numeric, b_numeric) {
        // Numeric identifiers have no leading zeros (checked when parsing), so
        // comparing by length and then lexically is the same as comparing values.
        (true, true) => a.len().cmp(&b.len()).then_with(|| a.cmp(b)),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                // A release is greater than any pre-release of the same core.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => cmp_pre(a, b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(pre) = &self.pre {
            write!(f, "-{pre}")?;
        }
        Ok(())
    }
}

/// A release announced by the feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub url: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct FeedDocument {
    version: String,
    url: String,
    notes: String,
}

/// Read a feed body of the form `{"version", "url", "notes"}`.
///
/// All three fields are required. Unknown fields are ignored so that the
/// feed can grow without breaking older clients.
pub fn parse_feed(json: &str) -> Result<Release, UpdateError> {
    if json.len() > MAX_FEED_BYTES {
        return Err(UpdateError::InvalidFeed(format!("feed is larger than {MAX_FEED_BYTES} bytes")));
    }
    let doc: FeedDocument =
        serde_json::from_str(json).map_err(|err| UpdateError::InvalidFeed(err.to_string()))?;
    let version = Version::parse(&doc.version)?;
    if !is_release_url(&doc.url) {
        return Err(UpdateError::InvalidFeed(
            "url must be an http or https link without spaces".to_string(),
        ));
    }
    if doc.notes.len() > MAX_NOTES_LEN {
        return Err(UpdateError::InvalidFeed(format!(
            "notes are longer than {MAX_NOTES_LEN} bytes"
        )));
    }
    Ok(Release { version, url: doc.url, notes: doc.notes })
}

fn is_release_url(url: &str) -> bool {
    url.len() <= MAX_URL_LEN
        && is_printable_ascii(url)
        && (has_scheme(url, "http://") || has_scheme(url, "https://"))
}

/// Case-insensitive check for a URL scheme prefix such as `http://`.
fn has_scheme(url: &str, scheme: &str) -> bool {
    url.as_bytes()
        .get(..scheme.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(scheme.as_bytes()))
}

fn is_printable_ascii(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_graphic())
}

/// Where the feed lives, as far as the std-only client needs to know.
#[derive(Debug, PartialEq)]
struct Endpoint {
    /// Host name or IP address, without IPv6 brackets.
    host: String,
    port: u16,
    path: String,
}

impl Endpoint {
    fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == 80 { host } else { format!("{host}:{}", self.port) }
    }
}

fn parse_http_url(url: &str) -> Result<Endpoint, UpdateError> {
    let invalid = |why: &str| UpdateError::InvalidUrl(why.to_string());
    if url.len() > MAX_URL_LEN || !is_printable_ascii(url) {
        return Err(invalid("too long, or contains spaces or non-ASCII characters"));
    }
    if has_scheme(url, "https://") {
        return Err(invalid("https feeds are not supported, use http://"));
    }
    if !has_scheme(url, "http://") {
        return Err(invalid("feed URL must start with http://"));
    }
    // Only ASCII is left, so byte 7 is a character boundary.
    let rest = &url[7..];
    let rest = rest.split_once('#').map_or(rest, |(before, _)| before);
    let split_at = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(split_at);
    let path = if tail.is_empty() {
        "/".to_string()
    } else if tail.starts_with('/') {
        tail.to_string()
    } else {
        format!("/{tail}")
    };

    if authority.is_empty() || authority.contains('@') {
        return Err(invalid("feed URL needs a host and no user information"));
    }
    let (host, port_text) = if let Some(inner) = authority.strip_prefix('[') {
        let (addr, after) = inner
            .split_once(']')
            .ok_or_else(|| invalid("unclosed IPv6 literal"))?;
        match after {
            "" => (addr, None),
            _ => {
                let port = after.strip_prefix(':').ok_or_else(|| invalid("bad port"))?;
                (addr, Some(port))
            }
        }
    } else {
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if host.contains(':') {
            return Err(invalid("IPv6 hosts must be written in brackets"));
        }
        (host, port)
    };
    if host.is_empty() {
        return Err(invalid("feed URL needs a host"));
    }
    let port = match port_text {
        None => 80,
        Some(text) => text
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or_else(|| invalid("bad port"))?,
    };
    Ok(Endpoint { host: host.to_string(), port, path })
}

/// Ask `feed_url` for the latest release and return it only if it is newer than `current`.
///
/// `timeout` bounds the whole exchange (connect, send and receive), not each
/// step. Only `http://` URLs are accepted; the check makes no request for any
/// other scheme.
pub fn check(
    feed_url: &str,
    timeout: Duration,
    current: &Version,
) -> Result<Option<Release>, UpdateError> {
    let body = if has_scheme(feed_url, "https://") {
        fetch_https(feed_url, timeout)?
    } else {
        let endpoint = parse_http_url(feed_url)?;
        let deadline = Instant::now().checked_add(timeout).ok_or(UpdateError::Timeout)?;
        fetch(&endpoint, deadline)?
    };
    let text = String::from_utf8(body)
        .map_err(|_| UpdateError::InvalidFeed("feed is not valid UTF-8".to_string()))?;
    let release = parse_feed(&text)?;
    Ok(if release.version > *current { Some(release) } else { None })
}

/// One `GET` over HTTPS with certificate checks. Returns the body of a 200 reply.
fn fetch_https(url: &str, timeout: Duration) -> Result<Vec<u8>, UpdateError> {
    if url.len() > MAX_URL_LEN || !is_printable_ascii(url) {
        return Err(UpdateError::InvalidUrl("too long, or contains spaces or non-ASCII characters".into()));
    }
    let rest = &url["https://".len()..];
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || authority.contains('@') {
        return Err(UpdateError::InvalidUrl("feed URL needs a host and no user information".into()));
    }
    let builder = ureq::AgentBuilder::new();
    // Windows builds use the system TLS stack (SChannel) and its certificate store.
    #[cfg(windows)]
    let builder = builder.tls_connector(std::sync::Arc::new(
        native_tls::TlsConnector::new().map_err(|e| UpdateError::Network(io::Error::other(e.to_string())))?,
    ));
    let agent = builder
        .timeout(timeout)
        .redirects(5)
        .user_agent(concat!("acxs-manager/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = match agent.get(url).set("Accept", "application/json").set("Accept-Encoding", "identity").call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => return Err(UpdateError::HttpStatus(code)),
        Err(ureq::Error::Transport(err)) => {
            let text = err.to_string();
            return Err(if text.to_ascii_lowercase().contains("timed out") {
                UpdateError::Timeout
            } else {
                UpdateError::Network(io::Error::other(text))
            });
        }
    };
    // A redirect must not leave HTTPS, or the protection would be lost.
    if !has_scheme(response.get_url(), "https://") {
        return Err(UpdateError::BadResponse("the feed redirected away from https".into()));
    }
    if response.status() != 200 {
        return Err(UpdateError::HttpStatus(response.status()));
    }
    if response.header("content-encoding").is_some_and(|v| !v.eq_ignore_ascii_case("identity")) {
        return Err(UpdateError::BadResponse("compressed responses are not supported".into()));
    }
    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_FEED_BYTES as u64 + 1)
        .read_to_end(&mut body)?;
    if body.len() > MAX_FEED_BYTES {
        return Err(UpdateError::BadResponse("feed is larger than the limit".into()));
    }
    Ok(body)
}

/// Time left before `deadline`, or `Timeout` once it has passed.
fn remaining(deadline: Instant) -> Result<Duration, UpdateError> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or(UpdateError::Timeout)
}

/// Perform one `GET` and return the decoded 200 response body.
fn fetch(endpoint: &Endpoint, deadline: Instant) -> Result<Vec<u8>, UpdateError> {
    let mut last_error = None;
    let mut connected = None;
    for addr in (endpoint.host.as_str(), endpoint.port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, remaining(deadline)?) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(err) => last_error = Some(err),
        }
    }
    let mut stream = match connected {
        Some(stream) => stream,
        None => {
            let err = last_error
                .unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "host has no addresses"));
            return Err(err.into());
        }
    };

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: acxs-manager/{}\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
        endpoint.path,
        endpoint.host_header(),
        env!("CARGO_PKG_VERSION"),
    );
    stream.set_write_timeout(Some(remaining(deadline)?))?;
    stream.write_all(request.as_bytes())?;

    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&chunk[..n]);
                if raw.len() > MAX_RESPONSE_BYTES {
                    return Err(UpdateError::BadResponse("response is larger than the limit".into()));
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err.into()),
        }
    }
    decode_response(&raw)
}

/// Check the status line and headers, then return the body with transfer
/// framing removed. Only HTTP/1.x with `identity` or `chunked` encoding is handled.
fn decode_response(raw: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let malformed = |why: &str| UpdateError::BadResponse(why.to_string());
    let head_end = find(raw, b"\r\n\r\n").ok_or_else(|| malformed("no end of headers"))?;
    let head = std::str::from_utf8(&raw[..head_end]).map_err(|_| malformed("headers are not text"))?;
    let body = &raw[head_end + 4..];

    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut fields = status_line.split(' ');
    if !fields.next().unwrap_or_default().starts_with("HTTP/1.") {
        return Err(malformed("not an HTTP/1 response"));
    }
    let status: u16 = fields
        .next()
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| malformed("bad status line"))?;
    if status != 200 {
        return Err(UpdateError::HttpStatus(status));
    }

    let mut chunked = false;
    let mut length: Option<usize> = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(|| malformed("bad header line"))?;
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("transfer-encoding") {
            if !value.eq_ignore_ascii_case("chunked") {
                return Err(malformed("unsupported transfer coding"));
            }
            chunked = true;
        } else if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse().map_err(|_| malformed("bad content-length"))?);
        } else if name.eq_ignore_ascii_case("content-encoding")
            && !value.eq_ignore_ascii_case("identity")
        {
            return Err(malformed("compressed responses are not supported"));
        }
    }

    let decoded = if chunked {
        dechunk(body)?
    } else {
        let len = length.unwrap_or(body.len());
        if body.len() < len {
            return Err(malformed("body is shorter than content-length"));
        }
        body[..len].to_vec()
    };
    if decoded.len() > MAX_FEED_BYTES {
        return Err(malformed("feed is larger than the limit"));
    }
    Ok(decoded)
}

/// Remove chunked transfer framing. Trailer fields after the last chunk are ignored.
fn dechunk(body: &[u8]) -> Result<Vec<u8>, UpdateError> {
    let malformed = |why: &str| UpdateError::BadResponse(why.to_string());
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_len = find(&body[pos..], b"\r\n").ok_or_else(|| malformed("truncated chunk size"))?;
        let size_text = std::str::from_utf8(&body[pos..pos + line_len])
            .map_err(|_| malformed("bad chunk size"))?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| malformed("bad chunk size"))?;
        pos += line_len + 2;
        if size == 0 {
            return Ok(out);
        }
        if size > MAX_FEED_BYTES - out.len() {
            return Err(malformed("feed is larger than the limit"));
        }
        let end = pos + size;
        if body.len() < end + 2 {
            return Err(malformed("truncated chunk"));
        }
        if &body[end..end + 2] != b"\r\n" {
            return Err(malformed("chunk is not followed by CRLF"));
        }
        out.extend_from_slice(&body[pos..end]);
        pos = end + 2;
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Errors from the update check.
#[derive(Debug)]
pub enum UpdateError {
    /// Text that is not `major.minor.patch` with optional `-pre` and `+build`.
    InvalidVersion(String),
    /// The feed is not JSON of the expected shape, or breaks a limit.
    InvalidFeed(String),
    /// The feed URL cannot be fetched by the std-only client.
    InvalidUrl(String),
    /// The server answered with a status other than 200.
    HttpStatus(u16),
    /// The HTTP response could not be understood or exceeded a limit.
    BadResponse(String),
    /// The timeout passed before the exchange finished.
    Timeout,
    /// Resolving, connecting or transferring failed.
    Network(io::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVersion(text) => write!(f, "invalid version `{text}`"),
            Self::InvalidFeed(why) => write!(f, "invalid update feed: {why}"),
            Self::InvalidUrl(why) => write!(f, "invalid feed URL: {why}"),
            Self::HttpStatus(code) => write!(f, "feed server answered with HTTP {code}"),
            Self::BadResponse(why) => write!(f, "bad HTTP response: {why}"),
            Self::Timeout => write!(f, "update check timed out"),
            Self::Network(err) => write!(f, "network error: {err}"),
        }
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for UpdateError {
    fn from(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Self::Timeout,
            _ => Self::Network(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn parses_plain_and_prefixed_versions() {
        assert_eq!(v("1.2.3"), Version { major: 1, minor: 2, patch: 3, pre: None });
        assert_eq!(v("v10.0.12"), Version { major: 10, minor: 0, patch: 12, pre: None });
        assert_eq!(v("  v0.1.0 ").to_string(), "0.1.0");
        assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
    }

    #[test]
    fn parses_pre_release_tags() {
        let version = v("v2.0.0-rc.1");
        assert_eq!(version.pre.as_deref(), Some("rc.1"));
        assert_eq!(version.to_string(), "2.0.0-rc.1");
    }

    #[test]
    fn rejects_malformed_versions() {
        let bad = [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-rc..1",
            "1.2.3-01",
            "1.2.3-rc_1",
            "1.2.3+",
            "vv1.2.3",
            "-1.2.3",
            "99999999999999999999999.0.0",
        ];
        for text in bad {
            assert!(Version::parse(text).is_err(), "{text:?} should be rejected");
        }
    }

    #[test]
    fn orders_by_core_version() {
        assert!(v("1.0.0") < v("1.0.1"));
        assert!(v("1.0.9") < v("1.1.0"));
        assert!(v("1.9.9") < v("2.0.0"));
        assert!(v("10.0.0") > v("9.99.99"));
        assert_eq!(v("1.0.0").cmp(&v("v1.0.0")), Ordering::Equal);
    }

    #[test]
    fn pre_release_sorts_below_its_release() {
        assert!(v("1.0.0-rc.1") < v("1.0.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0"));
        assert!(v("1.0.1-alpha") > v("1.0.0"));
        assert!(v("1.0.0-alpha") > v("0.9.9"));
    }

    #[test]
    fn pre_release_identifiers_follow_semver_precedence() {
        let ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} should be below {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn numeric_pre_release_identifiers_compare_as_numbers_and_rank_lower() {
        assert!(v("1.0.0-1") < v("1.0.0-alpha"));
        assert!(v("1.0.0-9") < v("1.0.0-10"));
        assert!(v("1.0.0-99999999999999999999999") < v("1.0.0-a"));
        assert!(v("1.0.0-a") > v("1.0.0-9"));
    }

    #[test]
    fn parses_complete_feed() {
        let json = r#"{"version":"1.4.0","url":"https://example.com/get","notes":"Fixes."}"#;
        let release = parse_feed(json).unwrap();
        assert_eq!(release.version, v("1.4.0"));
        assert_eq!(release.url, "https://example.com/get");
        assert_eq!(release.notes, "Fixes.");
    }

    #[test]
    fn feed_ignores_unknown_fields() {
        let json = r#"{"version":"1.0.0","url":"https://e.org/","notes":"","extra":[1,2]}"#;
        assert!(parse_feed(json).is_ok());
    }

    #[test]
    fn feed_with_missing_fields_is_rejected() {
        let cases = [
            r#"{"url":"https://example.com/get","notes":""}"#,
            r#"{"version":"1.4.0","notes":""}"#,
            r#"{"version":"1.4.0","url":"https://example.com/get"}"#,
            "{}",
        ];
        for json in cases {
            assert!(matches!(parse_feed(json), Err(UpdateError::InvalidFeed(_))), "{json}");
        }
    }

    #[test]
    fn feed_rejects_bad_values() {
        assert!(parse_feed("not json").is_err());
        assert!(parse_feed(r#"{"version":3,"url":"https://e.org/","notes":""}"#).is_err());
        assert!(parse_feed(r#"{"version":"soon","url":"https://e.org/","notes":""}"#).is_err());
        assert!(parse_feed(r#"{"version":"1.0.0","url":"javascript:alert(1)","notes":""}"#).is_err());
        assert!(parse_feed(r#"{"version":"1.0.0","url":"https://e.org/ x","notes":""}"#).is_err());
        let notes = "n".repeat(MAX_NOTES_LEN + 1);
        let json = format!(r#"{{"version":"1.0.0","url":"https://e.org/","notes":"{notes}"}}"#);
        assert!(parse_feed(&json).is_err());
    }

    #[test]
    fn non_http_feed_urls_are_rejected_without_connecting() {
        let current = v("1.0.0");
        for url in [
            "https://user@example.com/feed.json",
            "https://",
            "ftp://example.com/feed.json",
            "example.com/feed.json",
            "http://",
            "http://user@127.0.0.1/feed.json",
        ] {
            let result = check(url, Duration::from_secs(1), &current);
            assert!(matches!(result, Err(UpdateError::InvalidUrl(_))), "{url}");
        }
    }

    /// Serve one canned response on a loopback port. Returns the feed URL and
    /// a handle that yields the raw request the client sent.
    fn serve_once(response: Vec<u8>) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 512];
            while find(&request, b"\r\n\r\n").is_none() {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..n]);
            }
            stream.write_all(&response).unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{addr}/feed.json"), handle)
    }

    fn http_response(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn http_chunked(body: &str, piece: usize) -> Vec<u8> {
        let mut out = String::from(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        );
        for part in body.as_bytes().chunks(piece) {
            out.push_str(&format!("{:x}\r\n", part.len()));
            out.push_str(std::str::from_utf8(part).unwrap());
            out.push_str("\r\n");
        }
        out.push_str("0\r\n\r\n");
        out.into_bytes()
    }

    const FEED_1_5: &str =
        r#"{"version":"1.5.0","url":"https://example.com/get","notes":"New stuff"}"#;

    #[test]
    fn check_returns_release_when_feed_is_newer() {
        let (url, server) = serve_once(http_response("200 OK", FEED_1_5));
        let release = check(&url, Duration::from_secs(5), &v("1.4.2"))
            .unwrap()
            .expect("feed 1.5.0 is newer than 1.4.2");
        assert_eq!(release.version, v("1.5.0"));
        assert_eq!(release.notes, "New stuff");
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /feed.json HTTP/1.1\r\n"), "{request}");
    }

    #[test]
    fn check_offers_release_over_a_pre_release_of_same_core() {
        let (url, server) = serve_once(http_response("200 OK", FEED_1_5));
        let found = check(&url, Duration::from_secs(5), &v("1.5.0-rc.1")).unwrap();
        assert!(found.is_some());
        server.join().unwrap();
    }

    #[test]
    fn check_returns_none_for_same_version() {
        let (url, server) = serve_once(http_response("200 OK", FEED_1_5));
        let found = check(&url, Duration::from_secs(5), &v("1.5.0")).unwrap();
        assert_eq!(found, None);
        server.join().unwrap();
    }

    #[test]
    fn check_returns_none_when_feed_is_older() {
        let older = r#"{"version":"1.0.0","url":"https://example.com/get","notes":""}"#;
        let (url, server) = serve_once(http_response("200 OK", older));
        let found = check(&url, Duration::from_secs(5), &v("1.4.2")).unwrap();
        assert_eq!(found, None);
        server.join().unwrap();
    }

    #[test]
    fn check_reads_chunked_bodies() {
        let (url, server) = serve_once(http_chunked(FEED_1_5, 7));
        let found = check(&url, Duration::from_secs(5), &v("1.4.2")).unwrap();
        assert_eq!(found.map(|r| r.version), Some(v("1.5.0")));
        server.join().unwrap();
    }

    #[test]
    fn check_reports_non_200_status() {
        let (url, server) = serve_once(http_response("404 Not Found", "missing"));
        let result = check(&url, Duration::from_secs(5), &v("1.0.0"));
        assert!(matches!(result, Err(UpdateError::HttpStatus(404))), "{result:?}");
        server.join().unwrap();
    }

    #[test]
    fn check_reports_truncated_body() {
        let mut response = http_response("200 OK", FEED_1_5);
        let cut = response.len() - 10;
        response.truncate(cut);
        let (url, server) = serve_once(response);
        let result = check(&url, Duration::from_secs(5), &v("1.0.0"));
        assert!(matches!(result, Err(UpdateError::BadResponse(_))), "{result:?}");
        server.join().unwrap();
    }

    #[test]
    fn check_times_out_on_a_silent_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(1500));
        });
        let url = format!("http://{addr}/feed.json");
        let result = check(&url, Duration::from_millis(200), &v("1.0.0"));
        assert!(matches!(result, Err(UpdateError::Timeout)), "{result:?}");
        server.join().unwrap();
    }

    #[test]
    fn parse_http_url_splits_host_port_and_path() {
        let plain = parse_http_url("http://feeds.example.com/acxs/latest.json?x=1#frag").unwrap();
        assert_eq!(plain.host, "feeds.example.com");
        assert_eq!(plain.port, 80);
        assert_eq!(plain.path, "/acxs/latest.json?x=1");

        let bare = parse_http_url("http://127.0.0.1:8080").unwrap();
        assert_eq!((bare.host.as_str(), bare.port, bare.path.as_str()), ("127.0.0.1", 8080, "/"));
        assert_eq!(bare.host_header(), "127.0.0.1:8080");

        let v6 = parse_http_url("http://[::1]:9000/f").unwrap();
        assert_eq!((v6.host.as_str(), v6.port), ("::1", 9000));
        assert_eq!(v6.host_header(), "[::1]:9000");
    }

    #[test]
    fn decode_response_rejects_compressed_and_error_responses() {
        let gzip = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\n\r\nxx";
        assert!(decode_response(gzip).is_err());
        let redirect = b"HTTP/1.1 301 Moved\r\nLocation: /x\r\n\r\n";
        assert!(matches!(decode_response(redirect), Err(UpdateError::HttpStatus(301))));
    }

    #[test]
    fn dechunk_joins_chunks_and_ignores_extensions() {
        let body = b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n";
        assert_eq!(dechunk(body).unwrap(), b"Wikipedia");
    }

    #[test]
    fn dechunk_rejects_truncated_and_malformed_framing() {
        assert!(dechunk(b"4\r\nWi").is_err());
        assert!(dechunk(b"zz\r\nabc\r\n0\r\n\r\n").is_err());
        assert!(dechunk(b"3\r\nabcXX0\r\n\r\n").is_err());
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Asks the real update server: `cargo test --lib live_update_feed -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn live_update_feed() {
        let old = Version::parse("0.0.1").unwrap();
        let release = check("https://test2.azendix.de/acxs/latest.json", Duration::from_secs(15), &old).unwrap();
        let release = release.expect("the feed names a version above 0.0.1");
        assert!(release.url.starts_with("https://"));
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(release.version <= current, "the feed is ahead of this build: {}", release.version);
    }
}

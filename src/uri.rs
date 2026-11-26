//! Parsing and formatting of `acmanager://` and `acxs://` server invite URIs.
//!
//! Supported forms:
//! - `acmanager://race/online/join?ip=<host>&httpPort=<port>[&password=<pw>&car=<id>&skin=<id>&csp=<build>]`
//! - `acxs://join?ip=<host>&httpPort=<port>[&password=<pw>&car=<id>&skin=<id>&csp=<build>]`

use std::fmt;

/// Parsed server join URI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct JoinUri {
    pub host: String,
    pub http_port: u16,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub car: Option<String>,
    #[serde(default)]
    pub skin: Option<String>,
    #[serde(default)]
    pub min_csp_build: Option<u32>,
}

/// Errors returned when parsing a server join URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriError {
    UnsupportedScheme,
    MissingQuery,
    MissingHost,
    InvalidHost(String),
    MissingPort,
    InvalidPort(String),
    InvalidEncoding,
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UriError::UnsupportedScheme => {
                write!(f, "URI must start with acmanager:// or acxs://")
            }
            UriError::MissingQuery => write!(f, "URI has no query string"),
            UriError::MissingHost => write!(f, "URI is missing the ip parameter"),
            UriError::InvalidHost(h) => write!(f, "URI host {h:?} is not valid"),
            UriError::MissingPort => write!(f, "URI is missing the httpPort parameter"),
            UriError::InvalidPort(p) => write!(f, "URI httpPort {p:?} is not a valid port"),
            UriError::InvalidEncoding => write!(f, "URI contains invalid percent-encoding"),
        }
    }
}

impl std::error::Error for UriError {}

/// Returns true if `host` contains only characters valid in a hostname, IPv4, or bracketed IPv6 literal.
pub fn is_valid_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    !bare.is_empty()
        && host.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':' | b'[' | b']')
        })
}

/// Percent-encode a query parameter value.
pub fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Percent-decode a query parameter value (`+` is treated as space).
pub fn percent_decode(input: &str) -> Result<String, UriError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(UriError::InvalidEncoding);
                }
                let hi = hex_val(bytes[i + 1]).ok_or(UriError::InvalidEncoding)?;
                let lo = hex_val(bytes[i + 2]).ok_or(UriError::InvalidEncoding)?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| UriError::InvalidEncoding)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse an `acmanager://` or `acxs://` server join URI.
pub fn parse_join_uri(raw: &str) -> Result<JoinUri, UriError> {
    let trimmed = raw.trim();
    if trimmed.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(UriError::InvalidHost(trimmed.to_string()));
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("acmanager://") || lower.starts_with("acxs://")) {
        return Err(UriError::UnsupportedScheme);
    }
    let (_, query) = trimmed.split_once('?').ok_or(UriError::MissingQuery)?;

    let mut host: Option<String> = None;
    let mut port_raw: Option<String> = None;
    let mut password: Option<String> = None;
    let mut car: Option<String> = None;
    let mut skin: Option<String> = None;
    let mut min_csp_build: Option<u32> = None;

    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = percent_decode(v)?;
        match key {
            "ip" | "host" => {
                if !val.is_empty() {
                    host = Some(val);
                }
            }
            "httpPort" | "cport" | "port" => {
                if !val.is_empty() {
                    port_raw = Some(val);
                }
            }
            "password" | "pass" => {
                if !val.is_empty() {
                    password = Some(val);
                }
            }
            "car" | "model" => {
                if !val.is_empty() {
                    car = Some(val);
                }
            }
            "skin" => {
                if !val.is_empty() {
                    skin = Some(val);
                }
            }
            "csp" | "minCsp" => {
                if let Ok(b) = val.parse::<u32>()
                    && b > 0
                {
                    min_csp_build = Some(b);
                }
            }
            _ => {}
        }
    }

    let host = host.ok_or(UriError::MissingHost)?;
    if !is_valid_host(&host) {
        return Err(UriError::InvalidHost(host));
    }
    let port_str = port_raw.ok_or(UriError::MissingPort)?;
    let http_port = port_str
        .parse::<u16>()
        .ok()
        .filter(|p| *p != 0)
        .ok_or(UriError::InvalidPort(port_str))?;

    Ok(JoinUri {
        host,
        http_port,
        password,
        car,
        skin,
        min_csp_build,
    })
}

/// Format a standard Content Manager compatible `acmanager://` invite link.
pub fn format_acmanager_uri(host: &str, http_port: u16, password: Option<&str>) -> String {
    let mut out = format!(
        "acmanager://race/online/join?ip={}&httpPort={http_port}",
        percent_encode(host)
    );
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        out.push_str("&password=");
        out.push_str(&percent_encode(pw));
    }
    out
}

/// Format an `acxs://` invite link with optional car, skin, password, and minimum CSP build.
pub fn format_acxs_uri(uri: &JoinUri) -> String {
    let mut out = format!(
        "acxs://join?ip={}&httpPort={}",
        percent_encode(&uri.host),
        uri.http_port
    );
    if let Some(pw) = uri.password.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("&password=");
        out.push_str(&percent_encode(pw));
    }
    if let Some(car) = uri.car.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("&car=");
        out.push_str(&percent_encode(car));
    }
    if let Some(skin) = uri.skin.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("&skin=");
        out.push_str(&percent_encode(skin));
    }
    if let Some(csp) = uri.min_csp_build {
        out.push_str(&format!("&csp={csp}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_acmanager_and_acxs_uris_with_encoded_password_and_car() {
        let cm = parse_join_uri(
            "acmanager://race/online/join?ip=94.103.92.144&httpPort=8809&password=my%20pass%261&car=ks_ferrari_488_gt3&skin=red_01&csp=2651",
        )
        .unwrap();
        assert_eq!(cm.host, "94.103.92.144");
        assert_eq!(cm.http_port, 8809);
        assert_eq!(cm.password.as_deref(), Some("my pass&1"));
        assert_eq!(cm.car.as_deref(), Some("ks_ferrari_488_gt3"));
        assert_eq!(cm.skin.as_deref(), Some("red_01"));
        assert_eq!(cm.min_csp_build, Some(2651));

        let acxs = parse_join_uri("ACXS://join?httpPort=8081&ip=play.example.com").unwrap();
        assert_eq!(acxs.host, "play.example.com");
        assert_eq!(acxs.http_port, 8081);
        assert_eq!(acxs.password, None);
    }

    #[test]
    fn rejects_invalid_hosts_zero_ports_and_control_chars() {
        assert_eq!(parse_join_uri("https://example.com?ip=1.2.3.4&httpPort=8081"), Err(UriError::UnsupportedScheme));
        assert_eq!(parse_join_uri("acmanager://race/online/join"), Err(UriError::MissingQuery));
        assert!(matches!(
            parse_join_uri("acmanager://race/online/join?ip=bad%20host&httpPort=8081"),
            Err(UriError::InvalidHost(_))
        ));
        assert!(matches!(
            parse_join_uri("acmanager://race/online/join?ip=1.2.3.4&httpPort=0"),
            Err(UriError::InvalidPort(_))
        ));
        assert!(matches!(
            parse_join_uri("acmanager://race/online/join?ip=1.2.3.4\r\n&httpPort=8081"),
            Err(UriError::InvalidHost(_))
        ));
        assert_eq!(
            parse_join_uri("acmanager://race/online/join?ip=1.2.3.4&httpPort=8081&password=%ZZ"),
            Err(UriError::InvalidEncoding)
        );
    }

    #[test]
    fn format_acmanager_uri_round_trips_through_parse() {
        let formatted = format_acmanager_uri("198.51.100.4", 8081, Some("s3cr3t! &"));
        let parsed = parse_join_uri(&formatted).unwrap();
        assert_eq!(parsed.host, "198.51.100.4");
        assert_eq!(parsed.http_port, 8081);
        assert_eq!(parsed.password.as_deref(), Some("s3cr3t! &"));

        let full = JoinUri {
            host: "race.local".into(),
            http_port: 8082,
            password: Some("pw".into()),
            car: Some("bmw_m3_e30".into()),
            skin: Some("white".into()),
            min_csp_build: Some(3334),
        };
        assert_eq!(parse_join_uri(&format_acxs_uri(&full)).unwrap(), full);
    }
}

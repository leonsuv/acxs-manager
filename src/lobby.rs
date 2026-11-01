//! The Kunos lobby: the public list of Assetto Corsa servers.
//!
//! Content Manager (open source, `gro-ove/actools`, `KunosApiProvider`) loads the list with
//! `GET http://<lobby host>/lobby.ashx/list?guid=<SteamID64>&r=<n>` and gets a JSON array.
//! Each entry carries the server's address, its HTTP port (`cport`, the one `/INFO` and the
//! entry list are served on), name, player counts, track, car ids and session.
//!
//! The lobby wants the player's Steam id, as the game itself sends it. [`steam_id64`] reads it
//! from Steam's own `config/loginusers.vdf`; nothing is stored or sent anywhere else.

use crate::steam::{parse_vdf, VdfValue};
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

/// The lobby host Content Manager falls back to. It can be changed in the UI.
pub const DEFAULT_HOST: &str = "93.57.10.21";

/// A reply larger than this is refused.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// One public server as the lobby lists it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LobbyServer {
    pub ip: String,
    /// The HTTP port: `/INFO` and the entry list are served here.
    pub http_port: u16,
    /// The port the race runs on, the one `acs.exe` connects to (`tport`).
    pub race_port: u16,
    pub name: String,
    pub clients: u32,
    pub capacity: u32,
    pub track: String,
    /// Car folder names the server allows.
    pub cars: Vec<String>,
    /// Session code (0 booking, 1 practice, 2 qualifying, 3 race).
    pub session: u32,
    pub countries: Vec<String>,
    pub password: bool,
    /// Track as a name for people (directory lists give names, not folder ids).
    pub track_name: String,
    /// The server needs a Custom Shaders Patch build.
    pub csp_required: bool,
    pub region: String,
    /// Car names for people, when the list gives names instead of folder ids.
    pub car_names: Vec<String>,
    /// Drivers on the server, when the list includes them.
    pub player_names: Vec<String>,
}

impl LobbyServer {
    /// `ip:httpPort`, the key the UI uses for a server.
    pub fn key(&self) -> String {
        format!("{}:{}", self.ip, self.http_port)
    }
}

/// Steam id64 = this base + the 32-bit account id.
const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

/// The 32-bit account id of the user signed in to the running Steam client
/// (`HKCU\Software\Valve\Steam\ActiveProcess\ActiveUser`, `0` when nobody is). Windows only.
#[cfg(windows)]
fn active_steam_user() -> Option<u64> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam\ActiveProcess", "/v", "ActiveUser"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_active_user(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(windows))]
fn active_steam_user() -> Option<u64> {
    None
}

/// The `ActiveUser` value in `reg query` output: `    ActiveUser    REG_DWORD    0x12ab34cd`.
pub fn parse_active_user(output: &str) -> Option<u64> {
    output.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("ActiveUser")?;
        let hex = rest.trim_start().strip_prefix("REG_DWORD")?.trim().strip_prefix("0x")?;
        u64::from_str_radix(hex, 16).ok().filter(|id| *id != 0)
    })
}

/// The Steam id of whoever is using Steam on this PC right now, read fresh on every call
/// like Content Manager does: the signed-in user of the running client, or, when Steam is
/// not running, the account that signed in last.
pub fn current_steam_id64(steam_roots: &[PathBuf]) -> Option<String> {
    active_steam_user()
        .map(|id| (STEAM_ID64_BASE + id).to_string())
        .or_else(|| steam_id64(steam_roots))
}

/// The Steam id of the account Steam last signed in with: the entry of
/// `loginusers.vdf` with the newest `Timestamp`.
pub fn steam_id64(steam_roots: &[PathBuf]) -> Option<String> {
    steam_roots.iter().find_map(|root| {
        let text = std::fs::read_to_string(root.join("config").join("loginusers.vdf")).ok()?;
        steam_id_from_vdf(&text)
    })
}

/// See [`steam_id64`]. Public for tests.
pub fn steam_id_from_vdf(text: &str) -> Option<String> {
    let doc = parse_vdf(text);
    let users = doc.get(&["users"])?;
    let VdfValue::Map(entries) = users else { return None };
    entries
        .iter()
        .filter(|(id, _)| id.len() == 17 && id.bytes().all(|b| b.is_ascii_digit()))
        .max_by_key(|(_, user)| {
            user.get(&["timestamp"]).and_then(VdfValue::as_str).and_then(|t| t.parse::<u64>().ok()).unwrap_or(0)
        })
        .map(|(id, _)| id.clone())
}

/// The list URL, with the same 30 second cache key Content Manager uses.
pub fn list_url(host: &str, steam_id: &str, unix_seconds: u64) -> String {
    format!("http://{host}/lobby.ashx/list?guid={steam_id}&r={}", unix_seconds / 30 % (24 * 60))
}

fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
}

/// Download and parse the lobby list.
pub fn fetch(host: &str, steam_id: &str, timeout: Duration) -> Result<Vec<LobbyServer>, String> {
    if !valid_host(host) {
        return Err("The lobby host is not valid.".into());
    }
    if steam_id.len() != 17 || !steam_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("No valid Steam id.".into());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Redirects are not followed: a redirect to a web page means the lobby refused the request,
    // and following it would only hide that behind an HTML page.
    let response = web_agent(timeout, false)?
        .get(&list_url(host, steam_id, now))
        .call()
        .map_err(|e| format!("The lobby did not answer: {e}"))?;
    let status = response.status();
    let content_type = response.header("Content-Type").unwrap_or("unknown").to_string();
    let location = response.header("Location").map(str::to_string);
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_string(&mut body)
        .map_err(|e| format!("Could not read the lobby reply: {e}"))?;
    if status != 200 {
        return Err(match location {
            Some(to) => format!("The lobby answered HTTP {status} and sent the request to {to}, so it did not give a list."),
            None => format!("The lobby answered HTTP {status} ({content_type}) instead of a list."),
        });
    }
    parse_list(&body).map_err(|e| format!("{e} (HTTP {status}, {content_type}, starts with: {})", head(&body)))
}

/// The start of a reply on one line, for error messages.
fn head(body: &str) -> String {
    let flat: String = body.chars().take(140).map(|c| if c.is_control() { ' ' } else { c }).collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() { "nothing".to_string() } else { flat }
}

/// Parse the lobby's JSON array. Entries without an address are skipped.
pub fn parse_list(json: &str) -> Result<Vec<LobbyServer>, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("The lobby reply is not JSON: {e}"))?;
    // Some lobbies wrap the array in {"Servers": [...]}.
    let array = match &value {
        Value::Array(items) => items,
        Value::Object(map) => match map.get("Servers").or_else(|| map.get("servers")) {
            Some(Value::Array(items)) => items,
            _ => return Err("The lobby reply has no server list.".into()),
        },
        _ => return Err("The lobby reply has no server list.".into()),
    };
    let servers: Vec<LobbyServer> = array.iter().filter_map(parse_entry).collect();
    // Content Manager's own sanity check: a list where every entry has one address is junk.
    if servers.len() > 3 && servers.iter().all(|s| s.ip == servers[0].ip) {
        return Err("The lobby returned nonsense.".into());
    }
    Ok(servers)
}

fn parse_entry(value: &Value) -> Option<LobbyServer> {
    let obj = value.as_object()?;
    let text = |key: &str| obj.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let number = |key: &str| obj.get(key).and_then(Value::as_u64).unwrap_or(0);
    let flag = |key: &str| match obj.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) != 0,
        _ => false,
    };
    let list = |key: &str| -> Vec<String> {
        match obj.get(key) {
            Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        }
    };
    let ip = text("ip");
    if ip.is_empty() {
        return None;
    }
    Some(LobbyServer {
        ip,
        http_port: u16::try_from(number("cport")).ok().filter(|p| *p != 0)?,
        race_port: u16::try_from(if number("tport") != 0 { number("tport") } else { number("port") }).unwrap_or(0),
        name: text("name"),
        clients: number("clients") as u32,
        capacity: number("maxclients") as u32,
        track: text("track"),
        cars: list("cars"),
        session: number("session") as u32,
        countries: list("country"),
        password: flag("pass"),
        ..LobbyServer::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_skips_broken_ones() {
        let json = r#"[
            {"ip":"198.51.100.4","cport":8081,"port":9600,"name":"Test A","clients":3,"maxclients":20,
             "track":"ks_barcelona","cars":["abarth500","ks_audi_r8_lms"],"session":3,"country":["Italy","IT"],"pass":false},
            {"ip":"","cport":1},
            {"ip":"198.51.100.5","cport":0},
            {"ip":"198.51.100.6","cport":8082,"name":"Locked","pass":true}
        ]"#;
        let list = parse_list(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].key(), "198.51.100.4:8081");
        assert_eq!((list[0].clients, list[0].capacity, list[0].session), (3, 20, 3));
        assert_eq!(list[0].cars, ["abarth500", "ks_audi_r8_lms"]);
        assert!(list[1].password);
    }

    #[test]
    fn rejects_a_list_with_one_address_everywhere() {
        let entry = r#"{"ip":"198.51.100.9","cport":8081}"#;
        let json = format!("[{entry},{entry},{entry},{entry}]");
        assert!(parse_list(&json).is_err());
    }

    #[test]
    fn accepts_a_wrapped_list() {
        let json = r#"{"Servers":[{"ip":"198.51.100.4","cport":8081}]}"#;
        assert_eq!(parse_list(json).unwrap().len(), 1);
    }

    #[test]
    fn picks_the_newest_signed_in_steam_user() {
        let vdf = r#""users"
{
    "76561198000000001" { "AccountName" "a" "timestamp" "100" }
    "76561198000000002" { "AccountName" "b" "timestamp" "300" }
    "76561198000000003" { "AccountName" "c" "timestamp" "200" }
}"#;
        assert_eq!(steam_id_from_vdf(vdf).as_deref(), Some("76561198000000002"));
    }

    #[test]
    fn builds_the_list_url_like_content_manager() {
        assert_eq!(
            list_url("203.0.113.1", "76561198000000002", 3000),
            "http://203.0.113.1/lobby.ashx/list?guid=76561198000000002&r=100"
        );
    }
}

#[cfg(test)]
mod active_user_tests {
    use super::*;

    #[test]
    fn reads_the_active_user_from_reg_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\\ActiveProcess\r\n    ActiveUser    REG_DWORD    0x12ab34cd\r\n";
        assert_eq!(parse_active_user(out), Some(0x12ab34cd));
        assert_eq!(parse_active_user("    ActiveUser    REG_DWORD    0x0"), None);
    }
}

/// Download a server list from any web address that serves the lobby's JSON format, plain or
/// gzip-compressed. Community lists and Content Manager's "lobby lists" work this way.
pub fn fetch_url(url: &str, timeout: Duration) -> Result<Vec<LobbyServer>, String> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) || url.chars().any(|c| c.is_whitespace()) {
        return Err("The list address must be a web address starting with http:// or https://.".into());
    }
    let response = web_agent(timeout, true)?
        .get(url.trim())
        .call()
        .map_err(|e| format!("The list did not answer: {e}"))?;
    let status = response.status();
    let content_type = response.header("Content-Type").unwrap_or("unknown").to_string();
    let mut raw = Vec::new();
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_end(&mut raw)
        .map_err(|e| format!("Could not read the list: {e}"))?;
    let text = decode_body(&raw)?;
    parse_list(&text).map_err(|e| format!("{e} (HTTP {status}, {content_type}, starts with: {})", head(&text)))
}

/// The reply as text, unpacking gzip when it starts with the gzip marker.
fn decode_body(raw: &[u8]) -> Result<String, String> {
    if raw.starts_with(&[0x1f, 0x8b]) {
        let mut text = String::new();
        flate2::read::GzDecoder::new(raw)
            .take(MAX_BYTES * 4)
            .read_to_string(&mut text)
            .map_err(|e| format!("The list is compressed but could not be unpacked: {e}"))?;
        Ok(text)
    } else {
        String::from_utf8(raw.to_vec()).map_err(|_| "The list is not text.".to_string())
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn plain_and_gzip_bodies_both_decode() {
        assert_eq!(decode_body(b"[1]").unwrap(), "[1]");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(br#"[{"ip":"198.51.100.4","cport":8081}]"#).unwrap();
        let packed = encoder.finish().unwrap();
        let text = decode_body(&packed).unwrap();
        assert_eq!(parse_list(&text).unwrap().len(), 1);
    }

    #[test]
    fn refuses_addresses_that_are_not_web_addresses() {
        assert!(fetch_url("file:///c:/list.json", Duration::from_secs(1)).is_err());
        assert!(fetch_url("http://a b", Duration::from_secs(1)).is_err());
    }
}

/// An HTTP client for list downloads. On Windows it uses the system TLS stack and certificate
/// store, so `https://` addresses work like they do in the update check.
pub(crate) fn web_agent(timeout: Duration, follow_redirects: bool) -> Result<ureq::Agent, String> {
    let builder = ureq::AgentBuilder::new();
    #[cfg(windows)]
    let builder = builder.tls_connector(std::sync::Arc::new(native_tls::TlsConnector::new().map_err(|e| e.to_string())?));
    Ok(builder
        .timeout(timeout)
        .redirects(if follow_redirects { 5 } else { 0 })
        .user_agent(concat!("ACXS-Manager/", env!("CARGO_PKG_VERSION")))
        .build())
}

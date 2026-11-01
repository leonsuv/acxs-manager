//! A public server directory: <https://acservers.online>, a community site that lists Assetto
//! Corsa servers with live player counts.
//!
//! The Kunos lobby only answers the game's own client, so this is the open source of public
//! servers. The site serves JSON at `/api/v1/servers`: ten servers per page, `hasPlayers=true`
//! to leave out empty ones, and `q=` to search every listed server by name. Each entry carries a
//! Content Manager join link from which the address and HTTP port are read.
//!
//! Requests are polite: one at a time, a short pause between pages, a page cap, and a plain
//! `ACXS-Manager` user agent.

use crate::lobby::LobbyServer;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

const ENDPOINT: &str = "https://acservers.online/api/v1/servers";
/// The site's own address, for credit in the interface.
pub const SITE: &str = "acservers.online";
const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Pause between pages.
const PAUSE: Duration = Duration::from_millis(120);

/// One page of results.
#[derive(Debug, Default)]
pub struct Page {
    pub servers: Vec<LobbyServer>,
    pub total: u64,
    pub per_page: u64,
}

/// Percent-encode `text` for a query value.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The request address for one page.
pub fn page_url(query: Option<&str>, has_players: bool, page: u32) -> String {
    let mut url = format!("{ENDPOINT}?page={page}");
    if has_players {
        url.push_str("&hasPlayers=true");
    }
    if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
        url.push_str("&q=");
        url.push_str(&encode(q));
    }
    url
}

/// Read one page.
pub fn fetch_page(query: Option<&str>, has_players: bool, page: u32, timeout: Duration) -> Result<Page, String> {
    let response = crate::lobby::web_agent(timeout, true)?
        .get(&page_url(query, has_players, page))
        .call()
        .map_err(|e| format!("The server directory did not answer: {e}"))?;
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_BYTES)
        .read_to_string(&mut body)
        .map_err(|e| format!("Could not read the directory reply: {e}"))?;
    parse_page(&body)
}

/// Read up to `max_pages` pages (or all there are) and join them. `progress` is told how many
/// servers have been read so far.
pub fn fetch_all(
    query: Option<&str>,
    has_players: bool,
    max_pages: u32,
    timeout: Duration,
    mut progress: impl FnMut(usize),
) -> Result<Vec<LobbyServer>, String> {
    let mut all: Vec<LobbyServer> = Vec::new();
    let mut page = 1;
    loop {
        let result = fetch_page(query, has_players, page, timeout)?;
        let per_page = result.per_page.max(1);
        let pages = result.total.div_ceil(per_page).min(u64::from(max_pages)) as u32;
        all.extend(result.servers);
        progress(all.len());
        if page >= pages || result.total == 0 {
            break;
        }
        page += 1;
        std::thread::sleep(PAUSE);
    }
    Ok(all)
}

/// Parse a reply: `{"items":[...],"total":N,"page":1,"perPage":10,...}`.
pub fn parse_page(json: &str) -> Result<Page, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("The directory reply is not JSON: {e}"))?;
    let items = value.get("items").and_then(Value::as_array).ok_or("The directory reply has no server list.")?;
    Ok(Page {
        servers: items.iter().filter_map(parse_item).collect(),
        total: value.get("total").and_then(Value::as_u64).unwrap_or(0),
        per_page: value.get("perPage").and_then(Value::as_u64).unwrap_or(10),
    })
}

/// Address and HTTP port from `acmanager://race/online/join?ip=1.2.3.4&httpPort=8081`.
pub fn parse_join_url(url: &str) -> Option<(String, u16)> {
    let query = url.split_once('?')?.1;
    let (mut ip, mut port) = (None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("ip", value)) if !value.is_empty() => ip = Some(value.to_string()),
            Some(("httpPort", value)) => port = value.parse::<u16>().ok().filter(|p| *p != 0),
            _ => {}
        }
    }
    Some((ip?, port?))
}

fn parse_item(item: &Value) -> Option<LobbyServer> {
    let (ip, http_port) = parse_join_url(item.get("joinUrl")?.as_str()?)?;
    // Only addresses that make sense for a host name or IP.
    if !ip.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':')) {
        return None;
    }
    let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let number = |key: &str| item.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
    let list = |key: &str| -> Vec<String> {
        item.get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    Some(LobbyServer {
        ip,
        http_port,
        name: text("name"),
        clients: number("players"),
        capacity: number("capacity"),
        track_name: text("track"),
        countries: vec![text("countryCode")].into_iter().filter(|c| !c.is_empty()).collect(),
        password: item.get("passwordProtected").and_then(Value::as_bool).unwrap_or(false),
        csp_required: item.get("cspRequired").and_then(Value::as_bool).unwrap_or(false),
        region: text("region"),
        car_names: list("carNames"),
        player_names: list("playerNames"),
        ..LobbyServer::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"items":[
        {"name":"Test Server","track":"Knockhill","players":25,"capacity":34,"countryCode":"GB",
         "region":"Europe","passwordProtected":false,"cspRequired":true,"carNames":["Mini"],
         "playerNames":["A","B"],"joinUrl":"acmanager://race/online/join?ip=94.103.92.144&httpPort=8809"},
        {"name":"No link"},
        {"name":"Bad","joinUrl":"acmanager://race/online/join?ip=a b&httpPort=1"}
    ],"total":421,"page":1,"perPage":10}"#;

    #[test]
    fn reads_servers_and_skips_unusable_ones() {
        let page = parse_page(SAMPLE).unwrap();
        assert_eq!(page.total, 421);
        assert_eq!(page.per_page, 10);
        assert_eq!(page.servers.len(), 1);
        let s = &page.servers[0];
        assert_eq!((s.ip.as_str(), s.http_port), ("94.103.92.144", 8809));
        assert_eq!((s.clients, s.capacity), (25, 34));
        assert!(s.csp_required);
        assert_eq!(s.player_names, ["A", "B"]);
    }

    #[test]
    fn join_links_are_read_in_any_order() {
        assert_eq!(parse_join_url("acmanager://x?httpPort=8081&ip=1.2.3.4"), Some(("1.2.3.4".into(), 8081)));
        assert_eq!(parse_join_url("acmanager://x?ip=1.2.3.4"), None);
        assert_eq!(parse_join_url("nothing"), None);
    }

    #[test]
    fn search_text_is_encoded() {
        assert_eq!(page_url(Some("la canyons"), true, 2), "https://acservers.online/api/v1/servers?page=2&hasPlayers=true&q=la%20canyons");
        assert_eq!(page_url(None, false, 1), "https://acservers.online/api/v1/servers?page=1");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Talks to the real site; run with `cargo test --lib live_directory -- --ignored`.
    #[test]
    #[ignore]
    fn live_directory_answers_and_parses() {
        let page = fetch_page(None, true, 1, Duration::from_secs(20)).unwrap();
        assert!(page.total > 0 && !page.servers.is_empty());
        let found = fetch_all(Some("cruisin"), false, 2, Duration::from_secs(20), |_| {}).unwrap();
        assert!(!found.is_empty());
        println!("first: {} {}:{} {}/{}", found[0].name, found[0].ip, found[0].http_port, found[0].clients, found[0].capacity);
    }
}

/// Read the pages one after another and hand each to `on_page(servers, total)` as it arrives, so a
/// list can fill in while the rest is still loading. Stops after `max_pages` or the last page.
pub fn fetch_stream(
    query: Option<&str>,
    has_players: bool,
    max_pages: u32,
    timeout: Duration,
    mut on_page: impl FnMut(Vec<LobbyServer>, u64),
) -> Result<(), String> {
    let mut page = 1;
    loop {
        let result = fetch_page(query, has_players, page, timeout)?;
        let per_page = result.per_page.max(1);
        let pages = result.total.div_ceil(per_page).min(u64::from(max_pages)) as u32;
        let total = result.total;
        on_page(result.servers, total);
        if page >= pages || total == 0 {
            return Ok(());
        }
        page += 1;
        std::thread::sleep(PAUSE);
    }
}

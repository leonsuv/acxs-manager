//! Compares what ACXS would send for a join with a race.ini another launcher wrote:
//! `cargo run --example join_check`. Reads `race.ini`, asks the server named in it for its
//! status, and prints only whether each value matches, so no Steam id reaches the output.

use acxs_manager::{lobby, server_status, settings, steam};
use std::time::Duration;

fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('='))
}

fn main() {
    let cfg = settings::ac_user_cfg_dir().expect("documents");
    let text = std::fs::read_to_string(cfg.join("race.ini")).expect("race.ini");
    let host = value(&text, "SERVER_IP").expect("no [REMOTE] server").to_string();
    let http: u16 = value(&text, "SERVER_HTTP_PORT").unwrap().parse().unwrap();
    let status = server_status::fetch(&host, http, Duration::from_secs(6)).expect("server status");
    let steam_id = lobby::current_steam_id64(&steam::default_steam_roots()).unwrap_or_default();
    let reference_port: u16 = value(&text, "SERVER_PORT").unwrap().parse().unwrap();
    let reference_features: Vec<&str> = value(&text, "__FEATURES").unwrap_or("").split(',').collect();
    let features: Vec<&str> = status.features.iter().map(String::as_str).collect();
    println!("race port matches: {}", status.port == Some(reference_port));
    println!("features match: {} ({} vs {})", features == reference_features, features.len(), reference_features.len());
    println!("  only in reference: {:?}", reference_features.iter().filter(|f| !features.contains(f)).collect::<Vec<_>>());
    println!("  only from server:  {:?}", features.iter().filter(|f| !reference_features.contains(f)).collect::<Vec<_>>());
    println!("steam id matches: {}", steam_id == value(&text, "GUID").unwrap_or(""));
    println!("track reported: {:?}", status.track);
}

//! Assetto Corsa dedicated server configuration: `cfg/server_cfg.ini` and `cfg/entry_list.ini`.
//!
//! Only the keys the dedicated server reads are handled, with the defaults a fresh install uses.
//!
//! Files are read and written through [`crate::ini::Ini`]. `write_server_config` only changes
//! the keys this module owns, so comments, unknown keys and other sections survive a save.
//! `write_entry_list` replaces the whole entry list, because every `[CAR_n]` section is owned here.
//! Both writes keep the previous file as `<name>.acxs-backup-<stamp>`.

use std::io;
use std::path::Path;

use crate::ini::Ini;

/// File name of the server settings inside the cfg directory.
pub const SERVER_CFG: &str = "server_cfg.ini";
/// File name of the entry list inside the cfg directory.
pub const ENTRY_LIST: &str = "entry_list.ini";
const SERVER: &str = "SERVER";
const DYNAMIC_TRACK: &str = "DYNAMIC_TRACK";

/// Largest entry list the server reads: sections `CAR_0` to `CAR_254`.
const MAX_ENTRIES: usize = 255;

/// The `[SERVER]` keys this module owns, plus the optional `[DYNAMIC_TRACK]` section.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    /// `NAME`.
    pub name: String,
    /// `PASSWORD`. Empty means no password.
    pub password: String,
    /// `ADMIN_PASSWORD`. Empty means anyone can join as admin.
    pub admin_password: String,
    /// `CARS`, a semicolon-separated list of car model ids.
    pub cars: Vec<String>,
    /// `TRACK`.
    pub track: String,
    /// `CONFIG_TRACK`, the track layout. Empty for tracks without layouts.
    pub config_track: String,
    /// `MAX_CLIENTS`.
    pub max_clients: u32,
    /// `UDP_PORT`.
    pub udp_port: u16,
    /// `TCP_PORT`.
    pub tcp_port: u16,
    /// `HTTP_PORT`, the lobby port.
    pub http_port: u16,
    /// `LOOP_MODE`. When true the server restarts from the first session after the last one.
    pub loop_mode: bool,
    /// `SUN_ANGLE`.
    pub sun_angle: i32,
    /// `PICKUP_MODE_ENABLED`. Pickup mode needs one track and at least one entry-list car.
    pub pickup_mode: bool,
    /// `[DYNAMIC_TRACK]`. `None` means the section is absent, and writing leaves it out.
    pub dynamic_track: Option<DynamicTrack>,
}

/// The `[DYNAMIC_TRACK]` section: grip evolution over a session.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicTrack {
    /// `SESSION_START`, percent grip at the start of a session.
    pub session_start: u32,
    /// `RANDOMNESS`, random variation added to the starting grip.
    pub randomness: u32,
    /// `SESSION_TRANSFER`, percent of gained grip carried into the next session.
    pub session_transfer: u32,
    /// `LAP_GAIN`, laps needed to add 1% grip.
    pub lap_gain: u32,
}

impl Default for ServerConfig {
    /// The defaults a fresh install uses. `dynamic_track` is `None`, so a
    /// default config does not add a `[DYNAMIC_TRACK]` section to an existing file.
    fn default() -> Self {
        ServerConfig {
            name: "Assetto Corsa Server".to_string(),
            password: String::new(),
            admin_password: String::new(),
            cars: vec!["lotus_evora_gtc".to_string()],
            track: "ks_silverstone".to_string(),
            config_track: "gp".to_string(),
            max_clients: 18,
            udp_port: 9600,
            tcp_port: 9600,
            http_port: 8081,
            loop_mode: true,
            sun_angle: 48,
            pickup_mode: true,
            dynamic_track: None,
        }
    }
}

impl Default for DynamicTrack {
    /// The defaults a fresh install uses.
    fn default() -> Self {
        DynamicTrack {
            session_start: 100,
            randomness: 0,
            session_transfer: 100,
            lap_gain: 10,
        }
    }
}

/// One `[CAR_n]` section of `entry_list.ini`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entry {
    /// `MODEL`, the car model id.
    pub model: String,
    /// `SKIN`, the skin folder name.
    pub skin: String,
    /// `GUID`, the driver's Steam GUID. Empty for an open slot.
    pub guid: String,
    /// `DRIVERNAME`.
    pub driver_name: String,
    /// `SPECTATOR_MODE`, written as 1 or 0.
    pub spectator: bool,
    /// `BALLAST`, in kilograms.
    pub ballast_kg: u32,
    /// `RESTRICTOR`, percent power restriction.
    pub restrictor: u32,
}

/// Reads `cfg_dir/server_cfg.ini`.
///
/// A missing file returns an `io::ErrorKind::NotFound` error, and the caller decides whether
/// to fall back to `ServerConfig::default()`. Keys that are absent or unparseable take their
/// default, so a partly filled file still loads.
pub fn read_server_config(cfg_dir: &Path) -> io::Result<ServerConfig> {
    let ini = Ini::load(&cfg_dir.join(SERVER_CFG))?;
    let d = ServerConfig::default();
    let dynamic_track = if has_section(&ini, DYNAMIC_TRACK) {
        let t = DynamicTrack::default();
        Some(DynamicTrack {
            session_start: num(&ini, DYNAMIC_TRACK, "SESSION_START", t.session_start),
            randomness: num(&ini, DYNAMIC_TRACK, "RANDOMNESS", t.randomness),
            session_transfer: num(&ini, DYNAMIC_TRACK, "SESSION_TRANSFER", t.session_transfer),
            lap_gain: num(&ini, DYNAMIC_TRACK, "LAP_GAIN", t.lap_gain),
        })
    } else {
        None
    };
    Ok(ServerConfig {
        name: text(&ini, SERVER, "NAME", d.name),
        password: text(&ini, SERVER, "PASSWORD", d.password),
        admin_password: text(&ini, SERVER, "ADMIN_PASSWORD", d.admin_password),
        cars: ini.get(SERVER, "CARS").map(split_list).unwrap_or(d.cars),
        track: text(&ini, SERVER, "TRACK", d.track),
        config_track: text(&ini, SERVER, "CONFIG_TRACK", d.config_track),
        max_clients: num(&ini, SERVER, "MAX_CLIENTS", d.max_clients),
        udp_port: num(&ini, SERVER, "UDP_PORT", d.udp_port),
        tcp_port: num(&ini, SERVER, "TCP_PORT", d.tcp_port),
        http_port: num(&ini, SERVER, "HTTP_PORT", d.http_port),
        loop_mode: flag(&ini, SERVER, "LOOP_MODE", d.loop_mode),
        sun_angle: num(&ini, SERVER, "SUN_ANGLE", d.sun_angle),
        pickup_mode: flag(&ini, SERVER, "PICKUP_MODE_ENABLED", d.pickup_mode),
        dynamic_track,
    })
}

/// Writes `cfg` into `cfg_dir/server_cfg.ini`.
///
/// Only the keys in [`ServerConfig`] are set. Every other key, comment and section already in
/// the file is kept. The `[DYNAMIC_TRACK]` section is only touched when `dynamic_track` is `Some`.
/// The previous file is moved to a backup first. Values containing line breaks are rejected
/// with `InvalidInput`. Call [`validate`] for semantic checks.
pub fn write_server_config(cfg_dir: &Path, cfg: &ServerConfig) -> io::Result<()> {
    let path = cfg_dir.join(SERVER_CFG);
    check_strings(
        [&cfg.name, &cfg.password, &cfg.admin_password, &cfg.track, &cfg.config_track]
            .into_iter()
            .chain(&cfg.cars),
    )?;

    let mut ini = if path.exists() { Ini::load(&path)? } else { Ini::default() };
    ini.set(SERVER, "NAME", &cfg.name);
    ini.set(SERVER, "PASSWORD", &cfg.password);
    ini.set(SERVER, "ADMIN_PASSWORD", &cfg.admin_password);
    ini.set(SERVER, "CARS", cfg.cars.join(";"));
    ini.set(SERVER, "TRACK", &cfg.track);
    ini.set(SERVER, "CONFIG_TRACK", &cfg.config_track);
    ini.set(SERVER, "MAX_CLIENTS", cfg.max_clients);
    ini.set(SERVER, "UDP_PORT", cfg.udp_port);
    ini.set(SERVER, "TCP_PORT", cfg.tcp_port);
    ini.set(SERVER, "HTTP_PORT", cfg.http_port);
    ini.set(SERVER, "LOOP_MODE", u8::from(cfg.loop_mode));
    ini.set(SERVER, "SUN_ANGLE", cfg.sun_angle);
    ini.set(SERVER, "PICKUP_MODE_ENABLED", u8::from(cfg.pickup_mode));
    if let Some(dt) = &cfg.dynamic_track {
        ini.set(DYNAMIC_TRACK, "SESSION_START", dt.session_start);
        ini.set(DYNAMIC_TRACK, "RANDOMNESS", dt.randomness);
        ini.set(DYNAMIC_TRACK, "SESSION_TRANSFER", dt.session_transfer);
        ini.set(DYNAMIC_TRACK, "LAP_GAIN", dt.lap_gain);
    }
    ini.save_with_backup(&path)
}

/// Writes `cfg_dir/entry_list.ini` with one `[CAR_n]` section per entry, numbered from 0.
///
/// Each section gets `MODEL`, `SKIN`, `SPECTATOR_MODE` (1 or 0), `DRIVERNAME`, `GUID`,
/// `BALLAST` and `RESTRICTOR`. The file is replaced, and the previous one is kept as a backup.
/// An empty list or more than 255 entries is an `InvalidInput` error, because the server
/// refuses an empty entry list.
pub fn write_entry_list(cfg_dir: &Path, entries: &[Entry]) -> io::Result<()> {
    if entries.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "entry list needs at least one entry"));
    }
    if entries.len() > MAX_ENTRIES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("entry list has {} entries, the limit is {MAX_ENTRIES}", entries.len()),
        ));
    }
    let mut ini = Ini::default();
    for (i, e) in entries.iter().enumerate() {
        check_strings([&e.model, &e.skin, &e.guid, &e.driver_name])?;
        let section = format!("CAR_{i}");
        ini.set(&section, "MODEL", &e.model);
        ini.set(&section, "SKIN", &e.skin);
        ini.set(&section, "SPECTATOR_MODE", u8::from(e.spectator));
        ini.set(&section, "DRIVERNAME", &e.driver_name);
        ini.set(&section, "GUID", &e.guid);
        ini.set(&section, "BALLAST", e.ballast_kg);
        ini.set(&section, "RESTRICTOR", e.restrictor);
    }
    ini.save_with_backup(&cfg_dir.join(ENTRY_LIST))
}

/// Returns human-readable problems with `cfg`. An empty list means nothing was found.
///
/// Checks: empty name, track, or car list; zero `MAX_CLIENTS`; zero ports; `HTTP_PORT` equal to
/// `UDP_PORT` or `TCP_PORT`; empty `ADMIN_PASSWORD` (any driver could join as admin). `UDP_PORT` and
/// `TCP_PORT` may share a number: the default is 9600 for both, and they are different protocols.
pub fn validate(cfg: &ServerConfig) -> Vec<String> {
    let mut problems = Vec::new();
    if cfg.name.trim().is_empty() {
        problems.push("server name (NAME) is empty".to_string());
    }
    if cfg.track.trim().is_empty() {
        problems.push("track (TRACK) is not set".to_string());
    }
    if cfg.cars.is_empty() {
        problems.push("no cars are listed (CARS is empty)".to_string());
    }
    if cfg.max_clients == 0 {
        problems.push("max clients (MAX_CLIENTS) must be at least 1".to_string());
    }
    for (key, port) in [("UDP_PORT", cfg.udp_port), ("TCP_PORT", cfg.tcp_port), ("HTTP_PORT", cfg.http_port)] {
        if port == 0 {
            problems.push(format!("{key} must not be 0"));
        }
    }
    if cfg.http_port != 0 {
        for (key, port) in [("UDP_PORT", cfg.udp_port), ("TCP_PORT", cfg.tcp_port)] {
            if port == cfg.http_port {
                problems.push(format!("HTTP_PORT and {key} are both {port}; they must differ"));
            }
        }
    }
    if cfg.admin_password.trim().is_empty() {
        problems.push("admin password (ADMIN_PASSWORD) is empty: any driver can join as admin".to_string());
    }
    problems
}

/// Names of the starting presets, in menu order. Each one is accepted by [`preset`].
pub fn preset_names() -> &'static [&'static str] {
    &["Casual race", "Practice", "Sprint"]
}

/// A starting config for one of the [`preset_names`]. Any other name returns `None`.
///
/// Presets only pick the fields a server config owns. They set `admin_password` to a
/// placeholder so [`validate`] passes; callers should replace it before saving.
pub fn preset(name: &str) -> Option<ServerConfig> {
    let cars = ["ks_ferrari_488_gt3", "bmw_m3_e30", "ks_mazda_mx5"].map(str::to_string).to_vec();
    let cfg = match name {
        "Casual race" => ServerConfig {
            name: "Casual Race".to_string(),
            admin_password: "change-me".to_string(),
            cars,
            track: "ks_silverstone".to_string(),
            config_track: "gp".to_string(),
            max_clients: 16,
            loop_mode: true,
            pickup_mode: true,
            dynamic_track: Some(DynamicTrack { session_start: 95, randomness: 2, session_transfer: 90, lap_gain: 10 }),
            ..ServerConfig::default()
        },
        "Practice" => ServerConfig {
            name: "Practice".to_string(),
            admin_password: "change-me".to_string(),
            cars,
            track: "monza".to_string(),
            config_track: String::new(),
            max_clients: 12,
            loop_mode: false,
            pickup_mode: false,
            dynamic_track: Some(DynamicTrack { session_start: 100, randomness: 0, session_transfer: 100, lap_gain: 10 }),
            ..ServerConfig::default()
        },
        "Sprint" => ServerConfig {
            name: "Sprint".to_string(),
            admin_password: "change-me".to_string(),
            cars,
            track: "ks_red_bull_ring".to_string(),
            config_track: String::new(),
            max_clients: 20,
            loop_mode: true,
            pickup_mode: true,
            dynamic_track: None,
            ..ServerConfig::default()
        },
        _ => return None,
    };
    Some(cfg)
}

fn has_section(ini: &Ini, name: &str) -> bool {
    ini.section_names().iter().any(|s| s.eq_ignore_ascii_case(name))
}

fn text(ini: &Ini, section: &str, key: &str, default: String) -> String {
    ini.get(section, key).map(str::to_string).unwrap_or(default)
}

/// Integer value, or `default` when the key is missing, not a number, or out of range for `T`.
fn num<T: TryFrom<i64>>(ini: &Ini, section: &str, key: &str, default: T) -> T {
    ini.get_i64(section, key).and_then(|v| T::try_from(v).ok()).unwrap_or(default)
}

/// Flag stored as 0 or 1. Any non-zero number is true.
fn flag(ini: &Ini, section: &str, key: &str, default: bool) -> bool {
    ini.get_i64(section, key).map(|v| v != 0).unwrap_or(default)
}

/// Semicolon-separated list, trimmed, with empty items dropped.
fn split_list(value: &str) -> Vec<String> {
    value.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Rejects values with a line break, which would corrupt the line-based INI file.
fn check_strings<'a>(values: impl IntoIterator<Item = &'a String>) -> io::Result<()> {
    for value in values {
        if value.contains(['\r', '\n']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("value {value:?} contains a line break"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A unique directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("acxs-server-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn backups(dir: &Path, file: &str) -> usize {
        fs::read_dir(dir)
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(&format!("{file}.acxs-backup-")))
            .count()
    }

    #[test]
    fn missing_file_is_not_found() {
        let tmp = TempDir::new("missing");
        let err = read_server_config(tmp.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn round_trip_server_config() {
        let tmp = TempDir::new("roundtrip");
        let cfg = ServerConfig {
            name: "ACXS Test".to_string(),
            password: "pw".to_string(),
            admin_password: "admin".to_string(),
            cars: vec!["ks_ferrari_488_gt3".to_string(), "bmw_m3_e30".to_string()],
            track: "monza".to_string(),
            config_track: "".to_string(),
            max_clients: 24,
            udp_port: 9601,
            tcp_port: 9602,
            http_port: 8082,
            loop_mode: false,
            sun_angle: -20,
            pickup_mode: false,
            dynamic_track: Some(DynamicTrack {
                session_start: 95,
                randomness: 2,
                session_transfer: 80,
                lap_gain: 12,
            }),
        };
        write_server_config(tmp.path(), &cfg).unwrap();
        assert_eq!(read_server_config(tmp.path()).unwrap(), cfg);

        let text = fs::read_to_string(tmp.path().join(SERVER_CFG)).unwrap();
        assert!(text.contains("[SERVER]\r\n"));
        assert!(text.contains("CARS=ks_ferrari_488_gt3;bmw_m3_e30\r\n"));
        assert!(text.contains("LOOP_MODE=0\r\n"));
        assert!(text.contains("[DYNAMIC_TRACK]\r\n"));
    }

    #[test]
    fn default_config_writes_no_dynamic_track() {
        let tmp = TempDir::new("no-dt");
        write_server_config(tmp.path(), &ServerConfig::default()).unwrap();
        let text = fs::read_to_string(tmp.path().join(SERVER_CFG)).unwrap();
        assert!(!text.contains("DYNAMIC_TRACK"));
        assert_eq!(read_server_config(tmp.path()).unwrap().dynamic_track, None);
    }

    #[test]
    fn unknown_keys_and_sections_are_preserved() {
        let tmp = TempDir::new("unknown");
        let original = "; my server\r\n[SERVER]\r\nNAME=Old Name\r\nCUSTOM_KEY=keep me\r\nMAX_CLIENTS=10\r\n\r\n[EXTRA]\r\nX=1\r\n";
        fs::write(tmp.path().join(SERVER_CFG), original).unwrap();

        let mut cfg = read_server_config(tmp.path()).unwrap();
        assert_eq!(cfg.name, "Old Name");
        assert_eq!(cfg.max_clients, 10);
        cfg.name = "New Name".to_string();
        write_server_config(tmp.path(), &cfg).unwrap();

        let text = fs::read_to_string(tmp.path().join(SERVER_CFG)).unwrap();
        assert!(text.starts_with("; my server\r\n"));
        assert!(text.contains("NAME=New Name\r\n"));
        assert!(text.contains("CUSTOM_KEY=keep me\r\n"));
        assert!(text.contains("[EXTRA]\r\nX=1\r\n"));
        assert_eq!(backups(tmp.path(), SERVER_CFG), 1);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let tmp = TempDir::new("partial");
        fs::write(tmp.path().join(SERVER_CFG), "[SERVER]\r\nTRACK=spa\r\nUDP_PORT=bogus\r\n").unwrap();
        let cfg = read_server_config(tmp.path()).unwrap();
        let d = ServerConfig::default();
        assert_eq!(cfg.track, "spa");
        assert_eq!(cfg.udp_port, d.udp_port);
        assert_eq!(cfg.name, d.name);
        assert_eq!(cfg.dynamic_track, None);
    }

    #[test]
    fn validate_default_config_only_flags_admin_password() {
        let problems = validate(&ServerConfig::default());
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("ADMIN_PASSWORD"));

        let cfg = ServerConfig { admin_password: "x".to_string(), ..ServerConfig::default() };
        assert!(validate(&cfg).is_empty());
    }

    #[test]
    fn validate_catches_problems() {
        let base = ServerConfig { admin_password: "x".to_string(), ..ServerConfig::default() };

        let empty_track = ServerConfig { track: "  ".to_string(), ..base.clone() };
        assert!(validate(&empty_track).iter().any(|p| p.contains("TRACK")));

        let no_cars = ServerConfig { cars: vec![], ..base.clone() };
        assert!(validate(&no_cars).iter().any(|p| p.contains("CARS")));

        let no_clients = ServerConfig { max_clients: 0, ..base.clone() };
        assert!(validate(&no_clients).iter().any(|p| p.contains("MAX_CLIENTS")));

        let zero_port = ServerConfig { tcp_port: 0, ..base.clone() };
        assert!(validate(&zero_port).iter().any(|p| p.contains("TCP_PORT must not be 0")));

        let http_clash = ServerConfig { http_port: 9600, ..base.clone() };
        let problems = validate(&http_clash);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("UDP_PORT")));
        assert!(problems.iter().any(|p| p.contains("TCP_PORT")));

        let udp_tcp_same = ServerConfig { udp_port: 9700, tcp_port: 9700, ..base.clone() };
        assert!(validate(&udp_tcp_same).is_empty());

        let no_admin = ServerConfig { admin_password: String::new(), ..base };
        assert!(validate(&no_admin).iter().any(|p| p.contains("ADMIN_PASSWORD")));
    }

    #[test]
    fn presets_validate_without_problems() {
        for name in preset_names() {
            let cfg = preset(name).unwrap_or_else(|| panic!("preset {name} is missing"));
            assert!(validate(&cfg).is_empty(), "{name}: {:?}", validate(&cfg));
        }
    }

    #[test]
    fn unknown_preset_is_none() {
        assert!(preset("Endurance").is_none());
        assert!(preset("").is_none());
        assert!(preset("casual race").is_none());
    }

    #[test]
    fn rejects_line_breaks_in_values() {
        let tmp = TempDir::new("newline");
        let cfg = ServerConfig { name: "bad\nname".to_string(), ..ServerConfig::default() };
        let err = write_server_config(tmp.path(), &cfg).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!tmp.path().join(SERVER_CFG).exists());
    }

    fn sample_entry(model: &str, driver: &str, spectator: bool) -> Entry {
        Entry {
            model: model.to_string(),
            skin: "red".to_string(),
            guid: "76561198000000001".to_string(),
            driver_name: driver.to_string(),
            spectator,
            ballast_kg: 20,
            restrictor: 5,
        }
    }

    #[test]
    fn entry_list_writes_numbered_car_sections() {
        let tmp = TempDir::new("entries");
        let entries = [sample_entry("bmw_m3_e30", "Alice", false), sample_entry("ks_mazda_mx5", "Bob", true)];
        write_entry_list(tmp.path(), &entries).unwrap();

        let text = fs::read_to_string(tmp.path().join(ENTRY_LIST)).unwrap();
        assert!(text.contains("[CAR_0]\r\nMODEL=bmw_m3_e30\r\nSKIN=red\r\nSPECTATOR_MODE=0\r\nDRIVERNAME=Alice\r\nGUID=76561198000000001\r\nBALLAST=20\r\nRESTRICTOR=5\r\n"), "{text}");
        assert!(text.contains("[CAR_1]\r\nMODEL=ks_mazda_mx5\r\n"), "{text}");

        let ini = Ini::load(&tmp.path().join(ENTRY_LIST)).unwrap();
        assert_eq!(ini.get("CAR_1", "DRIVERNAME"), Some("Bob"));
        assert_eq!(ini.get_i64("CAR_1", "SPECTATOR_MODE"), Some(1));
        assert_eq!(ini.get_i64("CAR_0", "BALLAST"), Some(20));
    }

    #[test]
    fn entry_list_replaces_and_backs_up_previous_file() {
        let tmp = TempDir::new("entries-backup");
        write_entry_list(tmp.path(), &[sample_entry("a", "One", false), sample_entry("b", "Two", false)]).unwrap();
        write_entry_list(tmp.path(), &[sample_entry("c", "Three", false)]).unwrap();

        let ini = Ini::load(&tmp.path().join(ENTRY_LIST)).unwrap();
        assert_eq!(ini.get("CAR_0", "MODEL"), Some("c"));
        assert_eq!(ini.get("CAR_1", "MODEL"), None);
        assert_eq!(backups(tmp.path(), ENTRY_LIST), 1);
    }

    #[test]
    fn entry_list_rejects_empty_and_oversized() {
        let tmp = TempDir::new("entries-bad");
        assert_eq!(write_entry_list(tmp.path(), &[]).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        let many = vec![Entry::default(); MAX_ENTRIES + 1];
        assert_eq!(write_entry_list(tmp.path(), &many).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(!tmp.path().join(ENTRY_LIST).exists());
    }
}

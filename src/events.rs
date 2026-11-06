//! The game's own events: Special Events and the Career series.
//!
//! Each event is a folder with an `event.ini`, which already has the shape of a `race.ini`: the
//! game takes it nearly as it is. Reading is read-only; [`to_race_ini`] builds the file to start
//! one with, and the game's folders are never changed.

use std::path::{Path, PathBuf};

use crate::ini::Ini;

/// One event that can be started.
#[derive(Debug, Clone, PartialEq)]
pub struct GameEvent {
    /// Folder name, unique within its series.
    pub id: String,
    pub name: String,
    /// `Hotlap`, `Race`, `Drift`, `Time attack` or `Drag`, from the session type.
    pub kind: String,
    /// Empty for Special Events, the series name for Career events.
    pub series: String,
    pub track: String,
    pub layout: String,
    pub car: String,
    pub skin: String,
    /// What the medals ask for, easiest first, such as `1:30.000` or `5000 points`.
    pub goals: Vec<String>,
    pub ini: PathBuf,
    pub preview: Option<PathBuf>,
}

fn read_ini(path: &Path) -> Option<Ini> {
    let bytes = std::fs::read(path).ok()?;
    Some(Ini::parse(&String::from_utf8_lossy(&bytes)))
}

fn session_kind(ini: &Ini) -> &'static str {
    match ini.get_i64("SESSION_0", "TYPE") {
        Some(4) => "Hotlap",
        Some(5) => "Time attack",
        Some(6) => "Drift",
        Some(7) => "Drag",
        Some(2) => "Qualifying",
        Some(1) => "Practice",
        _ => "Race",
    }
}

/// The three medal goals. An objective may carry a trailing comment.
fn goals(ini: &Ini) -> Vec<String> {
    (0..3)
        .filter_map(|i| {
            let section = format!("CONDITION_{i}");
            let kind = ini.get(&section, "TYPE")?.split(';').next()?.trim().to_uppercase();
            let objective: u32 = ini.get(&section, "OBJECTIVE")?.split(';').next()?.trim().parse().ok()?;
            Some(match kind.as_str() {
                "TIME" => crate::laptimes::format_lap(objective),
                "POINTS" => format!("{objective} points"),
                "WINS" => format!("{objective} wins"),
                "AI" => format!("P{objective}"),
                _ => objective.to_string(),
            })
        })
        .collect()
}

fn read_event(dir: &Path, series: &str) -> Option<GameEvent> {
    let ini_path = dir.join("event.ini");
    let ini = read_ini(&ini_path)?;
    let id = dir.file_name()?.to_string_lossy().into_owned();
    let text = |section: &str, key: &str| ini.get(section, key).unwrap_or_default().trim().to_string();
    let track = text("RACE", "TRACK");
    let car = text("RACE", "MODEL");
    if track.is_empty() || car.is_empty() {
        return None;
    }
    let name = text("EVENT", "NAME");
    let mut skin = text("CAR_0", "SKIN");
    if skin.is_empty() {
        skin = text("RACE", "SKIN");
    }
    let preview = dir.join("preview.png");
    Some(GameEvent {
        name: if name.is_empty() { id.clone() } else { name },
        kind: session_kind(&ini).to_string(),
        series: series.to_string(),
        track,
        layout: text("RACE", "CONFIG_TRACK"),
        car,
        skin,
        goals: goals(&ini),
        ini: ini_path,
        preview: preview.is_file().then_some(preview),
        id,
    })
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    // SPECIAL_EVENT_2 before SPECIAL_EVENT_10.
    dirs.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let digits: String = name.chars().filter(char::is_ascii_digit).collect();
        (digits.parse::<u32>().unwrap_or(u32::MAX), name)
    });
    dirs
}

/// Special Events, in the game's order.
pub fn special_events(ac_root: &Path) -> Vec<GameEvent> {
    subfolders(&ac_root.join("content").join("specialevents"))
        .iter()
        .filter_map(|dir| read_event(dir, ""))
        .collect()
}

/// Career events, series by series.
pub fn career_events(ac_root: &Path) -> Vec<GameEvent> {
    let mut events = Vec::new();
    for series in subfolders(&ac_root.join("content").join("career")) {
        let name = read_ini(&series.join("series.ini"))
            .and_then(|ini| ini.get("SERIES", "NAME").map(|n| n.trim().to_string()))
            .unwrap_or_else(|| series.file_name().unwrap_or_default().to_string_lossy().into_owned());
        events.extend(subfolders(&series).iter().filter_map(|dir| read_event(dir, &name)));
    }
    events
}

impl GameEvent {
    /// True when the car and track are installed. DLC events may name content this PC lacks.
    pub fn is_playable(&self, ac_root: &Path) -> bool {
        let content = ac_root.join("content");
        let track = content.join("tracks").join(&self.track);
        content.join("cars").join(&self.car).is_dir()
            && (self.layout.is_empty() && track.is_dir() || !self.layout.is_empty() && track.join(&self.layout).is_dir())
    }
}

/// The `race.ini` for an event: its own text with the player's name and the sections the game
/// expects for a local session.
pub fn to_race_ini(event_text: &str, player_name: &str) -> String {
    let mut ini = Ini::parse(event_text);
    if !player_name.trim().is_empty() {
        ini.set("CAR_0", "DRIVER_NAME", player_name.trim());
    }
    ini.set("REMOTE", "ACTIVE", 0);
    for section in ["REPLAY", "RESTART", "BENCHMARK"] {
        ini.set(section, "ACTIVE", 0);
    }
    ini.set("HEADER", "VERSION", 2);
    ini.to_string_crlf()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: &str = "[EVENT]\nNAME=German Fury\nDESCRIPTION=Hotlap\n\n[RACE]\nTRACK=magione\nCONFIG_TRACK=\nMODEL=bmw_m3_e92_drift\nCARS=1\n\n[CAR_0]\nMODEL=-\nSKIN=bmw_academy\nDRIVER_NAME=Ramon Castillo\n\n[SESSION_0]\nNAME=Hotlap\nTYPE=4\nSPAWN_SET=HOTLAP_START\n\n[CONDITION_0]\nTYPE=TIME\nOBJECTIVE=90000\n\n[CONDITION_1]\nTYPE=POINTS   ; TIME / POINTS\nOBJECTIVE=3500 ; time in milliseconds\n";

    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("acxs-events-{tag}-{}", std::process::id()));
        for i in [2, 10] {
            let dir = root.join("content/specialevents").join(format!("SPECIAL_EVENT_{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("event.ini"), EVENT).unwrap();
        }
        let career = root.join("content/career/series0");
        std::fs::create_dir_all(career.join("event1")).unwrap();
        std::fs::write(career.join("series.ini"), "[SERIES]\nNAME=Season zero\n").unwrap();
        std::fs::write(career.join("event1/event.ini"), EVENT).unwrap();
        root
    }

    #[test]
    fn events_are_read_in_game_order_with_their_goals() {
        let root = fixture("read");
        let events = special_events(&root);
        assert_eq!(events.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["SPECIAL_EVENT_2", "SPECIAL_EVENT_10"]);
        let event = &events[0];
        assert_eq!((event.name.as_str(), event.kind.as_str()), ("German Fury", "Hotlap"));
        assert_eq!(event.skin, "bmw_academy");
        assert_eq!(event.goals, ["1:30.000", "3500 points"]);
        let career = career_events(&root);
        assert_eq!(career.len(), 1);
        assert_eq!(career[0].series, "Season zero");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_content_is_not_playable() {
        let root = fixture("play");
        let event = special_events(&root).remove(0);
        assert!(!event.is_playable(&root));
        std::fs::create_dir_all(root.join("content/cars/bmw_m3_e92_drift")).unwrap();
        std::fs::create_dir_all(root.join("content/tracks/magione")).unwrap();
        assert!(event.is_playable(&root));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_race_file_keeps_the_event_and_names_the_player() {
        let text = to_race_ini(EVENT, "Leon");
        let ini = Ini::parse(&text);
        assert_eq!(ini.get("CAR_0", "DRIVER_NAME"), Some("Leon"));
        assert_eq!(ini.get("SESSION_0", "SPAWN_SET"), Some("HOTLAP_START"));
        assert_eq!(ini.get_i64("REMOTE", "ACTIVE"), Some(0));
        assert_eq!(ini.get_i64("HEADER", "VERSION"), Some(2));
    }
}

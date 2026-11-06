//! Driving history imported from the Content Manager profile (`Profile (Sessions).data`).
//!
//! The file is read only, never changed. Each line is one JSON object describing a session.
//! Everything here is pure apart from [`default_profile_path`] and [`load_profile`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Longest airborne time in seconds that counts as a real jump.
const MAX_REAL_JUMP_S: f64 = 60.0;

/// One driven session, as far as the Content Manager recorded it.
#[derive(Debug, Clone, PartialEq)]
pub struct DrivenSession {
    pub car_id: String,
    pub track_id: String,
    /// Local start date, `YYYY-MM-DD`.
    pub date: String,
    /// Seconds in the session.
    pub seconds: f64,
    pub crashes: u32,
    pub offroad: u32,
    /// Top speed in km/h.
    pub max_speed: f64,
    /// Distance in metres.
    pub distance_m: f64,
    /// Longest jump, all four wheels off the ground, in seconds.
    pub longest_airborne: f64,
    /// Best lap in milliseconds, if a real lap was set.
    pub best_lap_ms: Option<u32>,
    /// Only recorded by ACXS, zero for imported sessions.
    pub lockups: u32,
    pub wheelspins: u32,
    pub cornering_seconds: f64,
    pub understeer_seconds: f64,
    pub oversteer_seconds: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawSession {
    car_id: Option<String>,
    track_id: Option<String>,
    started_at: Option<String>,
    time: Option<String>,
    best_lap: Option<String>,
    #[serde(default)]
    total_crashes: u32,
    #[serde(default)]
    gone_offroad: u32,
    #[serde(default)]
    max_speed: f64,
    #[serde(default)]
    distance: f64,
    #[serde(default)]
    longest_airborne: f64,
}

/// Parses `HH:MM:SS.fffffff` (any number of fraction digits) into seconds.
pub fn parse_duration(text: &str) -> Option<f64> {
    let mut parts = text.trim().split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// Parses the profile text. Lines that are blank or not valid sessions are skipped.
pub fn parse_profile(text: &str) -> Vec<DrivenSession> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<RawSession>(line.trim()).ok())
        .filter_map(|raw| {
            let car_id = raw.car_id.filter(|id| !id.is_empty())?;
            let track_id = raw.track_id.filter(|id| !id.is_empty())?;
            let date = raw.started_at?.get(..10)?.to_string();
            let seconds = parse_duration(&raw.time?)?;
            // The game reports a placeholder lap of one millisecond when none was set.
            let best_lap_ms = raw
                .best_lap
                .and_then(|lap| parse_duration(&lap))
                .map(|secs| (secs * 1000.0).round() as u32)
                .filter(|ms| *ms >= 1000);
            Some(DrivenSession {
                car_id,
                track_id,
                date,
                seconds,
                crashes: raw.total_crashes,
                offroad: raw.gone_offroad,
                max_speed: raw.max_speed,
                distance_m: raw.distance,
                longest_airborne: raw.longest_airborne,
                best_lap_ms,
                lockups: 0,
                wheelspins: 0,
                cornering_seconds: 0.0,
                understeer_seconds: 0.0,
                oversteer_seconds: 0.0,
            })
        })
        .collect()
}

/// `%LOCALAPPDATA%\AcTools Content Manager\Progress\Profile (Sessions).data`.
pub fn default_profile_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("AcTools Content Manager").join("Progress").join("Profile (Sessions).data"))
}

/// Reads and parses the profile file. A missing or unreadable file gives an empty history.
pub fn load_profile(path: &Path) -> Vec<DrivenSession> {
    std::fs::read_to_string(path).map(|text| parse_profile(&text)).unwrap_or_default()
}

/// Adds `recorded` sessions to the imported history. A recorded session that matches an
/// imported one (same car, track and day, drive time within a minute) is the same drive seen
/// by both tools, so it is skipped.
pub fn merge(mut imported: Vec<DrivenSession>, recorded: Vec<DrivenSession>) -> Vec<DrivenSession> {
    for new in recorded {
        let duplicate = imported.iter().any(|old| {
            old.car_id == new.car_id
                && old.track_id == new.track_id
                && old.date == new.date
                && (old.seconds - new.seconds).abs() < 60.0
        });
        if !duplicate {
            imported.push(new);
        }
    }
    imported
}

/// A label with a total, such as a car and the seconds spent in it.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageEntry {
    pub id: String,
    pub sessions: usize,
    pub seconds: f64,
}

/// Summary of the whole history.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrivingStats {
    pub sessions: usize,
    pub seconds: f64,
    pub distance_km: f64,
    pub crashes: u32,
    pub offroad: u32,
    pub max_speed: f64,
    pub longest_airborne: f64,
    /// Crashes per 100 km, `None` below one kilometre driven.
    pub crashes_per_100km: Option<f64>,
    pub lockups: u32,
    pub wheelspins: u32,
    /// Share of judged hard cornering spent in under- and oversteer, `None` without data.
    pub understeer_share: Option<f64>,
    pub oversteer_share: Option<f64>,
    /// Most driven first.
    pub top_cars: Vec<UsageEntry>,
    pub top_tracks: Vec<UsageEntry>,
    /// Hours driven per ISO week, oldest first, as `(week label, hours)`. The last 12 weeks that have data.
    pub weekly_hours: Vec<(String, f64)>,
    /// Best lap per car and track, most driven first. Filled by the caller, not by `summarize`.
    pub bests: Vec<PersonalBest>,
}

/// ISO week label `YYYY-Www` for a `YYYY-MM-DD` date.
fn week_label(date: &str) -> Option<String> {
    use chrono::{Datelike, NaiveDate};
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let week = day.iso_week();
    Some(format!("{}-W{:02}", week.year(), week.week()))
}

fn rank(map: BTreeMap<String, (usize, f64)>) -> Vec<UsageEntry> {
    let mut list: Vec<UsageEntry> =
        map.into_iter().map(|(id, (sessions, seconds))| UsageEntry { id, sessions, seconds }).collect();
    list.sort_by(|a, b| b.seconds.total_cmp(&a.seconds).then_with(|| a.id.cmp(&b.id)));
    list
}

/// Aggregates the sessions.
pub fn summarize(sessions: &[DrivenSession]) -> DrivingStats {
    let mut stats = DrivingStats { sessions: sessions.len(), ..DrivingStats::default() };
    let mut cars: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    let mut tracks: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    let mut weeks: BTreeMap<String, f64> = BTreeMap::new();
    let mut metres = 0.0;
    let (mut corner, mut under, mut over) = (0.0, 0.0, 0.0);
    for s in sessions {
        stats.seconds += s.seconds;
        metres += s.distance_m;
        stats.crashes += s.crashes;
        stats.offroad += s.offroad;
        stats.lockups += s.lockups;
        stats.wheelspins += s.wheelspins;
        corner += s.cornering_seconds;
        under += s.understeer_seconds;
        over += s.oversteer_seconds;
        stats.max_speed = stats.max_speed.max(s.max_speed);
        // The game reports a fall off the map as a jump of minutes; ignore values that cannot be a real jump.
        if s.longest_airborne <= MAX_REAL_JUMP_S {
            stats.longest_airborne = stats.longest_airborne.max(s.longest_airborne);
        }
        let car = cars.entry(s.car_id.clone()).or_default();
        car.0 += 1;
        car.1 += s.seconds;
        let track = tracks.entry(s.track_id.clone()).or_default();
        track.0 += 1;
        track.1 += s.seconds;
        if let Some(label) = week_label(&s.date) {
            *weeks.entry(label).or_default() += s.seconds / 3600.0;
        }
    }
    stats.distance_km = metres / 1000.0;
    stats.crashes_per_100km = (stats.distance_km >= 1.0).then(|| f64::from(stats.crashes) / stats.distance_km * 100.0);
    if corner >= 10.0 {
        stats.understeer_share = Some(under / corner);
        stats.oversteer_share = Some(over / corner);
    }
    stats.top_cars = rank(cars);
    stats.top_tracks = rank(tracks);
    let skip = weeks.len().saturating_sub(12);
    stats.weekly_hours = weeks.into_iter().skip(skip).collect();
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        r#"{"CarId":"bmw_m3_e92","TrackId":"ks_nordschleife/touristenfahrten","StartedAt":"2026-04-14T23:25:27.8327522+02:00","Time":"00:09:35.4815712","TotalCrashes":3,"GoneOffroad":6,"MaxSpeed":251.6,"Distance":20856.7}"#,
        "\n\n",
        r#"{"CarId":"ks_porsche_911_gt3_r_2016","TrackId":"la_canyons/freeroam","StartedAt":"2026-04-15T21:11:40.0815477+02:00","Time":"01:00:00","BestLap":"00:00:00.0010000","MaxSpeed":264.9,"Distance":40000.0,"LongestAirborne":28.4}"#,
        "\nnot json\n",
        r#"{"CarId":"ks_porsche_911_gt3_r_2016","TrackId":"la_canyons/freeroam","StartedAt":"2026-04-16T21:11:40+02:00","Time":"00:30:00","BestLap":"00:01:54.311","Distance":1000}"#,
    );

    #[test]
    fn duration_parses_fraction_and_rejects_garbage() {
        assert!((parse_duration("00:09:35.4815712").unwrap() - 575.4815712).abs() < 1e-6);
        assert_eq!(parse_duration("01:00:00"), Some(3600.0));
        assert_eq!(parse_duration("1:2"), None);
        assert_eq!(parse_duration("a:b:c"), None);
    }

    #[test]
    fn profile_skips_bad_lines_and_placeholder_laps() {
        let sessions = parse_profile(SAMPLE);
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].date, "2026-04-14");
        assert_eq!(sessions[0].crashes, 3);
        assert_eq!(sessions[1].best_lap_ms, None);
        assert_eq!(sessions[2].best_lap_ms, Some(114_311));
    }

    #[test]
    fn summary_totals_and_ranking() {
        let stats = summarize(&parse_profile(SAMPLE));
        assert_eq!(stats.sessions, 3);
        assert!((stats.distance_km - 61.8567).abs() < 1e-3);
        assert_eq!(stats.crashes, 3);
        assert_eq!(stats.max_speed, 264.9);
        assert_eq!(stats.longest_airborne, 28.4);
        assert_eq!(stats.top_cars[0].id, "ks_porsche_911_gt3_r_2016");
        assert_eq!(stats.top_cars[0].sessions, 2);
        assert_eq!(stats.top_tracks[0].id, "la_canyons/freeroam");
        let per100 = stats.crashes_per_100km.unwrap();
        assert!((per100 - 3.0 / 61.8567 * 100.0).abs() < 1e-3);
        assert_eq!(stats.weekly_hours.len(), 1);
    }

    #[test]
    fn merge_skips_the_same_drive_seen_twice() {
        let imported = parse_profile(SAMPLE);
        let mut same = imported[0].clone();
        same.seconds += 20.0;
        let mut other = imported[0].clone();
        other.date = "2026-05-01".into();
        let merged = merge(imported, vec![same, other]);
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn empty_history_is_harmless() {
        let stats = summarize(&[]);
        assert_eq!(stats.sessions, 0);
        assert_eq!(stats.crashes_per_100km, None);
        assert!(stats.weekly_hours.is_empty());
    }
}

/// One timed lap of the player.
#[derive(Debug, Clone, PartialEq)]
pub struct LapRow {
    pub car_id: String,
    /// `track-layout`, the Content Manager spelling.
    pub track_key: String,
    pub time_ms: u32,
}

#[derive(Deserialize)]
struct CmSession {
    track: Option<String>,
    #[serde(default)]
    players: Vec<CmPlayer>,
    #[serde(default)]
    sessions: Vec<CmRun>,
}

#[derive(Deserialize)]
struct CmPlayer {
    car: String,
}

#[derive(Deserialize)]
struct CmRun {
    #[serde(default)]
    laps: Vec<CmLap>,
}

#[derive(Deserialize)]
struct CmLap {
    #[serde(default)]
    car: i64,
    #[serde(default)]
    time: u64,
    #[serde(default)]
    cuts: u32,
}

/// Valid laps (no cuts, at least ten seconds) of the player, car 0, from one Content Manager
/// `Progress\Sessions\*.json` file.
pub fn parse_cm_laps(text: &str) -> Vec<LapRow> {
    let Ok(session) = serde_json::from_str::<CmSession>(text) else { return Vec::new() };
    let (Some(track), Some(player)) = (session.track, session.players.first()) else { return Vec::new() };
    session
        .sessions
        .iter()
        .flat_map(|run| &run.laps)
        .filter(|lap| lap.car == 0 && lap.cuts == 0 && (10_000..3_600_000).contains(&lap.time))
        .map(|lap| LapRow { car_id: player.car.clone(), track_key: track.clone(), time_ms: lap.time as u32 })
        .collect()
}

/// Reads every session file in the Content Manager's `Progress\Sessions` folder.
pub fn load_cm_laps() -> Vec<LapRow> {
    let Some(dir) = default_profile_path().and_then(|p| p.parent().map(|d| d.join("Sessions"))) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .flat_map(|text| parse_cm_laps(&text))
        .collect()
}

/// A personal best for one car on one track.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonalBest {
    pub car_id: String,
    pub track_key: String,
    pub time_ms: u32,
    pub laps: usize,
}

/// Best lap per car and track, tracks with the most laps first. `/` and `-` in track keys are
/// treated alike so both sources land on the same entry.
pub fn personal_bests(rows: &[LapRow]) -> Vec<PersonalBest> {
    let mut map: BTreeMap<(String, String), (u32, usize)> = BTreeMap::new();
    for row in rows {
        let entry = map.entry((row.car_id.clone(), row.track_key.replace('/', "-"))).or_insert((u32::MAX, 0));
        entry.0 = entry.0.min(row.time_ms);
        entry.1 += 1;
    }
    let mut list: Vec<PersonalBest> = map
        .into_iter()
        .map(|((car_id, track_key), (time_ms, laps))| PersonalBest { car_id, track_key, time_ms, laps })
        .collect();
    list.sort_by(|a, b| b.laps.cmp(&a.laps).then_with(|| a.track_key.cmp(&b.track_key)));
    list
}

#[cfg(test)]
mod lap_tests {
    use super::*;

    const CM: &str = r#"{"track":"la_canyons-freeroam","players":[{"name":"me","car":"c1","skin":"s"}],
        "sessions":[{"laps":[{"lap":0,"car":0,"time":1,"cuts":0},{"lap":1,"car":0,"time":873548,"cuts":0},
        {"lap":2,"car":0,"time":800000,"cuts":1},{"lap":1,"car":3,"time":500000,"cuts":0},
        {"lap":3,"car":0,"time":869937,"cuts":0}]}]}"#;

    #[test]
    fn only_valid_player_laps_are_read() {
        let rows = parse_cm_laps(CM);
        assert_eq!(rows.iter().map(|r| r.time_ms).collect::<Vec<_>>(), vec![873_548, 869_937]);
        assert!(parse_cm_laps("not json").is_empty());
    }

    #[test]
    fn bests_merge_both_track_spellings() {
        let mut rows = parse_cm_laps(CM);
        rows.push(LapRow { car_id: "c1".into(), track_key: "la_canyons/freeroam".into(), time_ms: 860_000 });
        let bests = personal_bests(&rows);
        assert_eq!(bests.len(), 1);
        assert_eq!((bests[0].time_ms, bests[0].laps), (860_000, 3));
    }
}

/// `2026-W45` as `2 - 8 Nov`. A label that does not parse stays as it is.
pub fn week_range(label: &str) -> String {
    use chrono::{Duration, NaiveDate, Weekday};
    let parsed = label.split_once("-W").and_then(|(year, week)| {
        NaiveDate::from_isoywd_opt(year.parse().ok()?, week.parse().ok()?, Weekday::Mon)
    });
    let Some(monday) = parsed else { return label.to_string() };
    let sunday = monday + Duration::days(6);
    if monday.format("%b").to_string() == sunday.format("%b").to_string() {
        format!("{} - {}", monday.format("%-d"), sunday.format("%-d %b"))
    } else {
        format!("{} - {}", monday.format("%-d %b"), sunday.format("%-d %b"))
    }
}

#[cfg(test)]
mod week_tests {
    use super::*;

    #[test]
    fn weeks_read_as_dates() {
        assert_eq!(week_range("2026-W45"), "2 - 8 Nov");
        assert_eq!(week_range("2026-W44"), "26 Oct - 1 Nov");
        assert_eq!(week_range("nonsense"), "nonsense");
    }
}

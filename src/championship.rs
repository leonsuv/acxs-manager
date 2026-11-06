//! Championship model: a named series of rounds, the points table that scores them,
//! standings, JSON storage and CSV import of one round's results.
//!
//! Every result carries a 1-based `position`. Finishers must hold unique positions
//! within a round. A retirement (`dnf`) keeps its position for display, but it scores
//! nothing, never counts as a win and is not ranked by best position.
//!
//! Position `n` scores `points_table[n - 1]`. Positions past the end of the table
//! score nothing.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// A named championship: its points table and the rounds run so far.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Championship {
    pub name: String,
    /// Points for positions 1, 2, 3 and so on.
    pub points_table: Vec<u32>,
    pub rounds: Vec<Round>,
}

/// One race weekend: the track it ran on and every driver's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Round {
    pub track: String,
    pub layout: Option<String>,
    pub results: Vec<RaceResult>,
}

/// One driver's result in one round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceResult {
    pub driver: String,
    /// 1-based finishing position. Unique among finishers within a round.
    pub position: u32,
    #[serde(default)]
    pub fastest_lap: bool,
    #[serde(default)]
    pub dnf: bool,
}

/// Extra rules that change how a round is scored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Scoring {
    /// Bonus points for the fastest lap, awarded only to a driver who did not retire.
    pub fastest_lap_bonus: u32,
}

/// One driver's line in the standings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub driver: String,
    pub points: u32,
    pub wins: u32,
    pub fastest_laps: u32,
    /// Best finishing position over all rounds. `None` if the driver never finished.
    pub best_position: Option<u32>,
    /// Rounds in which the driver has a result, finished or not.
    pub rounds_entered: u32,
}

#[derive(Debug)]
pub enum ChampionshipError {
    /// The data breaks a rule, such as a duplicate driver or an empty track.
    Invalid(String),
    /// The text is not a championship document.
    Malformed(String),
    /// A line of CSV could not be read. `line` is 1-based.
    Csv { line: usize, message: String },
    /// The file exists but cannot be used. It is left untouched.
    Corrupt { path: PathBuf, message: String },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for ChampionshipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(msg) => write!(f, "invalid championship: {msg}"),
            Self::Malformed(msg) => write!(f, "not a championship file: {msg}"),
            Self::Csv { line, message } => write!(f, "CSV line {line}: {message}"),
            Self::Corrupt { path, message } => write!(
                f,
                "{} is not a usable championship file and was left untouched: {message}",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "I/O error on {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for ChampionshipError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn invalid(msg: impl Into<String>) -> ChampionshipError {
    ChampionshipError::Invalid(msg.into())
}

/// Checks the rules for one round's results. Returns the first problem found.
fn check_results(results: &[RaceResult]) -> Result<(), String> {
    let mut drivers = HashSet::new();
    let mut finishing_positions = HashSet::new();
    let mut fastest_laps = 0usize;
    for result in results {
        if result.driver.trim().is_empty() {
            return Err("a result has an empty driver name".into());
        }
        if !drivers.insert(result.driver.as_str()) {
            return Err(format!("driver \"{}\" appears more than once", result.driver));
        }
        if result.position == 0 {
            return Err(format!("driver \"{}\" has position 0; positions start at 1", result.driver));
        }
        if !result.dnf && !finishing_positions.insert(result.position) {
            return Err(format!("position {} is given to more than one finisher", result.position));
        }
        if result.fastest_lap {
            fastest_laps += 1;
        }
    }
    if fastest_laps > 1 {
        return Err("more than one fastest lap is marked".into());
    }
    Ok(())
}

/// Named scoring tables for `points_preset`, listed from the first position down.
const PRESETS: &[(&str, &[u32])] = &[
    ("10-6-4-3-2-1", &[10, 6, 4, 3, 2, 1]),
    ("25-18-15-12-10-8-6-4-2-1", &[25, 18, 15, 12, 10, 8, 6, 4, 2, 1]),
    ("15-12-10-8-6-4-2-1", &[15, 12, 10, 8, 6, 4, 2, 1]),
];

/// The points table for a named preset, such as `"10-6-4-3-2-1"`. `None` if the name is unknown.
pub fn points_preset(name: &str) -> Option<Vec<u32>> {
    PRESETS.iter().find(|(preset, _)| *preset == name).map(|(_, table)| table.to_vec())
}

/// The names accepted by `points_preset`, in display order.
pub fn preset_names() -> Vec<&'static str> {
    PRESETS.iter().map(|(name, _)| *name).collect()
}

impl Championship {
    pub fn new(name: impl Into<String>, points_table: Vec<u32>) -> Self {
        Self { name: name.into(), points_table, rounds: Vec::new() }
    }

    /// Checks every rule: a non-empty name, a non-empty track for each round, and
    /// the result rules of [`check_results`] for each round.
    pub fn validate(&self) -> Result<(), ChampionshipError> {
        if self.name.trim().is_empty() {
            return Err(invalid("the championship name is empty"));
        }
        for (i, round) in self.rounds.iter().enumerate() {
            let number = i + 1;
            if round.track.trim().is_empty() {
                return Err(invalid(format!("round {number} has no track")));
            }
            check_results(&round.results)
                .map_err(|msg| invalid(format!("round {number} ({}): {msg}", round.track)))?;
        }
        Ok(())
    }

    /// Parses and validates a championship document.
    pub fn from_json(text: &str) -> Result<Self, ChampionshipError> {
        let championship: Championship =
            serde_json::from_str(text).map_err(|e| ChampionshipError::Malformed(e.to_string()))?;
        championship.validate()?;
        Ok(championship)
    }

    /// Serializes the championship as pretty JSON. Invalid data is refused.
    pub fn to_json(&self) -> Result<String, ChampionshipError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|e| ChampionshipError::Malformed(e.to_string()))
    }

    /// Reads and validates the championship at `path`. A file that does not
    /// parse or breaks a rule is reported as [`ChampionshipError::Corrupt`].
    pub fn load(path: &Path) -> Result<Self, ChampionshipError> {
        let text = fs::read_to_string(path).map_err(|source| ChampionshipError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_json(&text).map_err(|e| corrupt(path, e))
    }

    /// Writes the championship to `path` atomically.
    ///
    /// Invalid data is never written. If a file already exists at `path` and it is
    /// not a usable championship, the save is refused and the file is kept as it is.
    pub fn save(&self, path: &Path) -> Result<(), ChampionshipError> {
        let json = self.to_json()?;
        match fs::read_to_string(path) {
            Ok(existing) => {
                if let Err(e) = Self::from_json(&existing) {
                    return Err(corrupt(path, e));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ChampionshipError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
        write_atomic(path, json.as_bytes())
    }
}

/// Turns a load or parse failure into a `Corrupt` error for `path`.
fn corrupt(path: &Path, err: ChampionshipError) -> ChampionshipError {
    let message = match err {
        ChampionshipError::Invalid(msg) | ChampionshipError::Malformed(msg) => msg,
        other => other.to_string(),
    };
    ChampionshipError::Corrupt { path: path.to_path_buf(), message }
}

/// Writes `bytes` to a temp file beside `path`, then renames it over `path`.
/// A failed write leaves the existing file as it was.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ChampionshipError> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let io_err = |source: io::Error| ChampionshipError::Io { path: path.to_path_buf(), source };

    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(format!(".{}.{}.tmp", process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp_path = PathBuf::from(tmp_name);

    let result = (|| -> io::Result<()> {
        let mut file = File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp_path, path)
    })();

    match result {
        Ok(()) => Ok(()),
        Err(source) => {
            let _ = fs::remove_file(&tmp_path);
            Err(io_err(source))
        }
    }
}

/// Computes the standings from every round of `championship`, with no bonus.
/// See [`standings_with`].
pub fn standings(championship: &Championship) -> Vec<Standing> {
    standings_with(championship, Scoring::default())
}

/// Computes the standings from every round of `championship`.
///
/// Rows are sorted by points, then wins, then best position (a driver who never
/// finished sorts last), then driver name. The result depends only on the input.
pub fn standings_with(championship: &Championship, scoring: Scoring) -> Vec<Standing> {
    let mut rows: HashMap<&str, Standing> = HashMap::new();
    for round in &championship.rounds {
        for result in &round.results {
            let row = rows.entry(result.driver.as_str()).or_insert_with(|| Standing {
                driver: result.driver.clone(),
                points: 0,
                wins: 0,
                fastest_laps: 0,
                best_position: None,
                rounds_entered: 0,
            });
            row.rounds_entered += 1;
            if result.dnf {
                continue;
            }

            let table_points = result
                .position
                .checked_sub(1)
                .and_then(|i| championship.points_table.get(i as usize))
                .copied()
                .unwrap_or(0);
            row.points = row.points.saturating_add(table_points);
            if result.position == 1 {
                row.wins += 1;
            }
            row.best_position = Some(match row.best_position {
                Some(best) => best.min(result.position),
                None => result.position,
            });
            if result.fastest_lap {
                row.fastest_laps += 1;
                row.points = row.points.saturating_add(scoring.fastest_lap_bonus);
            }
        }
    }

    let mut out: Vec<Standing> = rows.into_values().collect();
    out.sort_by(|a, b| {
        b.points
            .cmp(&a.points)
            .then(b.wins.cmp(&a.wins))
            .then(a.best_position.is_none().cmp(&b.best_position.is_none()))
            .then(a.best_position.cmp(&b.best_position))
            .then(a.driver.cmp(&b.driver))
    });
    out
}

/// Parses one round's results from CSV text.
///
/// Each row is `driver,position[,fastest_lap[,dnf]]`. A header row whose first
/// field is `driver` is skipped, blank lines are ignored, and fields are split on
/// commas with no quoting. `fastest_lap` and `dnf` accept `true/false`,
/// `yes/no`, `y/n`, `1/0` or `x`, and default to false when left out.
///
/// The returned round has an empty track and no layout. The caller sets them.
pub fn import_results_csv(text: &str) -> Result<Round, ChampionshipError> {
    let mut results = Vec::new();
    let mut first_row = true;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let row = raw.trim();
        if row.is_empty() {
            continue;
        }
        let fields: Vec<&str> = row.split(',').map(str::trim).collect();
        if first_row && fields[0].eq_ignore_ascii_case("driver") {
            first_row = false;
            continue;
        }
        first_row = false;

        let csv_err = |message: String| ChampionshipError::Csv { line, message };
        if !(2..=4).contains(&fields.len()) {
            return Err(csv_err(format!(
                "expected 2 to 4 fields (driver,position,fastest_lap,dnf), found {}",
                fields.len()
            )));
        }
        let driver = fields[0];
        if driver.is_empty() {
            return Err(csv_err("driver name is empty".into()));
        }
        let position = match fields[1].parse::<u32>() {
            Ok(p) if p >= 1 => p,
            _ => {
                return Err(csv_err(format!(
                    "position must be a whole number from 1 upward, got \"{}\"",
                    fields[1]
                )));
            }
        };
        let fastest_lap = parse_flag(fields.get(2).copied()).map_err(csv_err)?;
        let dnf = parse_flag(fields.get(3).copied()).map_err(csv_err)?;

        results.push(RaceResult {
            driver: driver.to_string(),
            position,
            fastest_lap,
            dnf,
        });
    }

    if results.is_empty() {
        return Err(ChampionshipError::Invalid("the CSV has no result rows".into()));
    }
    check_results(&results).map_err(ChampionshipError::Invalid)?;
    Ok(Round { track: String::new(), layout: None, results })
}

/// Reads an optional yes/no column. A missing or empty cell is `false`.
fn parse_flag(cell: Option<&str>) -> Result<bool, String> {
    match cell.unwrap_or("").to_ascii_lowercase().as_str() {
        "" | "0" | "no" | "n" | "false" => Ok(false),
        "1" | "yes" | "y" | "true" | "x" => Ok(true),
        other => Err(format!("expected true/false, yes/no or 1/0, got \"{other}\"")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A directory unique to one test, removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "acxs-championship-{tag}-{}-{}",
                process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).expect("create test dir");
            Self(dir)
        }

        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn result(driver: &str, position: u32) -> RaceResult {
        RaceResult { driver: driver.into(), position, fastest_lap: false, dnf: false }
    }

    fn round(track: &str, results: Vec<RaceResult>) -> Round {
        Round { track: track.into(), layout: None, results }
    }

    fn table() -> Vec<u32> {
        vec![10, 6, 4, 2]
    }

    #[test]
    fn presets_resolve_by_name_and_unknown_names_do_not() {
        let names = preset_names();
        assert_eq!(names, ["10-6-4-3-2-1", "25-18-15-12-10-8-6-4-2-1", "15-12-10-8-6-4-2-1"]);
        assert_eq!(points_preset("10-6-4-3-2-1"), Some(vec![10, 6, 4, 3, 2, 1]));
        assert_eq!(points_preset("25-18-15-12-10-8-6-4-2-1"), Some(vec![25, 18, 15, 12, 10, 8, 6, 4, 2, 1]));
        assert_eq!(points_preset("15-12-10-8-6-4-2-1"), Some(vec![15, 12, 10, 8, 6, 4, 2, 1]));
        assert_eq!(points_preset("no such preset"), None);
    }

    #[test]
    fn preset_tables_are_non_empty_and_descending() {
        for name in preset_names() {
            let table = points_preset(name).expect("every listed preset resolves");
            assert!(!table.is_empty(), "{name} has no points");
            assert!(table.windows(2).all(|w| w[0] > w[1]), "{name} is not descending: {table:?}");
        }
    }

    fn standing<'a>(rows: &'a [Standing], driver: &str) -> &'a Standing {
        rows.iter().find(|s| s.driver == driver).expect("driver in standings")
    }

    #[test]
    fn points_follow_position_and_zero_past_the_table() {
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("Monza", vec![result("A", 1), result("B", 2), result("C", 5)]));
        let rows = standings(&champ);
        assert_eq!(standing(&rows, "A").points, 10);
        assert_eq!(standing(&rows, "B").points, 6);
        assert_eq!(standing(&rows, "C").points, 0);
        assert_eq!(standing(&rows, "A").wins, 1);
        assert_eq!(standing(&rows, "C").best_position, Some(5));
    }

    #[test]
    fn retirement_scores_nothing_and_is_not_a_finish() {
        let mut champ = Championship::new("Cup", table());
        let mut out = result("A", 1);
        out.dnf = true;
        champ.rounds.push(round("Spa", vec![out, result("B", 2)]));
        let rows = standings(&champ);
        let a = standing(&rows, "A");
        assert_eq!((a.points, a.wins, a.best_position, a.rounds_entered), (0, 0, None, 1));
        assert_eq!(rows[0].driver, "B");
    }

    #[test]
    fn totals_add_up_across_rounds() {
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("Monza", vec![result("A", 2), result("B", 1)]));
        champ.rounds.push(round("Spa", vec![result("A", 1), result("B", 3)]));
        let rows = standings(&champ);
        assert_eq!(rows[0].driver, "A");
        assert_eq!(standing(&rows, "A").points, 16);
        assert_eq!(standing(&rows, "B").points, 14);
        assert_eq!(standing(&rows, "A").rounds_entered, 2);
    }

    #[test]
    fn fastest_lap_bonus_is_optional_and_skips_retirements() {
        let mut champ = Championship::new("Cup", table());
        let mut fl = result("A", 2);
        fl.fastest_lap = true;
        let mut retired_fl = result("B", 1);
        retired_fl.fastest_lap = true;
        retired_fl.dnf = true;
        champ.rounds.push(round("Imola", vec![fl, retired_fl]));

        let plain = standings(&champ);
        assert_eq!(standing(&plain, "A").points, 6);
        assert_eq!(standing(&plain, "A").fastest_laps, 1);

        let bonus = standings_with(&champ, Scoring { fastest_lap_bonus: 1 });
        assert_eq!(standing(&bonus, "A").points, 7);
        assert_eq!(standing(&bonus, "B").points, 0);
        assert_eq!(standing(&bonus, "B").fastest_laps, 0);
    }

    #[test]
    fn ties_break_by_points_then_wins_then_best_position_then_name() {
        // Table [10, 6, 4, 2]. Win, Second, Zed and Third all end on 12 points.
        let mut champ = Championship::new("Cup", table());
        // Wins 1, best 1: 10 + 2.
        champ.rounds.push(round("R1", vec![result("Win", 1)]));
        champ.rounds.push(round("R9", vec![result("Win", 4)]));
        // Wins 0, best 2: 6 + 6.
        champ.rounds.push(round("R2", vec![result("Second", 2)]));
        champ.rounds.push(round("R3", vec![result("Second", 2)]));
        // Wins 0, best 2 as well, so the name decides: 6 + 6.
        champ.rounds.push(round("R7", vec![result("Zed", 2)]));
        champ.rounds.push(round("R8", vec![result("Zed", 2)]));
        // Wins 0, best 3: 4 + 4 + 4.
        champ.rounds.push(round("R4", vec![result("Third", 3)]));
        champ.rounds.push(round("R5", vec![result("Third", 3)]));
        champ.rounds.push(round("R6", vec![result("Third", 3)]));
        // Only 2 points.
        champ.rounds.push(round("R10", vec![result("Late", 4)]));

        let order: Vec<String> = standings(&champ).into_iter().map(|s| s.driver).collect();
        assert_eq!(order, ["Win", "Second", "Zed", "Third", "Late"]);
    }

    #[test]
    fn standings_are_deterministic() {
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("R1", vec![result("B", 1), result("A", 2), result("C", 3)]));
        champ.rounds.push(round("R2", vec![result("C", 1), result("A", 2)]));
        assert_eq!(standings(&champ), standings(&champ.clone()));
    }

    #[test]
    fn validate_rejects_duplicate_driver_in_a_round() {
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("Monza", vec![result("A", 1), result("A", 2)]));
        assert!(matches!(champ.validate(), Err(ChampionshipError::Invalid(_))));
    }

    #[test]
    fn validate_rejects_duplicate_finishing_position() {
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("Monza", vec![result("A", 1), result("B", 1)]));
        assert!(matches!(champ.validate(), Err(ChampionshipError::Invalid(_))));
    }

    #[test]
    fn validate_allows_retirements_to_share_a_position() {
        let mut champ = Championship::new("Cup", table());
        let mut a = result("A", 3);
        a.dnf = true;
        let mut b = result("B", 3);
        b.dnf = true;
        champ.rounds.push(round("Monza", vec![result("C", 1), a, b]));
        assert!(champ.validate().is_ok());
    }

    #[test]
    fn validate_rejects_position_zero_empty_names_and_two_fastest_laps() {
        let mut zero = Championship::new("Cup", table());
        zero.rounds.push(round("Monza", vec![result("A", 0)]));
        assert!(zero.validate().is_err());

        let mut no_driver = Championship::new("Cup", table());
        no_driver.rounds.push(round("Monza", vec![result("  ", 1)]));
        assert!(no_driver.validate().is_err());

        let mut no_track = Championship::new("Cup", table());
        no_track.rounds.push(round("", vec![result("A", 1)]));
        assert!(no_track.validate().is_err());

        let no_name = Championship::new(" ", table());
        assert!(no_name.validate().is_err());

        let mut two_fl = Championship::new("Cup", table());
        let mut a = result("A", 1);
        a.fastest_lap = true;
        let mut b = result("B", 2);
        b.fastest_lap = true;
        two_fl.rounds.push(round("Monza", vec![a, b]));
        assert!(two_fl.validate().is_err());
    }

    #[test]
    fn json_round_trip_keeps_the_championship() {
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(Round {
            track: "Monza".into(),
            layout: Some("GP".into()),
            results: vec![result("A", 1)],
        });
        let text = champ.to_json().expect("serialize");
        assert_eq!(Championship::from_json(&text).expect("parse"), champ);
    }

    #[test]
    fn from_json_rejects_unknown_fields_and_invalid_data() {
        let unknown = r#"{"name":"Cup","points_table":[1],"rounds":[],"extra":true}"#;
        assert!(matches!(
            Championship::from_json(unknown),
            Err(ChampionshipError::Malformed(_))
        ));

        let dup = r#"{"name":"Cup","points_table":[1],"rounds":[
            {"track":"Monza","layout":null,"results":[
              {"driver":"A","position":1},{"driver":"A","position":2}]}]}"#;
        assert!(matches!(Championship::from_json(dup), Err(ChampionshipError::Invalid(_))));
    }

    #[test]
    fn save_then_load_round_trips_through_the_disk() {
        let dir = TempDir::new("roundtrip");
        let path = dir.file("cup.json");
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("Spa", vec![result("A", 1), result("B", 2)]));
        champ.save(&path).expect("save");
        assert_eq!(Championship::load(&path).expect("load"), champ);
    }

    #[test]
    fn save_overwrites_a_valid_file_and_leaves_no_temp_file() {
        let dir = TempDir::new("overwrite");
        let path = dir.file("cup.json");
        Championship::new("First", table()).save(&path).expect("first save");
        Championship::new("Second", table()).save(&path).expect("second save");
        assert_eq!(Championship::load(&path).expect("load").name, "Second");
        let entries: Vec<_> = fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(entries.len(), 1, "only the championship file should remain: {entries:?}");
    }

    #[test]
    fn load_refuses_a_corrupt_file_without_changing_it() {
        let dir = TempDir::new("corrupt-load");
        let path = dir.file("cup.json");
        fs::write(&path, b"{ not json").unwrap();
        assert!(matches!(Championship::load(&path), Err(ChampionshipError::Corrupt { .. })));
        assert_eq!(fs::read(&path).unwrap(), b"{ not json");
    }

    #[test]
    fn save_refuses_to_overwrite_a_corrupt_file() {
        let dir = TempDir::new("corrupt-save");
        let path = dir.file("cup.json");
        fs::write(&path, b"garbage").unwrap();
        let result = Championship::new("Cup", table()).save(&path);
        assert!(matches!(result, Err(ChampionshipError::Corrupt { .. })));
        assert_eq!(fs::read(&path).unwrap(), b"garbage");
    }

    #[test]
    fn save_refuses_invalid_data_and_writes_nothing() {
        let dir = TempDir::new("invalid-save");
        let path = dir.file("cup.json");
        let mut champ = Championship::new("Cup", table());
        champ.rounds.push(round("Monza", vec![result("A", 1), result("A", 2)]));
        assert!(champ.save(&path).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn csv_import_reads_header_flags_and_optional_columns() {
        let text = "driver,position,fastest_lap,dnf\n\
                    Alice,1,yes,no\n\
                    \n\
                    Bob,2\n\
                    Cara,3,0,1\n\
                    Dan,4,no,TRUE\n";
        let round = import_results_csv(text).expect("import");
        assert_eq!(round.track, "");
        assert_eq!(round.layout, None);
        assert_eq!(round.results.len(), 4);
        assert!(round.results[0].fastest_lap && !round.results[0].dnf);
        assert!(!round.results[1].fastest_lap && !round.results[1].dnf);
        assert!(round.results[2].dnf && !round.results[2].fastest_lap);
        assert!(round.results[3].dnf);
    }

    #[test]
    fn csv_import_works_without_a_header() {
        let round = import_results_csv("Alice,1\nBob,2").expect("import");
        assert_eq!(round.results.len(), 2);
        assert_eq!(round.results[1].driver, "Bob");
    }

    #[test]
    fn csv_import_reports_the_bad_line() {
        let bad_position = import_results_csv("Alice,1\nBob,second");
        assert!(matches!(bad_position, Err(ChampionshipError::Csv { line: 2, .. })));

        let too_many = import_results_csv("Alice,1,no,no,extra");
        assert!(matches!(too_many, Err(ChampionshipError::Csv { line: 1, .. })));

        let bad_flag = import_results_csv("Alice,1,maybe");
        assert!(matches!(bad_flag, Err(ChampionshipError::Csv { line: 1, .. })));

        let zero = import_results_csv("Alice,0");
        assert!(matches!(zero, Err(ChampionshipError::Csv { line: 1, .. })));
    }

    #[test]
    fn csv_import_rejects_duplicates_and_empty_input() {
        assert!(matches!(
            import_results_csv("Alice,1\nAlice,2"),
            Err(ChampionshipError::Invalid(_))
        ));
        assert!(matches!(
            import_results_csv("Alice,1\nBob,1"),
            Err(ChampionshipError::Invalid(_))
        ));
        assert!(matches!(import_results_csv("driver,position\n\n"), Err(ChampionshipError::Invalid(_))));
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ServerResultFile {
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    track_config: String,
    #[serde(default)]
    result: Vec<ServerResultRow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ServerResultRow {
    #[serde(default)]
    driver_name: String,
    #[serde(default)]
    best_lap: u64,
    #[serde(default)]
    total_time: u64,
}

/// A race from a dedicated server's `results\*.json`, as the page's form takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRace {
    pub track: String,
    pub layout: String,
    /// Rows for [`import_results_csv`].
    pub csv: String,
}

/// `BestLap` of a driver who never set a lap.
const NO_LAP: u64 = 999_999_999;

/// Reads a dedicated server result file. Its `Result` list is in finishing order. Empty grid
/// slots have no driver name; a driver with no total time did not finish.
pub fn import_server_results(json: &str) -> Result<ServerRace, ChampionshipError> {
    let file: ServerResultFile = serde_json::from_str(json)
        .map_err(|e| ChampionshipError::Invalid(format!("not a server result file: {e}")))?;
    let mut seen = HashSet::new();
    let rows: Vec<&ServerResultRow> = file
        .result
        .iter()
        .filter(|row| !row.driver_name.trim().is_empty() && seen.insert(row.driver_name.trim()))
        .collect();
    if rows.is_empty() {
        return Err(ChampionshipError::Invalid("the result file has no drivers".into()));
    }
    let set_lap = |row: &&&ServerResultRow| row.best_lap > 0 && row.best_lap < NO_LAP;
    let fastest = rows.iter().filter(set_lap).map(|row| row.best_lap).min();
    let mut csv = String::from("driver,position,fastest_lap,dnf\n");
    for (index, row) in rows.iter().enumerate() {
        let name = row.driver_name.trim().replace(',', " ");
        let quickest = if set_lap(&row) && Some(row.best_lap) == fastest { "yes" } else { "no" };
        let dnf = if row.total_time == 0 { "yes" } else { "no" };
        csv.push_str(&format!("{name},{},{quickest},{dnf}\n", index + 1));
    }
    Ok(ServerRace { track: file.track_name, layout: file.track_config, csv })
}

#[cfg(test)]
mod server_result_tests {
    use super::*;

    const FILE: &str = r#"{"TrackName":"monza","TrackConfig":"","Type":"RACE","Result":[
        {"DriverName":"Ann, A.","DriverGuid":"1","CarId":0,"CarModel":"c","BestLap":90000,"TotalTime":600000,"BallastKG":0},
        {"DriverName":"Bob","DriverGuid":"2","CarId":1,"CarModel":"c","BestLap":89000,"TotalTime":601000,"BallastKG":0},
        {"DriverName":"Cid","DriverGuid":"3","CarId":2,"CarModel":"c","BestLap":999999999,"TotalTime":0,"BallastKG":0},
        {"DriverName":"","DriverGuid":"","CarId":3,"CarModel":"c","BestLap":999999999,"TotalTime":0,"BallastKG":0}
    ],"Laps":[],"Events":[]}"#;

    #[test]
    fn a_server_result_file_becomes_a_scored_round() {
        let race = import_server_results(FILE).unwrap();
        assert_eq!(race.track, "monza");
        let round = import_results_csv(&race.csv).unwrap();
        assert_eq!(round.results.len(), 3);
        assert_eq!(round.results[0].driver, "Ann  A.");
        assert_eq!(round.results[0].position, 1);
        assert!(round.results[1].fastest_lap && !round.results[0].fastest_lap);
        assert!(round.results[2].dnf && !round.results[2].fastest_lap);
    }

    #[test]
    fn other_json_is_refused() {
        assert!(import_server_results("{}").is_err());
        assert!(import_server_results("not json").is_err());
    }
}

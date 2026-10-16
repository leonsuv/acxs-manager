//! Assetto Corsa quick-race configuration (`cfg/race.ini`).
//!
//! Building and validating a [`QuickRace`] is pure. The only filesystem access
//! is [`write_race_ini`], which takes the AC root explicitly and never deletes
//! anything: an existing `race.ini` is renamed to a timestamped backup first.
//!
//! The generated file mirrors the layout of the race.ini files Assetto Corsa
//! writes for quick races, reduced to what a single-player race with AI
//! opponents needs. Field meanings are documented on [`QuickRace::to_race_ini`].
//! Sections, keys and value formats follow what Content Manager (AcTools,
//! `Game.cs` / `Game.Properties.cs`) writes for a quick race: the player's car in
//! `[RACE] MODEL` with `CAR_0 MODEL=-`, `SUN_ANGLE` from the time of day, and the
//! `[GROOVE]`, `[GHOST_CAR]`, `[WIND]` and inactive `[REPLAY]`/`[RESTART]`/`[BENCHMARK]`
//! sections.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Assetto Corsa allows 40 cars on track: the player plus 39 AI.
const MAX_AI_CARS: u32 = 39;
const MAX_AI_LEVEL: u8 = 100;
const MAX_AI_AGGRESSION: u8 = 100;
const MAX_HOUR: u8 = 23;

/// Session type value for a race in AC's race.ini (`TYPE=3`).
const SESSION_TYPE_RACE: u8 = 3;

/// File name prefix for backups of an overwritten race.ini.
const BACKUP_PREFIX: &str = "race.ini.acxs-backup-";

/// Settings for a single quick race against AI opponents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickRace {
    /// Track folder name, e.g. `"monza"`. Required.
    pub track: String,
    /// Track layout folder, e.g. `"gp"`. `None` selects the track's default layout.
    pub track_layout: Option<String>,
    /// Car model folder name, e.g. `"ks_ferrari_488_gt3"`. Used for the player and all AI cars.
    pub car: String,
    /// Skin folder name for the player's car. May be empty.
    pub skin: String,
    /// Number of AI opponents, 0..=39.
    pub ai_count: u32,
    /// Race length in laps. Must be at least 1.
    pub laps: u32,
    /// Weather type id, e.g. `"3_clear"`.
    pub weather: String,
    /// Hour of day used for lighting, 0..=23.
    pub time_of_day_hour: u8,
    /// AI pace, 0..=100.
    pub ai_level: u8,
    /// Optional AI aggression, 0..=100. `None` leaves the game default.
    pub ai_aggression: Option<u8>,
    /// Name shown for the player.
    pub player_name: String,
    /// Player's starting grid slot, 0..=ai_count.
    pub grid_position: u32,
}

impl Default for QuickRace {
    fn default() -> Self {
        Self {
            track: "monza".to_string(),
            track_layout: None,
            car: "ks_ferrari_488_gt3".to_string(),
            skin: String::new(),
            ai_count: 7,
            laps: 5,
            weather: "3_clear".to_string(),
            time_of_day_hour: 10,
            ai_level: 90,
            ai_aggression: None,
            player_name: "Driver".to_string(),
            grid_position: 0,
        }
    }
}

impl QuickRace {
    /// Checks the race settings. Returns every problem found, each as a
    /// human-readable sentence, or `Ok(())` if the race can be written.
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();

        if self.track.trim().is_empty() {
            problems.push("Track is not set.".to_string());
        }
        if self.car.trim().is_empty() {
            problems.push("Car is not set.".to_string());
        }
        if self.weather.trim().is_empty() {
            problems.push("Weather is not set.".to_string());
        }
        if self.player_name.trim().is_empty() {
            problems.push("Player name is empty.".to_string());
        }
        if self.ai_count > MAX_AI_CARS {
            problems.push(format!(
                "AI car count is {}, but the maximum is {MAX_AI_CARS}.",
                self.ai_count
            ));
        }
        if self.ai_level > MAX_AI_LEVEL {
            problems.push(format!(
                "AI level is {}, but the maximum is {MAX_AI_LEVEL}.",
                self.ai_level
            ));
        }
        if let Some(aggression) = self.ai_aggression
            && aggression > MAX_AI_AGGRESSION
        {
            problems.push(format!(
                "AI aggression is {aggression}, but the maximum is {MAX_AI_AGGRESSION}."
            ));
        }
        if self.laps == 0 {
            problems.push("Race must have at least one lap.".to_string());
        }
        if self.time_of_day_hour > MAX_HOUR {
            problems.push(format!(
                "Time of day is {}, but the hour must be 0 to {MAX_HOUR}.",
                self.time_of_day_hour
            ));
        }
        if self.grid_position > self.ai_count {
            problems.push(format!(
                "Grid position {} is past the last grid slot {} (there are {} AI cars).",
                self.grid_position, self.ai_count, self.ai_count
            ));
        }

        // Values are written one per line, so a line break would corrupt the file.
        let text_fields = [
            ("Track", Some(self.track.as_str())),
            ("Track layout", self.track_layout.as_deref()),
            ("Car", Some(self.car.as_str())),
            ("Skin", Some(self.skin.as_str())),
            ("Weather", Some(self.weather.as_str())),
            ("Player name", Some(self.player_name.as_str())),
        ];
        for (label, value) in text_fields {
            if let Some(value) = value
                && value.contains(['\r', '\n'])
            {
                problems.push(format!("{label} must not contain line breaks."));
            }
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// Renders the race as race.ini text.
    ///
    /// Lines end with CRLF, which is what Assetto Corsa on Windows expects.
    /// Call [`QuickRace::validate`] first; this function does not check input.
    ///
    /// Sections and fields (meanings are the believed ones, not verified in-game):
    ///
    /// - `[RACE]`: `TRACK` is the track folder, `CONFIG_TRACK` the layout folder
    ///   (empty for the default layout), `CARS` the total number of cars on the
    ///   grid including the player, `AI_LEVEL` the default AI pace.
    ///   `FIXED_SETUP=0` allows setup changes, `PENALTIES=1` enables penalties,
    ///   `JUMP_START_PENALTY=0` disables the jump-start penalty.
    /// - `[REMOTE]`: `ACTIVE=0` marks this as an offline (non-multiplayer) race.
    /// - `[CAR_0]`: the player. `MODEL` and `SKIN` are the car and skin folders,
    ///   `DRIVER_NAME` the displayed name, `SETUP` empty for the default setup,
    ///   `GRID_POSITION` the player's starting slot (believed to be zero-based).
    ///   No `AI_LEVEL` for the player.
    /// - `[CAR_1]`..`[CAR_N]`: AI cars. `MODEL` is the same car, `SKIN` empty so
    ///   the game picks one, `AI_LEVEL` the pace for that car, and
    ///   `AI_AGGRESSION` only when set.
    /// - `[SESSION_0]`: one race session. `TYPE=3` is a race, `LAPS` the lap count.
    ///   No practice or qualifying session is written, so the game should go
    ///   straight to the race.
    ///   `STARTING_POSITION` is the player's 1-based grid slot.
    /// - `[WEATHER]`: `NAME` is the weather type id (the game reads `[WEATHER]`,
    ///   not a numbered section).
    /// - `[LIGHTING]`: `SUN_ANGLE` is what the game reads; it is derived from the
    ///   hour (13:00 is 0 degrees, 16 degrees per hour, clamped to the game's
    ///   -80..80 range, i.e. 08:00 to 18:00). `TIME_OF_DAY_HOUR` is kept for
    ///   reference, `TIME_MULT=1` keeps time at normal speed.
    /// - `[HEADER] VERSION=2`, `[TEMPERATURE]`, `[WIND]`, `[DYNAMIC_TRACK]` (the
    ///   "Optimum" preset), `[GROOVE]`, `[GHOST_CAR]`, `[LAP_INVALIDATOR]` and
    ///   `[OPTIONS]` use the values Content Manager writes by default.
    pub fn to_race_ini(&self) -> String {
        let mut out = String::new();

        section(&mut out, "RACE");
        kv(&mut out, "TRACK", &self.track);
        kv(
            &mut out,
            "CONFIG_TRACK",
            self.track_layout.as_deref().unwrap_or(""),
        );
        kv(
            &mut out,
            "CARS",
            &self.ai_count.saturating_add(1).to_string(),
        );
        kv(&mut out, "MODEL", &self.car);
        kv(&mut out, "MODEL_CONFIG", "");
        kv(&mut out, "SKIN", &self.skin);
        kv(&mut out, "DRIFT_MODE", "0");
        kv(&mut out, "RACE_LAPS", &self.laps.to_string());
        kv(&mut out, "AI_LEVEL", &self.ai_level.to_string());
        kv(&mut out, "FIXED_SETUP", "0");
        kv(&mut out, "PENALTIES", "1");
        kv(&mut out, "JUMP_START_PENALTY", "0");

        section(&mut out, "REMOTE");
        kv(&mut out, "ACTIVE", "0");

        // The player's car is named in [RACE] MODEL; CAR_0 MODEL is "-", as the
        // game's own launcher writes it.
        section(&mut out, "CAR_0");
        kv(&mut out, "SETUP", "");
        kv(&mut out, "SKIN", &self.skin);
        kv(&mut out, "MODEL", "-");
        kv(&mut out, "MODEL_CONFIG", "");
        kv(&mut out, "BALLAST", "0");
        kv(&mut out, "RESTRICTOR", "0");
        kv(&mut out, "DRIVER_NAME", &self.player_name);
        kv(&mut out, "GRID_POSITION", &self.grid_position.to_string());

        for index in 1..=self.ai_count {
            section(&mut out, &format!("CAR_{index}"));
            kv(&mut out, "MODEL", &self.car);
            kv(&mut out, "SKIN", "");
            kv(&mut out, "SETUP", "");
            kv(&mut out, "MODEL_CONFIG", "");
            kv(&mut out, "DRIVER_NAME", &format!("AI {index}"));
            kv(&mut out, "BALLAST", "0");
            kv(&mut out, "RESTRICTOR", "0");
            kv(&mut out, "AI_LEVEL", &self.ai_level.to_string());
            if let Some(aggression) = self.ai_aggression {
                kv(&mut out, "AI_AGGRESSION", &aggression.to_string());
            }
        }

        section(&mut out, "SESSION_0");
        kv(&mut out, "NAME", "Quick Race");
        kv(&mut out, "TYPE", &SESSION_TYPE_RACE.to_string());
        kv(&mut out, "LAPS", &self.laps.to_string());
        kv(&mut out, "DURATION_MINUTES", "0");
        kv(&mut out, "SPAWN_SET", "START");
        kv(&mut out, "STARTING_POSITION", &self.grid_position.saturating_add(1).to_string());

        section(&mut out, "WEATHER");
        kv(&mut out, "NAME", &self.weather);

        section(&mut out, "LIGHTING");
        kv(&mut out, "SUN_ANGLE", &format!("{:.2}", f64::from(sun_angle(self.time_of_day_hour))));
        kv(
            &mut out,
            "TIME_OF_DAY_HOUR",
            &self.time_of_day_hour.to_string(),
        );
        kv(&mut out, "TIME_MULT", "1.0");
        kv(&mut out, "CLOUD_SPEED", "0.200");

        section(&mut out, "TEMPERATURE");
        kv(&mut out, "AMBIENT", "26");
        kv(&mut out, "ROAD", "32");

        section(&mut out, "WIND");
        kv(&mut out, "SPEED_KMH_MIN", "0");
        kv(&mut out, "SPEED_KMH_MAX", "0");
        kv(&mut out, "DIRECTION_DEG", "0");

        // The "Optimum" grip preset of the game's launcher.
        section(&mut out, "DYNAMIC_TRACK");
        kv(&mut out, "PRESET", "5");
        kv(&mut out, "SESSION_START", "100");
        kv(&mut out, "RANDOMNESS", "0");
        kv(&mut out, "LAP_GAIN", "1");
        kv(&mut out, "SESSION_TRANSFER", "100");

        section(&mut out, "GROOVE");
        kv(&mut out, "VIRTUAL_LAPS", "10");
        kv(&mut out, "MAX_LAPS", "30");
        kv(&mut out, "STARTING_LAPS", "0");

        section(&mut out, "GHOST_CAR");
        kv(&mut out, "RECORDING", "0");
        kv(&mut out, "PLAYING", "0");
        kv(&mut out, "LOAD", "0");
        kv(&mut out, "FILE", "");
        kv(&mut out, "ENABLED", "0");

        section(&mut out, "LAP_INVALIDATOR");
        kv(&mut out, "ALLOWED_TYRES_OUT", "-1");

        section(&mut out, "OPTIONS");
        kv(&mut out, "USE_MPH", "0");

        for inactive in ["REPLAY", "RESTART", "BENCHMARK"] {
            section(&mut out, inactive);
            kv(&mut out, "ACTIVE", "0");
        }

        section(&mut out, "HEADER");
        kv(&mut out, "VERSION", "2");

        out
    }
}

/// Sun angle the game uses for `hour`: 0 at 13:00, 16 degrees per hour,
/// clamped to -80..=80 (08:00 to 18:00, the range the game accepts).
pub fn sun_angle(hour: u8) -> i32 {
    ((i32::from(hour) - 13) * 16).clamp(-80, 80)
}

/// Starts a section header, with a blank line before it unless it is the first.
fn section(out: &mut String, name: &str) {
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out.push('[');
    out.push_str(name);
    out.push_str("]\r\n");
}

/// Writes one `KEY=value` line.
fn kv(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push('=');
    out.push_str(value);
    out.push_str("\r\n");
}

/// Writes `race` to `cfg_dir/race.ini` and returns the path written.
///
/// The game reads `race.ini` from the user config folder,
/// `<Documents>/Assetto Corsa/cfg` (see [`crate::settings::ac_user_cfg_dir`]),
/// not from the install folder.
///
/// Fails with `InvalidInput` (message: the joined validation problems) if the
/// race is not valid; nothing is touched in that case. Creates `cfg/` if
/// missing. An existing `race.ini` is renamed, not deleted, to
/// `race.ini.acxs-backup-YYYYMMDD-HHMMSS` (with a `-2`, `-3`, ... suffix if
/// that name is taken) before the new file is created.
pub fn write_race_ini(cfg_dir: &Path, race: &QuickRace) -> io::Result<PathBuf> {
    if let Err(problems) = race.validate() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            problems.join(" "),
        ));
    }

    let cfg_dir = cfg_dir.to_path_buf();
    fs::create_dir_all(&cfg_dir)?;

    let target = cfg_dir.join("race.ini");
    if path_exists(&target)? {
        let backup = next_backup_path(&cfg_dir)?;
        fs::rename(&target, &backup)?;
    }

    // create_new so that nothing is ever overwritten silently.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?;
    file.write_all(race.to_race_ini().as_bytes())?;
    Ok(target)
}

/// Returns a backup path in `cfg_dir` that does not exist yet.
fn next_backup_path(cfg_dir: &Path) -> io::Result<PathBuf> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let base = format!("{BACKUP_PREFIX}{stamp}");
    let mut candidate = cfg_dir.join(&base);
    let mut n = 2u32;
    while path_exists(&candidate)? {
        candidate = cfg_dir.join(format!("{base}-{n}"));
        n += 1;
    }
    Ok(candidate)
}

/// Like `Path::exists`, but reports errors other than "not found".
fn path_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory unique to this process and test name.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("acxs-race-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn backups_in(cfg_dir: &Path) -> Vec<PathBuf> {
        let mut found: Vec<PathBuf> = fs::read_dir(cfg_dir)
            .expect("read cfg dir")
            .map(|entry| entry.expect("dir entry").path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(BACKUP_PREFIX))
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn default_is_valid() {
        assert_eq!(QuickRace::default().validate(), Ok(()));
    }

    #[test]
    fn validate_reports_each_problem() {
        let race = QuickRace {
            track: "  ".to_string(),
            car: String::new(),
            ai_count: 40,
            laps: 0,
            time_of_day_hour: 24,
            ai_level: 101,
            player_name: String::new(),
            grid_position: 41,
            ..QuickRace::default()
        };
        let problems = race.validate().expect_err("race should be invalid");
        assert_eq!(problems.len(), 8, "problems: {problems:?}");
        assert!(problems.iter().any(|p| p.contains("Track")));
        assert!(problems.iter().any(|p| p.contains("Car")));
        assert!(problems.iter().any(|p| p.contains("AI car count")));
        assert!(problems.iter().any(|p| p.contains("AI level")));
        assert!(problems.iter().any(|p| p.contains("lap")));
        assert!(problems.iter().any(|p| p.contains("hour")));
        assert!(problems.iter().any(|p| p.contains("Player name")));
        assert!(problems.iter().any(|p| p.contains("Grid position")));
    }

    #[test]
    fn validate_rejects_aggression_and_line_breaks() {
        let race = QuickRace {
            ai_aggression: Some(101),
            skin: "red\nblue".to_string(),
            ..QuickRace::default()
        };
        let problems = race.validate().expect_err("race should be invalid");
        assert_eq!(problems.len(), 2, "problems: {problems:?}");
        assert!(problems.iter().any(|p| p.contains("AI aggression")));
        assert!(problems.iter().any(|p| p.contains("Skin")));
    }

    #[test]
    fn ini_has_player_and_one_section_per_ai_car() {
        let race = QuickRace {
            ai_count: 3,
            ..QuickRace::default()
        };
        let ini = race.to_race_ini();
        assert!(ini.contains("[CAR_0]\r\n"));
        let car_headers = ini.lines().filter(|l| l.starts_with("[CAR_")).count();
        assert_eq!(car_headers, 3 + 1);
        assert!(ini.contains("[CAR_3]\r\n"));
        assert!(!ini.contains("[CAR_4]"));
        assert!(ini.contains("CARS=4\r\n"));
        assert!(ini.contains("GRID_POSITION=0\r\n"));
    }

    #[test]
    fn ini_uses_crlf_only() {
        let ini = QuickRace::default().to_race_ini();
        assert!(ini.ends_with("\r\n"));
        let lf = ini.matches('\n').count();
        let cr = ini.matches('\r').count();
        let crlf = ini.matches("\r\n").count();
        assert!(lf > 0);
        assert_eq!(lf, crlf, "bare LF found");
        assert_eq!(cr, crlf, "bare CR found");
    }

    #[test]
    fn ini_contains_expected_values() {
        let race = QuickRace {
            track_layout: Some("gp".to_string()),
            ai_aggression: Some(40),
            laps: 12,
            weather: "4_mid_clear".to_string(),
            time_of_day_hour: 17,
            ..QuickRace::default()
        };
        let ini = race.to_race_ini();
        assert!(ini.contains("TRACK=monza\r\n"));
        assert!(ini.contains("CONFIG_TRACK=gp\r\n"));
        assert!(ini.contains("[SESSION_0]\r\nNAME=Quick Race\r\nTYPE=3\r\nLAPS=12\r\n"));
        assert!(ini.contains("[CAR_0]\r\nSETUP=\r\nSKIN=\r\nMODEL=-\r\n"));
        assert!(ini.contains("STARTING_POSITION=1\r\n"));
        assert!(ini.contains("[WEATHER]\r\nNAME=4_mid_clear\r\n"));
        assert!(ini.contains("SUN_ANGLE=64.00\r\nTIME_OF_DAY_HOUR=17\r\nTIME_MULT=1.0\r\n"));
        assert!(ini.contains("RACE_LAPS=12\r\n"));
        assert!(ini.contains("DRIVER_NAME=Driver\r\n"));
        // AI aggression goes on AI cars only, never on the player.
        let player = ini.split("[CAR_1]").next().expect("player section");
        assert!(!player.contains("AI_AGGRESSION"));
        assert_eq!(ini.matches("AI_AGGRESSION=40\r\n").count(), 7);
    }

    #[test]
    fn no_aggression_key_when_unset() {
        let ini = QuickRace::default().to_race_ini();
        assert!(!ini.contains("AI_AGGRESSION"));
    }

    #[test]
    fn write_creates_file_and_backs_up_previous() {
        let root = scratch_dir("write_creates_and_backs_up");
        let cfg_dir = root.join("cfg");

        let first = QuickRace::default();
        let path = write_race_ini(&root.join("cfg"), &first).expect("first write");
        assert_eq!(path, cfg_dir.join("race.ini"));
        let first_text = fs::read_to_string(&path).expect("read first");
        assert_eq!(first_text, first.to_race_ini());
        assert!(backups_in(&cfg_dir).is_empty());

        let second = QuickRace {
            laps: 7,
            ..QuickRace::default()
        };
        write_race_ini(&root.join("cfg"), &second).expect("second write");
        let backups = backups_in(&cfg_dir);
        assert_eq!(backups.len(), 1, "backups: {backups:?}");
        assert_eq!(
            fs::read_to_string(&backups[0]).expect("read backup"),
            first_text
        );
        assert_eq!(
            fs::read_to_string(cfg_dir.join("race.ini")).expect("read new"),
            second.to_race_ini()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_race_writes_nothing() {
        let root = scratch_dir("invalid_race_writes_nothing");
        let race = QuickRace {
            track: String::new(),
            ..QuickRace::default()
        };
        let err = write_race_ini(&root.join("cfg"), &race).expect_err("invalid race must fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("Track"));
        assert!(!root.join("cfg").exists());

        let _ = fs::remove_dir_all(&root);
    }
}

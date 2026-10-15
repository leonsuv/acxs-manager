//! Named race presets: saved quick-race setups kept in `acxs-presets.json`
//! under the Assetto Corsa root.
//!
//! The store never deletes data. Replacing a preset keeps the old value in
//! `history`, and removing a preset moves it to `removed`. Both lists are
//! written to the file, so every value ever saved stays recoverable.
//!
//! Names are trimmed, must be non-empty and at most 64 characters, and are
//! matched case-insensitively. A file that cannot be parsed, has duplicate
//! names, or was written by a newer format version is reported as an error
//! by [`PresetStore::load`]; it is never overwritten.
//!
//! [`QuickRace`] does not implement serde, so the file uses the private
//! `*Record` types below and converts to and from them.

use crate::race::QuickRace;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "acxs-presets.json";
const FORMAT_VERSION: u32 = 1;
const MAX_NAME_CHARS: usize = 64;

/// A named quick-race setup.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub race: QuickRace,
}

/// All presets, plus the values they replaced and the ones they were removed from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetStore {
    presets: Vec<Preset>,
    history: Vec<Preset>,
    removed: Vec<Preset>,
}

impl PresetStore {
    /// An empty store with nothing saved yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the store from `path`. A missing file gives an empty store.
    /// Any other problem (unreadable, not valid JSON, newer format version,
    /// invalid or duplicate names) is an error and the file is left alone.
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(err) => return Err(with_path(err, path)),
        };

        let file: FileRecord = serde_json::from_str(&text)
            .map_err(|err| invalid_data(path, &format!("not a valid preset file: {err}")))?;
        if file.version > FORMAT_VERSION {
            return Err(invalid_data(
                path,
                &format!(
                    "written by a newer version (format {}, this program reads {FORMAT_VERSION})",
                    file.version
                ),
            ));
        }

        let mut store = Self::new();
        for record in file.presets {
            let preset = preset_from_record(record).map_err(|msg| invalid_data(path, &msg))?;
            if store.index_of(&preset.name).is_some() {
                return Err(invalid_data(
                    path,
                    &format!("preset \"{}\" appears more than once", preset.name),
                ));
            }
            store.presets.push(preset);
        }
        store.history = file
            .history
            .into_iter()
            .map(|record| preset_from_record(record).map_err(|msg| invalid_data(path, &msg)))
            .collect::<io::Result<_>>()?;
        store.removed = file
            .removed
            .into_iter()
            .map(|record| preset_from_record(record).map_err(|msg| invalid_data(path, &msg)))
            .collect::<io::Result<_>>()?;
        store.sort();
        Ok(store)
    }

    /// Writes the store to `path` atomically: the JSON goes to a temporary file
    /// next to `path`, which is then renamed over it. On failure the previous
    /// file is unchanged.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let file = FileRecord {
            version: FORMAT_VERSION,
            presets: self.presets.iter().map(PresetRecord::from).collect(),
            history: self.history.iter().map(PresetRecord::from).collect(),
            removed: self.removed.iter().map(PresetRecord::from).collect(),
        };
        let mut text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
        text.push('\n');

        let tmp = temp_path(path);
        let result =
            write_synced(&tmp, text.as_bytes()).and_then(|()| fs::rename(&tmp, path));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// All live presets, sorted by name case-insensitively.
    pub fn list(&self) -> &[Preset] {
        &self.presets
    }

    /// The live preset with this name (case-insensitive, surrounding spaces ignored).
    pub fn get(&self, name: &str) -> Option<&Preset> {
        self.index_of(name).map(|index| &self.presets[index])
    }

    /// Old values replaced by [`PresetStore::upsert`], oldest first.
    pub fn history(&self) -> &[Preset] {
        &self.history
    }

    /// Presets moved out by [`PresetStore::remove`], oldest first.
    pub fn removed(&self) -> &[Preset] {
        &self.removed
    }

    /// Adds a preset, or replaces the one with the same name. When replacing,
    /// the old preset is kept in [`PresetStore::history`]. The name is stored
    /// trimmed and with the spelling given in this call.
    ///
    /// Fails with `InvalidInput` if the name is empty or longer than 64
    /// characters after trimming. Nothing changes in that case.
    pub fn upsert(&mut self, name: &str, race: QuickRace) -> io::Result<()> {
        let name = normalize_name(name)?;
        match self.index_of(&name) {
            Some(index) => {
                let old = std::mem::replace(&mut self.presets[index], Preset { name, race });
                self.history.push(old);
            }
            None => self.presets.push(Preset { name, race }),
        }
        self.sort();
        Ok(())
    }

    /// Moves the preset with this name into [`PresetStore::removed`].
    /// Returns `false` if there was no such preset.
    pub fn remove(&mut self, name: &str) -> bool {
        match self.index_of(name) {
            Some(index) => {
                let preset = self.presets.remove(index);
                self.removed.push(preset);
                true
            }
            None => false,
        }
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        let key = name.trim().to_lowercase();
        self.presets
            .iter()
            .position(|preset| preset.name.to_lowercase() == key)
    }

    fn sort(&mut self) {
        self.presets.sort_by_key(|preset| preset.name.to_lowercase());
    }
}

/// The preset file for an Assetto Corsa install: `<ac_root>/acxs-presets.json`.
pub fn default_path(ac_root: &Path) -> PathBuf {
    ac_root.join(FILE_NAME)
}

/// Trims a name and checks its length. Returns the trimmed name.
fn normalize_name(name: &str) -> io::Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Preset name must not be empty.",
        ));
    }
    let count = trimmed.chars().count();
    if count > MAX_NAME_CHARS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Preset name is {count} characters, but the maximum is {MAX_NAME_CHARS}."),
        ));
    }
    Ok(trimmed.to_string())
}

fn preset_from_record(record: PresetRecord) -> Result<Preset, String> {
    let name = normalize_name(&record.name).map_err(|err| err.to_string())?;
    Ok(Preset {
        name,
        race: record.race.into(),
    })
}

fn invalid_data(path: &Path, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}: {detail}", path.display()),
    )
}

fn with_path(err: io::Error, path: &Path) -> io::Error {
    io::Error::new(err.kind(), format!("{}: {err}", path.display()))
}

/// A temporary name next to `path`, unique to this process.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| std::ffi::OsString::from(FILE_NAME), |n| n.to_os_string());
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[derive(Serialize, Deserialize)]
struct FileRecord {
    version: u32,
    presets: Vec<PresetRecord>,
    #[serde(default)]
    history: Vec<PresetRecord>,
    #[serde(default)]
    removed: Vec<PresetRecord>,
}

#[derive(Serialize, Deserialize)]
struct PresetRecord {
    name: String,
    race: RaceRecord,
}

/// Serde mirror of [`QuickRace`], field for field.
#[derive(Serialize, Deserialize)]
struct RaceRecord {
    track: String,
    track_layout: Option<String>,
    car: String,
    skin: String,
    ai_count: u32,
    laps: u32,
    weather: String,
    time_of_day_hour: u8,
    ai_level: u8,
    ai_aggression: Option<u8>,
    player_name: String,
    grid_position: u32,
}

impl From<&Preset> for PresetRecord {
    fn from(preset: &Preset) -> Self {
        Self {
            name: preset.name.clone(),
            race: RaceRecord::from(&preset.race),
        }
    }
}

impl From<&QuickRace> for RaceRecord {
    fn from(race: &QuickRace) -> Self {
        Self {
            track: race.track.clone(),
            track_layout: race.track_layout.clone(),
            car: race.car.clone(),
            skin: race.skin.clone(),
            ai_count: race.ai_count,
            laps: race.laps,
            weather: race.weather.clone(),
            time_of_day_hour: race.time_of_day_hour,
            ai_level: race.ai_level,
            ai_aggression: race.ai_aggression,
            player_name: race.player_name.clone(),
            grid_position: race.grid_position,
        }
    }
}

impl From<RaceRecord> for QuickRace {
    fn from(record: RaceRecord) -> Self {
        Self {
            track: record.track,
            track_layout: record.track_layout,
            car: record.car,
            skin: record.skin,
            ai_count: record.ai_count,
            laps: record.laps,
            weather: record.weather,
            time_of_day_hour: record.time_of_day_hour,
            ai_level: record.ai_level,
            ai_aggression: record.ai_aggression,
            player_name: record.player_name,
            grid_position: record.grid_position,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory unique to this process and test name.
    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("acxs-presets-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn race_with_laps(laps: u32) -> QuickRace {
        QuickRace {
            laps,
            ..QuickRace::default()
        }
    }

    fn names(store: &PresetStore) -> Vec<&str> {
        store.list().iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = scratch_dir("round_trip");
        let path = default_path(&dir);
        assert_eq!(path, dir.join("acxs-presets.json"));

        let spa = QuickRace {
            track: "spa".to_string(),
            track_layout: Some("gp".to_string()),
            skin: "red".to_string(),
            ai_aggression: Some(40),
            grid_position: 3,
            weather: "4_mid_clear".to_string(),
            ..QuickRace::default()
        };
        let mut store = PresetStore::new();
        store.upsert("Spa Sprint", spa.clone()).expect("upsert spa");
        store
            .upsert("  monza  ", race_with_laps(5))
            .expect("upsert monza");
        store.save(&path).expect("save");

        // Only the preset file remains: the temporary file was renamed away.
        assert_eq!(fs::read_dir(&dir).expect("read dir").count(), 1);

        let loaded = PresetStore::load(&path).expect("load");
        assert_eq!(loaded, store);
        assert_eq!(names(&loaded), ["monza", "Spa Sprint"]);
        assert_eq!(loaded.get("SPA SPRINT").expect("spa").race, spa);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = scratch_dir("missing");
        let store = PresetStore::load(&default_path(&dir)).expect("load missing");
        assert!(store.list().is_empty());
        assert!(store.history().is_empty());
        assert!(store.removed().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_replaces_and_keeps_history() {
        let dir = scratch_dir("upsert_history");
        let path = default_path(&dir);

        let mut store = PresetStore::new();
        store.upsert("Sprint", race_with_laps(5)).expect("first");
        store.upsert("Sprint", race_with_laps(8)).expect("second");
        store.upsert("sprint", race_with_laps(10)).expect("third");

        assert_eq!(store.list().len(), 1);
        assert_eq!(store.get("Sprint").expect("sprint").race.laps, 10);
        let old_laps: Vec<u32> = store.history().iter().map(|p| p.race.laps).collect();
        assert_eq!(old_laps, [5, 8]);
        assert_eq!(store.history()[0].name, "Sprint");

        store.save(&path).expect("save");
        let loaded = PresetStore::load(&path).expect("load");
        assert_eq!(loaded.history(), store.history());
        assert_eq!(loaded.get("Sprint").expect("sprint").race.laps, 10);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_keeps_data_in_removed_list() {
        let dir = scratch_dir("remove_keeps");
        let path = default_path(&dir);

        let mut store = PresetStore::new();
        store.upsert("A", race_with_laps(3)).expect("upsert a");
        store.upsert("B", race_with_laps(4)).expect("upsert b");

        assert!(store.remove("a"));
        assert!(!store.remove("a"), "second remove finds nothing");
        assert!(store.get("A").is_none());
        assert_eq!(names(&store), ["B"]);
        assert_eq!(store.removed().len(), 1);
        assert_eq!(store.removed()[0].name, "A");
        assert_eq!(store.removed()[0].race.laps, 3);

        store.save(&path).expect("save");
        let loaded = PresetStore::load(&path).expect("load");
        assert_eq!(loaded.removed(), store.removed());
        assert_eq!(names(&loaded), ["B"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_names_are_rejected() {
        let mut store = PresetStore::new();
        let bad = vec![
            String::new(),
            "   ".to_string(),
            "x".repeat(65),
            format!("  {}  ", "x".repeat(65)),
        ];
        for name in &bad {
            let err = store
                .upsert(name, QuickRace::default())
                .expect_err("name should be rejected");
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "name {name:?}");
        }
        assert!(store.list().is_empty());

        // Exactly 64 characters is fine, counted in characters not bytes.
        store
            .upsert(&"x".repeat(64), QuickRace::default())
            .expect("64 ascii chars");
        store
            .upsert(&format!("  {}  ", "é".repeat(64)), QuickRace::default())
            .expect("64 multi-byte chars after trim");
        assert_eq!(store.list().len(), 2);
    }

    #[test]
    fn corrupt_file_is_error_and_untouched() {
        let dir = scratch_dir("corrupt");
        let path = default_path(&dir);
        let garbage = "{ this is not json";
        fs::write(&path, garbage).expect("write corrupt file");

        let err = PresetStore::load(&path).expect_err("corrupt file must fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(&path).expect("read back"), garbage);
        assert_eq!(fs::read_dir(&dir).expect("read dir").count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn newer_version_is_rejected_and_untouched() {
        let dir = scratch_dir("newer_version");
        let path = default_path(&dir);
        let text = r#"{"version": 99, "presets": []}"#;
        fs::write(&path, text).expect("write newer file");

        let err = PresetStore::load(&path).expect_err("newer format must fail");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(&path).expect("read back"), text);

        let _ = fs::remove_dir_all(&dir);
    }
}

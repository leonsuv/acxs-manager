//! Local store of lap records: best lap per car per track (and layout), with full history.
//!
//! Each record holds the track and optional layout, the car, the driver, the lap time,
//! optional sector times and the ISO-8601 time the lap was recorded. Sector times feed
//! [`best_sectors`] (best time per sector) and [`theoretical_best`] (best lap made of
//! the best sectors).
//!
//! Records are never removed. `add` appends, and "best" values are computed from the
//! full list, so a replaced best lap stays in `history`.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// Longest lap time accepted by [`LapStore::add`]: one hour.
pub const MAX_LAP_TIME_MS: u32 = 3_600_000;

/// One completed lap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LapRecord {
    pub track: String,
    pub layout: Option<String>,
    pub car: String,
    pub driver: String,
    pub lap_time_ms: u32,
    pub sectors_ms: Option<Vec<u32>>,
    /// ISO-8601 timestamp, e.g. `2026-10-15T23:29:00.000Z`.
    pub recorded_at: String,
}

/// All lap records, in the order they were added (oldest first).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LapStore {
    records: Vec<LapRecord>,
}

#[derive(Debug)]
pub enum LapStoreError {
    /// Lap time was zero or above [`MAX_LAP_TIME_MS`].
    InvalidLapTime { lap_time_ms: u32 },
    /// The file exists but is not a valid lap store. It is never overwritten.
    Corrupt {
        path: PathBuf,
        source: serde_json::Error,
    },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for LapStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLapTime { lap_time_ms } => write!(
                f,
                "invalid lap time {lap_time_ms} ms: must be above 0 and at most {MAX_LAP_TIME_MS} ms"
            ),
            Self::Corrupt { path, source } => write!(
                f,
                "lap store {} is corrupt and was left untouched: {source}",
                path.display()
            ),
            Self::Io { path, source } => {
                write!(f, "lap store I/O error on {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LapStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidLapTime { .. } => None,
            Self::Corrupt { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
        }
    }
}

/// Current UTC time as ISO-8601 with millisecond precision, for `LapRecord::recorded_at`.
pub fn now_recorded_at() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Format a lap time as `M:SS.mmm`, e.g. `1:23.456`.
pub fn format_lap(ms: u32) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

/// Track and layout match. Track ids are compared ignoring ASCII case.
/// A missing layout only matches a missing layout.
fn same_track(rec: &LapRecord, track: &str, layout: Option<&str>) -> bool {
    rec.track.eq_ignore_ascii_case(track)
        && match (rec.layout.as_deref(), layout) {
            (None, None) => true,
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
}

impl LapStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// All records, oldest first.
    pub fn records(&self) -> &[LapRecord] {
        &self.records
    }

    /// Read the store at `path`. A missing file gives an empty store.
    /// A file that exists but does not parse is an error and is not modified.
    pub fn load(path: &Path) -> Result<Self, LapStoreError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(source) => {
                return Err(LapStoreError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        serde_json::from_slice(&bytes).map_err(|source| LapStoreError::Corrupt {
            path: path.to_path_buf(),
            source,
        })
    }

    /// Write the store to `path` atomically: the JSON goes to a temp file next to
    /// the target, which is then renamed over it. A failed write leaves the old file intact.
    pub fn save(&self, path: &Path) -> Result<(), LapStoreError> {
        let io_err = |source: io::Error| LapStoreError::Io {
            path: path.to_path_buf(),
            source,
        };

        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| io_err(io::Error::new(io::ErrorKind::InvalidData, e)))?;

        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).map_err(io_err)?;
        }

        let mut tmp_os = path.as_os_str().to_owned();
        tmp_os.push(format!(".{}.tmp", std::process::id()));
        let tmp_path = PathBuf::from(tmp_os);

        let result = (|| -> io::Result<()> {
            let mut file = fs::File::create(&tmp_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&tmp_path, path)
        })();

        if result.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }
        result.map_err(io_err)
    }

    /// Append a record. Lap times of 0 or above one hour are rejected.
    pub fn add(&mut self, record: LapRecord) -> Result<(), LapStoreError> {
        if record.lap_time_ms == 0 || record.lap_time_ms > MAX_LAP_TIME_MS {
            return Err(LapStoreError::InvalidLapTime {
                lap_time_ms: record.lap_time_ms,
            });
        }
        self.records.push(record);
        Ok(())
    }

    /// Fastest record for this car on this track and layout.
    /// If two laps tie, the one added first wins.
    pub fn best_for(&self, track: &str, layout: Option<&str>, car: &str) -> Option<&LapRecord> {
        self.records
            .iter()
            .filter(|r| same_track(r, track, layout) && r.car.eq_ignore_ascii_case(car))
            .min_by_key(|r| r.lap_time_ms)
    }

    /// Best lap of each car on this track and layout, fastest first, at most `limit` entries.
    /// Cars with equal best times keep the order in which they first appeared.
    pub fn leaderboard(&self, track: &str, layout: Option<&str>, limit: usize) -> Vec<&LapRecord> {
        let mut best: Vec<&LapRecord> = Vec::new();
        for rec in self.records.iter().filter(|r| same_track(r, track, layout)) {
            match best.iter_mut().find(|b| b.car.eq_ignore_ascii_case(&rec.car)) {
                Some(slot) => {
                    if rec.lap_time_ms < slot.lap_time_ms {
                        *slot = rec;
                    }
                }
                None => best.push(rec),
            }
        }
        best.sort_by_key(|r| r.lap_time_ms);
        best.truncate(limit);
        best
    }

    /// Every record for this car on this track and layout, in the order they were added.
    pub fn history(&self, track: &str, layout: Option<&str>, car: &str) -> Vec<&LapRecord> {
        self.records
            .iter()
            .filter(|r| same_track(r, track, layout) && r.car.eq_ignore_ascii_case(car))
            .collect()
    }
}

/// Best time per sector for one car on one track and layout, taken from the sector
/// splits of every matching lap. Laps without splits, laps with an empty or zero
/// split, and laps whose split count differs from the first usable split are skipped.
/// Returns `None` when no lap has a usable split.
pub fn best_sectors(
    records: &[LapRecord],
    track: &str,
    layout: Option<&str>,
    car: &str,
) -> Option<Vec<u32>> {
    let splits: Vec<&[u32]> = records
        .iter()
        .filter(|r| same_track(r, track, layout) && r.car.eq_ignore_ascii_case(car))
        .filter_map(|r| r.sectors_ms.as_deref())
        .filter(|s| !s.is_empty() && s.iter().all(|&ms| ms > 0))
        .collect();

    let count = splits.first()?.len();
    let mut best: Vec<u32> = splits[0].to_vec();
    for split in splits.iter().filter(|s| s.len() == count).skip(1) {
        for (slot, &ms) in best.iter_mut().zip(split.iter()) {
            *slot = (*slot).min(ms);
        }
    }
    Some(best)
}

/// Theoretical best lap for one car on one track and layout: the sum of the best
/// sector times from [`best_sectors`]. Returns `None` when there is no usable split
/// or the sum does not fit in a `u32`.
pub fn theoretical_best(
    records: &[LapRecord],
    track: &str,
    layout: Option<&str>,
    car: &str,
) -> Option<u32> {
    best_sectors(records, track, layout, car)?
        .into_iter()
        .try_fold(0u32, |acc, ms| acc.checked_add(ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique directory under the system temp dir, removed on drop (even if a test panics).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "acxs-laptimes-{tag}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
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

    fn rec(
        track: &str,
        layout: Option<&str>,
        car: &str,
        driver: &str,
        lap_time_ms: u32,
        recorded_at: &str,
    ) -> LapRecord {
        LapRecord {
            track: track.to_string(),
            layout: layout.map(str::to_string),
            car: car.to_string(),
            driver: driver.to_string(),
            lap_time_ms,
            sectors_ms: None,
            recorded_at: recorded_at.to_string(),
        }
    }

    #[test]
    fn add_rejects_zero_and_over_one_hour() {
        let mut store = LapStore::new();
        assert!(matches!(
            store.add(rec("monza", None, "ks_ferrari", "a", 0, "t")),
            Err(LapStoreError::InvalidLapTime { lap_time_ms: 0 })
        ));
        assert!(matches!(
            store.add(rec("monza", None, "ks_ferrari", "a", 3_600_001, "t")),
            Err(LapStoreError::InvalidLapTime { lap_time_ms: 3_600_001 })
        ));
        assert!(store.add(rec("monza", None, "ks_ferrari", "a", 3_600_000, "t")).is_ok());
        assert!(store.add(rec("monza", None, "ks_ferrari", "a", 1, "t")).is_ok());
        assert_eq!(store.records().len(), 2);
    }

    #[test]
    fn best_for_matches_track_layout_and_car() {
        let mut store = LapStore::new();
        store.add(rec("monza", None, "ks_ferrari", "a", 83_000, "t1")).unwrap();
        store.add(rec("monza", None, "ks_ferrari", "a", 82_500, "t2")).unwrap();
        store.add(rec("monza", None, "ks_bmw", "b", 80_000, "t3")).unwrap();
        // Same track, different layout: must not count.
        store.add(rec("monza", Some("gp"), "ks_ferrari", "a", 70_000, "t4")).unwrap();
        // Different track.
        store.add(rec("spa", None, "ks_ferrari", "a", 60_000, "t5")).unwrap();

        let best = store.best_for("monza", None, "ks_ferrari").unwrap();
        assert_eq!(best.lap_time_ms, 82_500);
        assert_eq!(best.recorded_at, "t2");

        // Car and track comparison ignores ASCII case.
        assert_eq!(store.best_for("MONZA", None, "KS_Ferrari").unwrap().lap_time_ms, 82_500);

        let gp = store.best_for("monza", Some("gp"), "ks_ferrari").unwrap();
        assert_eq!(gp.lap_time_ms, 70_000);

        assert!(store.best_for("monza", None, "ks_audi").is_none());
        assert!(store.best_for("nurburgring", None, "ks_ferrari").is_none());
    }

    #[test]
    fn leaderboard_is_best_per_car_sorted_and_limited() {
        let mut store = LapStore::new();
        store.add(rec("monza", None, "ks_ferrari", "a", 84_000, "t1")).unwrap();
        store.add(rec("monza", None, "ks_bmw", "b", 83_000, "t2")).unwrap();
        store.add(rec("monza", None, "ks_ferrari", "a", 82_000, "t3")).unwrap();
        store.add(rec("monza", None, "ks_audi", "c", 85_000, "t4")).unwrap();
        store.add(rec("monza", Some("gp"), "ks_audi", "c", 60_000, "t5")).unwrap();

        let board = store.leaderboard("monza", None, 10);
        let got: Vec<(&str, u32)> = board.iter().map(|r| (r.car.as_str(), r.lap_time_ms)).collect();
        assert_eq!(
            got,
            vec![("ks_ferrari", 82_000), ("ks_bmw", 83_000), ("ks_audi", 85_000)]
        );

        let top2 = store.leaderboard("monza", None, 2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].car, "ks_ferrari");
        assert_eq!(top2[1].car, "ks_bmw");

        assert!(store.leaderboard("monza", None, 0).is_empty());
        assert!(store.leaderboard("spa", None, 10).is_empty());
    }

    #[test]
    fn history_keeps_every_lap_in_add_order() {
        let mut store = LapStore::new();
        store.add(rec("monza", None, "ks_ferrari", "a", 84_000, "t1")).unwrap();
        store.add(rec("monza", None, "ks_bmw", "b", 83_000, "t2")).unwrap();
        store.add(rec("monza", None, "ks_ferrari", "a", 82_000, "t3")).unwrap();
        store.add(rec("monza", None, "ks_ferrari", "a", 82_500, "t4")).unwrap();

        let hist = store.history("monza", None, "KS_FERRARI");
        let times: Vec<u32> = hist.iter().map(|r| r.lap_time_ms).collect();
        assert_eq!(times, vec![84_000, 82_000, 82_500]);

        // Replacing the best lap does not remove the old ones.
        assert_eq!(store.best_for("monza", None, "ks_ferrari").unwrap().lap_time_ms, 82_000);
        assert_eq!(store.records().len(), 4);
        assert!(store.history("monza", Some("gp"), "ks_ferrari").is_empty());
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = TempDir::new("roundtrip");
        let path = dir.path().join("laptimes.json");

        let mut store = LapStore::new();
        let mut with_sectors = rec("monza", Some("gp"), "ks_ferrari", "Leon", 82_456, "2026-10-15T20:00:00.000Z");
        with_sectors.sectors_ms = Some(vec![27_100, 28_900, 26_456]);
        store.add(with_sectors).unwrap();
        store.add(rec("spa", None, "ks_bmw", "Leon", 120_001, "2026-10-15T21:00:00.000Z")).unwrap();

        store.save(&path).unwrap();
        let loaded = LapStore::load(&path).unwrap();
        assert_eq!(loaded, store);

        // Only the target file remains, no temp file.
        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["laptimes.json".to_string()]);
    }

    #[test]
    fn load_missing_file_gives_empty_store() {
        let dir = TempDir::new("missing");
        let store = LapStore::load(&dir.path().join("nope.json")).unwrap();
        assert!(store.records().is_empty());
    }

    #[test]
    fn corrupt_file_is_error_and_left_untouched() {
        let dir = TempDir::new("corrupt");
        let path = dir.path().join("laptimes.json");
        let garbage = b"{\"records\": [ this is not json";
        fs::write(&path, garbage).unwrap();

        let err = LapStore::load(&path).unwrap_err();
        assert!(matches!(err, LapStoreError::Corrupt { .. }));

        assert_eq!(fs::read(&path).unwrap(), garbage);
    }

    #[test]
    fn format_lap_cases() {
        assert_eq!(format_lap(0), "0:00.000");
        assert_eq!(format_lap(83_456), "1:23.456");
        assert_eq!(format_lap(81_001), "1:21.001");
        assert_eq!(format_lap(60_000), "1:00.000");
        assert_eq!(format_lap(3_599_999), "59:59.999");
    }

    fn with_sectors(mut record: LapRecord, sectors: &[u32]) -> LapRecord {
        record.sectors_ms = Some(sectors.to_vec());
        record
    }

    #[test]
    fn best_sectors_takes_fastest_split_per_sector() {
        let records = vec![
            with_sectors(rec("monza", None, "ks_ferrari", "a", 82_000, "t1"), &[27_000, 29_000, 26_000]),
            with_sectors(rec("monza", None, "ks_ferrari", "a", 81_500, "t2"), &[27_500, 28_000, 26_000]),
            // No splits: ignored.
            rec("monza", None, "ks_ferrari", "a", 70_000, "t3"),
        ];
        assert_eq!(
            best_sectors(&records, "monza", None, "ks_ferrari"),
            Some(vec![27_000, 28_000, 26_000])
        );
    }

    #[test]
    fn theoretical_best_is_sum_of_best_sectors() {
        let records = vec![
            with_sectors(rec("monza", None, "ks_ferrari", "a", 82_000, "t1"), &[27_000, 29_000, 26_000]),
            with_sectors(rec("monza", None, "ks_ferrari", "a", 81_500, "t2"), &[27_500, 28_000, 26_000]),
        ];
        assert_eq!(theoretical_best(&records, "monza", None, "ks_ferrari"), Some(81_000));

        // The theoretical lap is never slower than the fastest real lap.
        let fastest_lap = records.iter().map(|r| r.lap_time_ms).min().unwrap();
        assert!(theoretical_best(&records, "monza", None, "ks_ferrari").unwrap() <= fastest_lap);
    }

    #[test]
    fn sector_bests_respect_track_layout_and_car() {
        let records = vec![
            with_sectors(rec("monza", None, "ks_ferrari", "a", 80_000, "t1"), &[25_000, 28_000, 27_000]),
            with_sectors(rec("monza", Some("gp"), "ks_ferrari", "a", 70_000, "t2"), &[20_000, 25_000, 25_000]),
            with_sectors(rec("spa", None, "ks_ferrari", "a", 60_000, "t3"), &[10_000, 25_000, 25_000]),
            with_sectors(rec("monza", None, "ks_bmw", "b", 60_000, "t4"), &[15_000, 22_000, 23_000]),
        ];
        assert_eq!(
            best_sectors(&records, "monza", None, "ks_ferrari"),
            Some(vec![25_000, 28_000, 27_000])
        );
        assert_eq!(
            best_sectors(&records, "MONZA", Some("GP"), "KS_Ferrari"),
            Some(vec![20_000, 25_000, 25_000])
        );
        assert_eq!(theoretical_best(&records, "monza", None, "ks_ferrari"), Some(80_000));
        assert_eq!(theoretical_best(&records, "monza", Some("GP"), "ks_ferrari"), Some(70_000));
        assert_eq!(best_sectors(&records, "monza", None, "ks_audi"), None);
        assert_eq!(theoretical_best(&records, "nurburgring", None, "ks_ferrari"), None);
    }

    #[test]
    fn no_usable_splits_gives_none() {
        let plain = vec![rec("monza", None, "ks_ferrari", "a", 80_000, "t1")];
        assert_eq!(best_sectors(&plain, "monza", None, "ks_ferrari"), None);
        assert_eq!(theoretical_best(&plain, "monza", None, "ks_ferrari"), None);
        assert_eq!(best_sectors(&[], "monza", None, "ks_ferrari"), None);
    }

    #[test]
    fn invalid_and_mismatched_splits_are_ignored() {
        let records = vec![
            with_sectors(rec("monza", None, "ks_ferrari", "a", 80_000, "t1"), &[26_000, 27_000, 27_000]),
            // Zero sector: not a valid split.
            with_sectors(rec("monza", None, "ks_ferrari", "a", 75_000, "t2"), &[0, 25_000, 25_000]),
            // Empty split list: ignored.
            with_sectors(rec("monza", None, "ks_ferrari", "a", 75_000, "t3"), &[]),
            // Two sectors where the first usable split had three: ignored.
            with_sectors(rec("monza", None, "ks_ferrari", "a", 74_000, "t4"), &[37_000, 37_000]),
        ];
        assert_eq!(
            best_sectors(&records, "monza", None, "ks_ferrari"),
            Some(vec![26_000, 27_000, 27_000])
        );
        assert_eq!(theoretical_best(&records, "monza", None, "ks_ferrari"), Some(80_000));
    }

    #[test]
    fn theoretical_best_overflow_is_none() {
        let records = vec![with_sectors(rec("monza", None, "ks_ferrari", "a", 1, "t1"), &[u32::MAX, 1])];
        assert_eq!(
            best_sectors(&records, "monza", None, "ks_ferrari"),
            Some(vec![u32::MAX, 1])
        );
        assert_eq!(theoretical_best(&records, "monza", None, "ks_ferrari"), None);
    }
}

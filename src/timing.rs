//! Timing tower model: race order, gaps to the leader and session-best lookup.
//!
//! A feed is either a list of [`Entry`] values or CSV text with the columns
//! `driver,laps,last_ms,best_ms,position`. [`build_tower`] validates the rows,
//! sorts them by position and fills in the gap column. Times are whole
//! milliseconds. An empty `last_ms` or `best_ms` field (or `-`) means no lap yet.

use std::fmt;

/// One driver's line in the tower.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub driver: String,
    pub position: u32,
    pub laps: u32,
    pub last_ms: Option<u32>,
    pub best_ms: Option<u32>,
    /// Gap to the leader in milliseconds. A gap already set on the input is
    /// kept as the feed's own gap; otherwise it is the difference of best laps.
    pub gap_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimingError {
    /// A driver name is empty or only whitespace.
    EmptyName,
    /// Two drivers share a position.
    DuplicatePosition(u32),
    /// A position of zero, which no grid uses.
    ZeroPosition(String),
    /// A CSV line that cannot be read. `line` is 1-based.
    Line { line: usize, message: String },
}

impl fmt::Display for TimingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "a driver name is empty"),
            Self::DuplicatePosition(p) => write!(f, "position {p} is used more than once"),
            Self::ZeroPosition(name) => write!(f, "{name} has position 0; positions start at 1"),
            Self::Line { line, message } => write!(f, "line {line}: {message}"),
        }
    }
}

impl std::error::Error for TimingError {}

/// Validate entries, sort them by position and compute gaps to the leader.
///
/// The leader is the lowest position and always has a gap of zero. Any other
/// entry keeps its own `gap_ms` when it has one, or else gets its best lap minus
/// the leader's best lap. The gap is `None` when either lap is missing.
pub fn build_tower(entries: Vec<Entry>) -> Result<Vec<Entry>, TimingError> {
    let mut rows = Vec::with_capacity(entries.len());
    for mut entry in entries {
        let name = entry.driver.trim();
        if name.is_empty() {
            return Err(TimingError::EmptyName);
        }
        if entry.position == 0 {
            return Err(TimingError::ZeroPosition(name.to_string()));
        }
        entry.driver = name.to_string();
        rows.push(entry);
    }

    rows.sort_by_key(|e| e.position);
    if let Some(pair) = rows.windows(2).find(|w| w[0].position == w[1].position) {
        return Err(TimingError::DuplicatePosition(pair[0].position));
    }

    let leader_best = rows.first().and_then(|leader| leader.best_ms);
    for (index, row) in rows.iter_mut().enumerate() {
        row.gap_ms = if index == 0 {
            Some(0)
        } else if row.gap_ms.is_some() {
            row.gap_ms
        } else {
            match (row.best_ms, leader_best) {
                (Some(best), Some(lead)) => Some(i64::from(best) - i64::from(lead)),
                _ => None,
            }
        };
    }
    Ok(rows)
}

/// Parse the CSV feed and build the tower from it.
///
/// A header line whose first field is `driver` is skipped. Blank lines and lines
/// starting with `#` are ignored. Fields are not quoted, so names cannot hold commas.
pub fn parse_csv(text: &str) -> Result<Vec<Entry>, TimingError> {
    let mut entries = Vec::new();
    let mut first_content = true;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();
        if first_content && fields[0].eq_ignore_ascii_case("driver") {
            first_content = false;
            continue;
        }
        first_content = false;
        entries.push(parse_row(line_no, &fields)?);
    }
    build_tower(entries)
}

fn parse_row(line: usize, fields: &[&str]) -> Result<Entry, TimingError> {
    let fail = |message: String| TimingError::Line { line, message };
    if fields.len() != 5 {
        return Err(fail(format!("expected 5 fields, found {}", fields.len())));
    }
    if fields[0].is_empty() {
        return Err(fail("driver name is empty".to_string()));
    }
    let laps = fields[1]
        .parse::<u32>()
        .map_err(|_| fail(format!("laps {:?} is not a whole number", fields[1])))?;
    let last_ms = parse_optional_ms(fields[2]).map_err(|m| fail(format!("last_ms {m}")))?;
    let best_ms = parse_optional_ms(fields[3]).map_err(|m| fail(format!("best_ms {m}")))?;
    let position = fields[4]
        .parse::<u32>()
        .map_err(|_| fail(format!("position {:?} is not a whole number", fields[4])))?;
    Ok(Entry {
        driver: fields[0].to_string(),
        position,
        laps,
        last_ms,
        best_ms,
        gap_ms: None,
    })
}

/// `Ok(None)` for an empty or `-` field, `Err` with a reason for a bad value.
fn parse_optional_ms(field: &str) -> Result<Option<u32>, String> {
    if field.is_empty() || field == "-" {
        return Ok(None);
    }
    match field.parse::<u32>() {
        Ok(0) => Err("of 0 is not a lap time".to_string()),
        Ok(ms) => Ok(Some(ms)),
        Err(_) => Err(format!("{field:?} is not a whole number of milliseconds")),
    }
}

/// The fastest lap in the session and the driver who set it.
///
/// Ties go to the driver in the better position. Returns `None` when nobody has a lap.
pub fn session_best(entries: &[Entry]) -> Option<(String, u32)> {
    entries
        .iter()
        .filter_map(|e| e.best_ms.map(|best| (best, e.position, e.driver.as_str())))
        .min_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(best, _, name)| (name.to_string(), best))
}

/// A lap time as `m:ss.mmm`, e.g. `83456` becomes `1:23.456`. Minutes are not capped.
pub fn format_lap(ms: u32) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

/// A gap as a signed `+s.mmm`, e.g. `1234` becomes `+1.234`. Seconds are the total, not split into minutes.
pub fn format_gap(gap_ms: i64) -> String {
    let sign = if gap_ms < 0 { '-' } else { '+' };
    let abs = gap_ms.unsigned_abs();
    format!("{sign}{}.{:03}", abs / 1_000, abs % 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(driver: &str, position: u32, best_ms: Option<u32>) -> Entry {
        Entry {
            driver: driver.to_string(),
            position,
            laps: 10,
            last_ms: None,
            best_ms,
            gap_ms: None,
        }
    }

    #[test]
    fn tower_is_sorted_by_position() {
        let tower = build_tower(vec![
            entry("C", 3, Some(90_000)),
            entry("A", 1, Some(88_000)),
            entry("B", 2, Some(89_000)),
        ])
        .unwrap();
        let order: Vec<&str> = tower.iter().map(|e| e.driver.as_str()).collect();
        assert_eq!(order, ["A", "B", "C"]);
    }

    #[test]
    fn gaps_come_from_best_laps_against_leader() {
        let tower = build_tower(vec![
            entry("A", 1, Some(88_000)),
            entry("B", 2, Some(88_750)),
            entry("C", 3, Some(87_900)),
        ])
        .unwrap();
        assert_eq!(tower[0].gap_ms, Some(0));
        assert_eq!(tower[1].gap_ms, Some(750));
        assert_eq!(tower[2].gap_ms, Some(-100));
    }

    #[test]
    fn gap_is_none_without_a_lap_to_compare() {
        let tower = build_tower(vec![
            entry("A", 1, Some(88_000)),
            entry("B", 2, None),
            entry("C", 3, Some(89_000)),
        ])
        .unwrap();
        assert_eq!(tower[1].gap_ms, None);
        assert_eq!(tower[2].gap_ms, Some(1_000));

        let no_leader_lap = build_tower(vec![entry("A", 1, None), entry("B", 2, Some(89_000))]).unwrap();
        assert_eq!(no_leader_lap[0].gap_ms, Some(0));
        assert_eq!(no_leader_lap[1].gap_ms, None);
    }

    #[test]
    fn feed_gap_is_kept_for_non_leaders() {
        let mut b = entry("B", 2, Some(89_000));
        b.gap_ms = Some(4_500);
        let tower = build_tower(vec![entry("A", 1, Some(88_000)), b]).unwrap();
        assert_eq!(tower[1].gap_ms, Some(4_500));
    }

    #[test]
    fn duplicate_position_is_rejected() {
        let err = build_tower(vec![entry("A", 1, None), entry("B", 1, None)]).unwrap_err();
        assert_eq!(err, TimingError::DuplicatePosition(1));
    }

    #[test]
    fn empty_and_zero_position_are_rejected() {
        assert_eq!(build_tower(vec![entry("  ", 1, None)]), Err(TimingError::EmptyName));
        assert_eq!(
            build_tower(vec![entry("A", 0, None)]),
            Err(TimingError::ZeroPosition("A".to_string()))
        );
    }

    #[test]
    fn names_are_trimmed() {
        let tower = build_tower(vec![entry("  Ana ", 1, None)]).unwrap();
        assert_eq!(tower[0].driver, "Ana");
    }

    #[test]
    fn csv_with_header_and_blank_lines() {
        let text = "driver,laps,last_ms,best_ms,position\n\
                    # comment\n\
                    \n\
                    Ana, 12, 91000, 89500, 2\n\
                    Ben,12,90100,89000,1\n\
                    Cy,11,,90400,3\n";
        let tower = parse_csv(text).unwrap();
        assert_eq!(tower.len(), 3);
        assert_eq!(tower[0].driver, "Ben");
        assert_eq!(tower[1].laps, 12);
        assert_eq!(tower[1].last_ms, Some(91_000));
        assert_eq!(tower[2].last_ms, None);
        assert_eq!(tower[1].gap_ms, Some(500));
        assert_eq!(tower[2].gap_ms, Some(1_400));
    }

    #[test]
    fn csv_dash_means_no_lap() {
        let tower = parse_csv("Ana,3,-,-,1").unwrap();
        assert_eq!(tower[0].best_ms, None);
        assert_eq!(tower[0].last_ms, None);
    }

    #[test]
    fn csv_rejects_bad_rows_with_line_numbers() {
        let short = parse_csv("Ana,3,90000,89000").unwrap_err();
        assert!(matches!(short, TimingError::Line { line: 1, .. }));

        let text_laps = parse_csv("Ana,x,90000,89000,1").unwrap_err();
        assert!(matches!(text_laps, TimingError::Line { line: 1, .. }));

        let zero_time = parse_csv("driver,laps,last_ms,best_ms,position\nAna,3,0,89000,1").unwrap_err();
        assert!(matches!(zero_time, TimingError::Line { line: 2, .. }));

        let no_name = parse_csv(",3,90000,89000,1").unwrap_err();
        assert!(matches!(no_name, TimingError::Line { line: 1, .. }));
    }

    #[test]
    fn csv_duplicate_position_is_rejected() {
        let err = parse_csv("Ana,3,,89000,1\nBen,3,,90000,1").unwrap_err();
        assert_eq!(err, TimingError::DuplicatePosition(1));
    }

    #[test]
    fn empty_feed_gives_empty_tower() {
        assert_eq!(parse_csv("  \n").unwrap(), Vec::new());
    }

    #[test]
    fn session_best_is_fastest_lap_with_tie_to_better_position() {
        let entries = vec![
            entry("A", 1, Some(90_000)),
            entry("B", 2, Some(88_500)),
            entry("C", 3, Some(88_500)),
            entry("D", 4, None),
        ];
        assert_eq!(session_best(&entries), Some(("B".to_string(), 88_500)));
        assert_eq!(session_best(&[entry("A", 1, None)]), None);
        assert_eq!(session_best(&[]), None);
    }

    #[test]
    fn lap_format_is_minutes_seconds_millis() {
        assert_eq!(format_lap(83_456), "1:23.456");
        assert_eq!(format_lap(59_999), "0:59.999");
        assert_eq!(format_lap(0), "0:00.000");
        assert_eq!(format_lap(3_601_005), "60:01.005");
    }

    #[test]
    fn gap_format_is_signed_seconds() {
        assert_eq!(format_gap(1_234), "+1.234");
        assert_eq!(format_gap(0), "+0.000");
        assert_eq!(format_gap(-500), "-0.500");
        assert_eq!(format_gap(75_010), "+75.010");
    }
}

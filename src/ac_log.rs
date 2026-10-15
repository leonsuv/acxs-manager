//! Reads and summarizes Assetto Corsa and Custom Shaders Patch (CSP) log files,
//! so a failed launch can be explained without opening the raw log.
//!
//! Lines recognised as errors:
//! - `ERROR: ...` prefix.
//! - CSP log rows with a `| ERROR |` column, message after the second `|`.
//! - `Error, cannot initialize ...` and `CRITICAL ERROR` messages from the game.
//! - `CRASH in:` starts a crash stack.
//! - `[CSP SAYS] message` carries the cause reported by CSP.
//!
//! Warning and info lines use the same shapes with `WARNING` or `INFO` in place of
//! `ERROR`. Those forms are assumed by analogy and are not checked against real logs.
//!
//! Log location: the game log is `log.txt` in `Assetto Corsa/logs/` under the user's
//! documents folder. That folder is not under the install root, so
//! [`known_log_paths`] returns no paths.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Largest amount of a log file read by [`read_log`]. Larger files keep their tail.
pub const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;

/// Maximum number of entries in [`LogSummary::first_errors`].
pub const MAX_FIRST_ERRORS: usize = 20;

const UTF8_BOM: &str = "\u{feff}";
const UTF8_BOM_BYTES: &[u8] = b"\xEF\xBB\xBF";

/// Substrings that mark a line as an error. Warning and info forms are handled in
/// [`classify`], not here.
const ERROR_MARKERS: &[&str] = &[
    "ERROR:",
    "| ERROR |",
    "CRITICAL ERROR",
    "CRASH in:",
    "Error, cannot initialize",
    "[CSP SAYS]",
    "COULD NOT FIND SUSPENSION OBJECT",
    "swapChain->Present failed",
    "ACClient:: ACP_WRONG_PASSWORD",
    "ACP_NO_SLOTS_AVAILABLE",
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Other,
}

#[derive(Debug, Clone)]
pub struct LogLine {
    pub level: LogLevel,
    /// The line with trailing whitespace removed.
    pub text: String,
    /// 1-based line number in the parsed text.
    pub line_no: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LogSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    /// The first errors in the log, at most [`MAX_FIRST_ERRORS`].
    pub first_errors: Vec<LogLine>,
}

fn classify(line: &str) -> LogLevel {
    if ERROR_MARKERS.iter().any(|m| line.contains(m)) {
        LogLevel::Error
    } else if line.contains("| WARNING |") || line.contains("WARNING:") {
        LogLevel::Warning
    } else if line.contains("| INFO |") || line.contains("INFO:") {
        LogLevel::Info
    } else {
        LogLevel::Other
    }
}

/// Splits log text into non-blank lines, classifying each one.
///
/// A leading BOM is ignored. `line_no` counts every physical line, blank ones included.
pub fn parse_lines(text: &str) -> Vec<LogLine> {
    let text = text.strip_prefix(UTF8_BOM).unwrap_or(text);
    text.lines()
        .enumerate()
        .filter_map(|(i, raw)| {
            let line = raw.trim_end();
            if line.trim().is_empty() {
                return None;
            }
            Some(LogLine {
                level: classify(line),
                text: line.to_string(),
                line_no: i + 1,
            })
        })
        .collect()
}

/// Counts errors, warnings and infos, and keeps the first [`MAX_FIRST_ERRORS`] errors.
pub fn summarize(text: &str) -> LogSummary {
    let mut summary = LogSummary::default();
    for line in parse_lines(text) {
        match line.level {
            LogLevel::Error => {
                summary.errors += 1;
                if summary.first_errors.len() < MAX_FIRST_ERRORS {
                    summary.first_errors.push(line);
                }
            }
            LogLevel::Warning => summary.warnings += 1,
            LogLevel::Info => summary.infos += 1,
            LogLevel::Other => {}
        }
    }
    summary
}

/// Reads a log file as lossy UTF-8.
///
/// Files over [`MAX_LOG_BYTES`] return only their tail, since the end of a log shows the
/// crash. The partial first line is dropped. A leading UTF-8 BOM is removed.
pub fn read_log(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let truncated = len > MAX_LOG_BYTES;
    if truncated {
        file.seek(SeekFrom::Start(len - MAX_LOG_BYTES))?;
    }
    let mut bytes = Vec::new();
    file.take(MAX_LOG_BYTES).read_to_end(&mut bytes)?;

    let body: &[u8] = if truncated {
        match bytes.iter().position(|&b| b == b'\n') {
            Some(i) => &bytes[i + 1..],
            None => &bytes[..],
        }
    } else if bytes.starts_with(UTF8_BOM_BYTES) {
        &bytes[UTF8_BOM_BYTES.len()..]
    } else {
        &bytes[..]
    };
    Ok(String::from_utf8_lossy(body).into_owned())
}

/// Locations of AC and CSP logs under the install root `ac_root`.
///
/// The game log is under the user's documents folder, not under `ac_root`, so it cannot be
/// resolved from the install root. This therefore returns an empty list. Callers can read
/// `<MyDocuments>/Assetto Corsa/logs/log.txt` directly with [`read_log`].
pub fn known_log_paths(ac_root: &Path) -> Vec<PathBuf> {
    let _ = ac_root;
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file in the system temp dir that is removed when the guard is dropped,
    /// including when an assertion fails.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new(name: &str, bytes: &[u8]) -> TempFile {
            let path = std::env::temp_dir().join(format!(
                "acxs_ac_log_test_{}_{}",
                std::process::id(),
                name
            ));
            std::fs::write(&path, bytes).unwrap();
            TempFile(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn classifies_error_lines() {
        let samples = [
            "ERROR: INIReader: content/cars/abc/data/car.ini > KEY_NOT_FOUND: [INFO] SCREEN_NAME",
            "Error, cannot initialize Post Processing, system/cfg/ppfilters/default.ini not found",
            "Kunos Simulazioni: CRITICAL ERROR CANNOT HAVE 0 CARS IN RACE.INI",
            "CRASH in: ",
            "[CSP SAYS] shader compile failed",
            "2024-05-01 10:00:00 | ERROR | shader compile failed",
        ];
        for s in samples {
            let lines = parse_lines(s);
            assert_eq!(lines[0].level, LogLevel::Error, "line: {s}");
        }
    }

    #[test]
    fn classifies_warning_lines() {
        let lines = parse_lines("2024-05-01 10:00:00 | WARNING | texture is large\nWARNING: texture is large");
        assert_eq!(lines[0].level, LogLevel::Warning);
        assert_eq!(lines[1].level, LogLevel::Warning);
    }

    #[test]
    fn classifies_info_lines() {
        let lines = parse_lines("2024-05-01 10:00:00 | INFO | loaded car");
        assert_eq!(lines[0].level, LogLevel::Info);
    }

    #[test]
    fn classifies_other_lines() {
        let lines = parse_lines("content/cars/abc/data/car.ini loaded\n    (1234): AnimationBlender::blendAnimations");
        assert!(lines.iter().all(|l| l.level == LogLevel::Other));
    }

    #[test]
    fn line_numbers_skip_blank_lines_and_handle_crlf() {
        let lines = parse_lines("first\r\n\r\n   \r\nERROR: third\r\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].line_no, 1);
        assert_eq!(lines[1].line_no, 4);
        assert_eq!(lines[1].text, "ERROR: third");
        assert_eq!(lines[1].level, LogLevel::Error);
    }

    #[test]
    fn summary_counts_levels() {
        let text = "ERROR: a\n| WARNING | b\n| INFO | c\nplain\nERROR: d\n";
        let s = summarize(text);
        assert_eq!(s.errors, 2);
        assert_eq!(s.warnings, 1);
        assert_eq!(s.infos, 1);
        assert_eq!(s.first_errors.len(), 2);
        assert_eq!(s.first_errors[1].line_no, 5);
    }

    #[test]
    fn first_errors_capped_at_twenty() {
        let text: String = (0..50).map(|i| format!("ERROR: number {i}\n")).collect();
        let s = summarize(&text);
        assert_eq!(s.errors, 50);
        assert_eq!(s.first_errors.len(), MAX_FIRST_ERRORS);
        assert_eq!(s.first_errors[0].text, "ERROR: number 0");
        assert_eq!(s.first_errors[19].text, "ERROR: number 19");
    }

    #[test]
    fn read_log_keeps_tail_of_large_file() {
        let mut text = String::new();
        let mut i = 0usize;
        while (text.len() as u64) <= MAX_LOG_BYTES + 1024 {
            text.push_str(&format!("filler line {i:08} padding padding padding\n"));
            i += 1;
        }
        text.push_str("CRASH in: final line\n");
        let file = TempFile::new("large.txt", text.as_bytes());
        let read = read_log(file.path()).unwrap();

        assert!(read.len() as u64 <= MAX_LOG_BYTES);
        assert!(read.ends_with("CRASH in: final line\n"));
        assert!(!read.contains("filler line 00000000"));
        // The first line must be complete, not a cut-off fragment of a filler line.
        assert!(read.starts_with("filler line "));
        assert!(read.lines().next().unwrap().ends_with("padding padding padding"));
    }

    #[test]
    fn read_log_strips_bom() {
        let mut bytes = UTF8_BOM_BYTES.to_vec();
        bytes.extend_from_slice(b"ERROR: first\n");
        let file = TempFile::new("bom.txt", &bytes);
        let read = read_log(file.path()).unwrap();

        assert_eq!(read, "ERROR: first\n");
    }

    #[test]
    fn parse_lines_ignores_bom() {
        let lines = parse_lines("\u{feff}ERROR: first");
        assert_eq!(lines[0].text, "ERROR: first");
        assert_eq!(lines[0].line_no, 1);
        assert_eq!(lines[0].level, LogLevel::Error);
    }

    #[test]
    fn lossy_utf8_is_replaced_not_rejected() {
        let file = TempFile::new("lossy.txt", b"ok line\n\xff\xfe bad bytes\nERROR: after bad\n");
        let read = read_log(file.path()).unwrap();

        assert!(read.contains('\u{FFFD}'));
        let s = summarize(&read);
        assert_eq!(s.errors, 1);
        assert_eq!(s.first_errors[0].text, "ERROR: after bad");
    }

    #[test]
    fn read_log_missing_file_is_error() {
        let path = std::env::temp_dir().join(format!(
            "acxs_ac_log_test_missing_{}.txt",
            std::process::id()
        ));
        assert!(read_log(&path).is_err());
    }

    #[test]
    fn known_log_paths_is_empty() {
        assert!(known_log_paths(Path::new("C:/Program Files (x86)/Steam/steamapps/common/assettocorsa")).is_empty());
    }
}

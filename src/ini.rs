//! Minimal INI reader and writer for Assetto Corsa files.
//!
//! Keeps comments, blank lines and key order, so editing one value and saving
//! does not reformat the rest of the file. Keys are matched case-insensitively,
//! section names too, as the game does.

use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
enum Line {
    /// `key=value`, with the key as written in the file.
    Entry { key: String, value: String },
    /// Section header, comment, blank line, or anything else. Kept verbatim.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub name: String,
    lines: Vec<Line>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ini {
    /// Lines before the first section header.
    preamble: Vec<Line>,
    sections: Vec<Section>,
    /// The file was not valid UTF-8 and was read as Windows ANSI (Latin-1), so it is
    /// written back the same way and its bytes survive a load and save.
    latin1: bool,
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let text = text.trim_start_matches('\u{feff}');
        let mut ini = Ini::default();
        let mut current: Option<usize> = None;
        for raw in text.lines() {
            let trimmed = raw.trim();
            if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                ini.sections.push(Section {
                    name: name.trim().to_string(),
                    lines: vec![Line::Raw(raw.to_string())],
                });
                current = Some(ini.sections.len() - 1);
                continue;
            }
            let line = match trimmed.split_once('=') {
                Some((key, value))
                    if !trimmed.starts_with(';') && !trimmed.starts_with('#') && !key.trim().is_empty() =>
                {
                    Line::Entry {
                        key: key.trim().to_string(),
                        value: value.trim().to_string(),
                    }
                }
                _ => Line::Raw(raw.to_string()),
            };
            match current {
                Some(i) => ini.sections[i].lines.push(line),
                None => ini.preamble.push(line),
            }
        }
        ini
    }

    /// Reads `path`. Files that are not valid UTF-8 (many mod files are saved as
    /// Windows ANSI) are read byte for byte as Latin-1 instead of failing, and are
    /// saved in that encoding again.
    pub fn load(path: &Path) -> io::Result<Ini> {
        let bytes = fs::read(path)?;
        match String::from_utf8(bytes) {
            Ok(text) => Ok(Ini::parse(&text)),
            Err(err) => {
                let text: String = err.into_bytes().iter().map(|&b| char::from(b)).collect();
                let mut ini = Ini::parse(&text);
                ini.latin1 = true;
                Ok(ini)
            }
        }
    }

    /// Save in Windows ANSI (Latin-1) instead of UTF-8, as [`Ini::load`] does for
    /// files that were not UTF-8. Characters above U+00FF still force UTF-8.
    pub fn set_latin1(&mut self, latin1: bool) {
        self.latin1 = latin1;
    }

    fn find_section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn section_names(&self) -> Vec<&str> {
        self.sections.iter().map(|s| s.name.as_str()).collect()
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.find_section(section)?.lines.iter().find_map(|line| match line {
            Line::Entry { key: k, value } if k.eq_ignore_ascii_case(key) => Some(value.as_str()),
            _ => None,
        })
    }

    /// The value as a number. A trailing `;` or `//` comment, which the game's
    /// own files use on most lines (`TOTALMASS=1240 ; kg`), is ignored.
    pub fn get_f64(&self, section: &str, key: &str) -> Option<f64> {
        value_without_comment(self.get(section, key)?).parse().ok()
    }

    /// Like [`Ini::get_f64`], for integers.
    pub fn get_i64(&self, section: &str, key: &str) -> Option<i64> {
        value_without_comment(self.get(section, key)?).parse().ok()
    }

    /// All `key=value` pairs of a section, in file order.
    pub fn entries(&self, section: &str) -> Vec<(&str, &str)> {
        self.find_section(section)
            .map(|s| {
                s.lines
                    .iter()
                    .filter_map(|line| match line {
                        Line::Entry { key, value } => Some((key.as_str(), value.as_str())),
                        Line::Raw(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Set a value. Creates the section and key when missing; updates in place otherwise.
    pub fn set(&mut self, section: &str, key: &str, value: impl ToString) {
        let value = value.to_string();
        let index = match self.sections.iter().position(|s| s.name.eq_ignore_ascii_case(section)) {
            Some(i) => i,
            None => {
                self.sections.push(Section {
                    name: section.to_string(),
                    lines: vec![Line::Raw(format!("[{section}]"))],
                });
                self.sections.len() - 1
            }
        };
        let lines = &mut self.sections[index].lines;
        for line in lines.iter_mut() {
            if let Line::Entry { key: k, value: v } = line && k.eq_ignore_ascii_case(key) {
                *v = value;
                return;
            }
        }
        // Insert after the last entry of the section so new keys stay grouped.
        let insert_at = lines
            .iter()
            .rposition(|l| matches!(l, Line::Entry { .. }))
            .map(|i| i + 1)
            .unwrap_or(lines.len());
        lines.insert(insert_at, Line::Entry { key: key.to_string(), value });
    }

    /// Render with CRLF line endings, which is what the game writes on Windows.
    pub fn to_string_crlf(&self) -> String {
        let mut out = String::new();
        let all = self.preamble.iter().chain(self.sections.iter().flat_map(|s| s.lines.iter()));
        for line in all {
            match line {
                Line::Entry { key, value } => out.push_str(&format!("{key}={value}")),
                Line::Raw(text) => out.push_str(text),
            }
            out.push_str("\r\n");
        }
        out
    }

    /// Write to `path`, first moving any existing file to `<name>.acxs-backup-<stamp>`.
    /// Nothing is ever deleted.
    pub fn save_with_backup(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // Never reuse a backup name: step forward a second until the name is free.
            let mut when = chrono::Local::now();
            let backup = loop {
                let candidate = path.with_file_name(format!(
                    "{name}.acxs-backup-{}",
                    when.format("%Y%m%d-%H%M%S")
                ));
                if !candidate.exists() {
                    break candidate;
                }
                when += chrono::TimeDelta::seconds(1);
            };
            fs::rename(path, backup)?;
        }
        fs::write(path, self.encoded())
    }

    /// The CRLF text in the file's own encoding: Latin-1 when it was read that way
    /// and every character still fits, UTF-8 otherwise.
    fn encoded(&self) -> Vec<u8> {
        let text = self.to_string_crlf();
        if self.latin1 && text.chars().all(|c| u32::from(c) <= 0xFF) {
            text.chars().map(|c| u32::from(c) as u8).collect()
        } else {
            text.into_bytes()
        }
    }
}

/// `value` up to its first `;` or `//` comment, trimmed.
fn value_without_comment(value: &str) -> &str {
    let cut = [value.find(';'), value.find("//")].into_iter().flatten().min().unwrap_or(value.len());
    value[..cut].trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_file_loads_and_saves_byte_for_byte() {
        let dir = std::env::temp_dir().join(format!("acxs-ini-ansi-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("car.ini");
        // A comment with "Größe" written in Windows-1252.
        let bytes = b"[BASIC]\r\nTOTALMASS=1240 ; Gr\xf6\xdfe\r\n".to_vec();
        fs::write(&path, &bytes).unwrap();
        let mut ini = Ini::load(&path).unwrap();
        assert_eq!(ini.get_f64("BASIC", "TOTALMASS"), Some(1240.0));
        ini.set("BASIC", "NEW", 1);
        ini.save_with_backup(&path).unwrap();
        let saved = fs::read(&path).unwrap();
        assert!(saved.starts_with(b"[BASIC]\r\nTOTALMASS=1240 ; Gr\xf6\xdfe\r\nNEW=1\r\n"));
        fs::remove_dir_all(&dir).unwrap();
    }

    const SAMPLE: &str = "; header comment\r\n[BASIC]\r\nNAME=Ferrari\r\n\r\n[ENGINE]\r\nMASS=1200 ; inline kept as value\r\n";

    #[test]
    fn reads_values_case_insensitively() {
        let ini = Ini::parse(SAMPLE);
        assert_eq!(ini.get("basic", "name"), Some("Ferrari"));
        // The inline comment stays part of the raw value, but numbers ignore it.
        assert!(ini.get("ENGINE", "mass").is_some_and(|v| v.contains(';')));
        assert!(ini.get_f64("ENGINE", "mass").is_some());
        assert_eq!(ini.get("ENGINE", "MASS"), Some("1200 ; inline kept as value"));
    }

    #[test]
    fn round_trip_keeps_comments_and_order() {
        let ini = Ini::parse(SAMPLE);
        assert_eq!(ini.to_string_crlf(), SAMPLE);
    }

    #[test]
    fn set_updates_in_place_and_adds_new_keys() {
        let mut ini = Ini::parse(SAMPLE);
        ini.set("BASIC", "NAME", "Lotus");
        ini.set("BASIC", "TAGS", "gt3");
        ini.set("NEW", "KEY", 5);
        let text = ini.to_string_crlf();
        assert!(text.contains("NAME=Lotus\r\n"));
        assert!(text.contains("[BASIC]\r\nNAME=Lotus\r\nTAGS=gt3\r\n"));
        assert!(text.contains("[NEW]\r\nKEY=5\r\n"));
        assert!(text.starts_with("; header comment\r\n"));
    }

    #[test]
    fn save_with_backup_never_deletes() {
        let dir = std::env::temp_dir().join(format!("acxs-ini-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("car.ini");
        Ini::parse("[A]\nX=1\n").save_with_backup(&path).unwrap();
        Ini::parse("[A]\nX=2\n").save_with_backup(&path).unwrap();
        assert_eq!(Ini::load(&path).unwrap().get("A", "X"), Some("2"));
        let backups = fs::read_dir(&dir).unwrap().count();
        assert_eq!(backups, 2);
        fs::remove_dir_all(dir).unwrap();
    }
}

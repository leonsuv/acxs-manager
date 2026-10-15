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

    pub fn load(path: &Path) -> io::Result<Ini> {
        Ok(Ini::parse(&fs::read_to_string(path)?))
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

    pub fn get_f64(&self, section: &str, key: &str) -> Option<f64> {
        self.get(section, key)?.parse().ok()
    }

    pub fn get_i64(&self, section: &str, key: &str) -> Option<i64> {
        self.get(section, key)?.parse().ok()
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
        fs::write(path, self.to_string_crlf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "; header comment\r\n[BASIC]\r\nNAME=Ferrari\r\n\r\n[ENGINE]\r\nMASS=1200 ; inline kept as value\r\n";

    #[test]
    fn reads_values_case_insensitively() {
        let ini = Ini::parse(SAMPLE);
        assert_eq!(ini.get("basic", "name"), Some("Ferrari"));
        assert_eq!(ini.get_f64("ENGINE", "mass"), None); // inline comment is part of the value
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

//! Three-way merge for Assetto Corsa ini files (setups, configs).
//!
//! `base` is the common ancestor, `ours` is the user's copy and `theirs` is the
//! other version (for example a newer setup). The changes made from `base` to
//! `theirs` are applied onto `ours`, so neither side's edits are lost. Comments,
//! blank lines and key order of `ours` are kept.
//!
//! Each key is compared on its own. A missing key is `None`, so removing a key
//! counts as changing it to `None`.
//! - unchanged on both sides: kept as in `base`
//! - changed on one side only: that side's value is used (a removal included)
//! - changed identically on both sides: that value is used
//! - changed differently on both sides: a conflict. The `ours` value is kept
//!   and the conflict is reported.

use std::collections::HashSet;
use std::io;
use std::path::Path;

use crate::ini::Ini;

/// A key that both sides changed to different values.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeConflict {
    Both {
        section: String,
        key: String,
        base: Option<String>,
        ours: Option<String>,
        theirs: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct MergeResult {
    /// `ours` with the changes from `theirs` applied. Conflicting keys keep the `ours` value.
    pub merged: Ini,
    pub conflicts: Vec<MergeConflict>,
    /// Keys whose value was taken from `theirs` because only `theirs` changed them.
    pub applied_from_theirs: usize,
    /// Keys changed only in `ours`, whose value was kept.
    pub applied_from_ours: usize,
}

/// Merge the changes made from `base` to `theirs` into `ours`. See the module docs for the rules.
pub fn three_way(base: &Ini, ours: &Ini, theirs: &Ini) -> MergeResult {
    let mut conflicts = Vec::new();
    let mut applied_from_theirs = 0;
    let mut applied_from_ours = 0;
    let mut removals: Vec<(String, String)> = Vec::new();
    let mut sets: Vec<(String, String, String)> = Vec::new();

    let sections = unique_names(&[ours.section_names(), theirs.section_names(), base.section_names()]);
    for section in sections {
        let keys = unique_names(&[key_names(ours, section), key_names(theirs, section), key_names(base, section)]);
        for key in keys {
            let b = base.get(section, key).map(str::to_string);
            let o = ours.get(section, key).map(str::to_string);
            let t = theirs.get(section, key).map(str::to_string);

            let result = if o == t {
                o.clone()
            } else if o == b {
                applied_from_theirs += 1;
                t.clone()
            } else if t == b {
                applied_from_ours += 1;
                o.clone()
            } else {
                conflicts.push(MergeConflict::Both {
                    section: section.to_string(),
                    key: key.to_string(),
                    base: b,
                    ours: o.clone(),
                    theirs: t,
                });
                o.clone()
            };

            if result != o {
                match result {
                    Some(value) => sets.push((section.to_string(), key.to_string(), value)),
                    None => removals.push((section.to_string(), key.to_string())),
                }
            }
        }
    }

    let mut merged = without_entries(ours, &removals);
    for (section, key, value) in sets {
        merged.set(&section, &key, value);
    }
    MergeResult {
        merged,
        conflicts,
        applied_from_theirs,
        applied_from_ours,
    }
}

/// Write the merged file, moving any existing file to a backup first (see `Ini::save_with_backup`).
/// Refuses to write while there are conflicts, unless `allow_conflicts` is set. In that case the
/// conflicting keys have the `ours` value.
pub fn apply_merge(path: &Path, result: &MergeResult, allow_conflicts: bool) -> io::Result<()> {
    if !allow_conflicts && !result.conflicts.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} conflict(s) to resolve before writing {}",
                result.conflicts.len(),
                path.display()
            ),
        ));
    }
    result.merged.save_with_backup(path)
}

/// Names from all lists, without case-insensitive repeats, in first-seen order.
fn unique_names<'a>(lists: &[Vec<&'a str>]) -> Vec<&'a str> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for name in lists.iter().flatten() {
        if seen.insert(name.to_ascii_lowercase()) {
            out.push(*name);
        }
    }
    out
}

fn key_names<'a>(ini: &'a Ini, section: &str) -> Vec<&'a str> {
    ini.entries(section).into_iter().map(|(key, _)| key).collect()
}

/// `ini` without the given `key=value` lines. `Ini` has no remove method, so the text is
/// rebuilt and parsed again. Comments, blank lines and key order are kept.
fn without_entries(ini: &Ini, removed: &[(String, String)]) -> Ini {
    if removed.is_empty() {
        return ini.clone();
    }
    let rendered = ini.to_string_crlf();
    let mut text = String::new();
    let mut section: Option<&str> = None;
    for line in rendered.lines() {
        let trimmed = line.trim();
        let mut drop = false;
        if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = Some(name.trim());
        } else if let Some((key, _)) = trimmed.split_once('=') {
            let key = key.trim();
            if !trimmed.starts_with(';') && !trimmed.starts_with('#') && !key.is_empty() {
                drop = removed.iter().any(|(s, k)| {
                    section.is_some_and(|cur| cur.eq_ignore_ascii_case(s)) && k.eq_ignore_ascii_case(key)
                });
            }
        }
        if !drop {
            text.push_str(line);
            text.push_str("\r\n");
        }
    }
    Ini::parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn ini(text: &str) -> Ini {
        Ini::parse(text)
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("acxs-ini-merge-{}-{name}", std::process::id()))
    }

    #[test]
    fn non_overlapping_changes_merge_cleanly() {
        let base = ini("[A]\nX=1\nY=1\n");
        let ours = ini("[A]\nX=2\nY=1\n");
        let theirs = ini("[A]\nX=1\nY=3\n");
        let r = three_way(&base, &ours, &theirs);
        assert!(r.conflicts.is_empty());
        assert_eq!(r.merged.get("A", "X"), Some("2"));
        assert_eq!(r.merged.get("A", "Y"), Some("3"));
        assert_eq!(r.applied_from_theirs, 1);
        assert_eq!(r.applied_from_ours, 1);
    }

    #[test]
    fn same_key_different_changes_conflict_and_keep_ours() {
        let base = ini("[A]\nX=1\n");
        let ours = ini("[A]\nX=2\n");
        let theirs = ini("[A]\nX=3\n");
        let r = three_way(&base, &ours, &theirs);
        assert_eq!(
            r.conflicts,
            vec![MergeConflict::Both {
                section: "A".into(),
                key: "X".into(),
                base: Some("1".into()),
                ours: Some("2".into()),
                theirs: Some("3".into()),
            }]
        );
        assert_eq!(r.merged.get("A", "X"), Some("2"));
    }

    #[test]
    fn identical_changes_do_not_conflict() {
        let base = ini("[A]\nX=1\n");
        let ours = ini("[A]\nX=2\n");
        let theirs = ini("[A]\nx=2\n");
        let r = three_way(&base, &ours, &theirs);
        assert!(r.conflicts.is_empty());
        assert_eq!(r.merged.get("A", "X"), Some("2"));
        assert_eq!(r.applied_from_theirs, 0);
        assert_eq!(r.applied_from_ours, 0);
    }

    #[test]
    fn keys_and_sections_added_on_one_side_are_kept() {
        let base = ini("[A]\nX=1\n");
        let ours = ini("[A]\nX=1\nW=5\n");
        let theirs = ini("[A]\nX=1\nZ=9\n[B]\nK=1\n");
        let r = three_way(&base, &ours, &theirs);
        assert!(r.conflicts.is_empty());
        assert_eq!(r.merged.get("A", "W"), Some("5"));
        assert_eq!(r.merged.get("A", "Z"), Some("9"));
        assert_eq!(r.merged.get("B", "K"), Some("1"));
        assert_eq!(r.merged.section_names(), vec!["A", "B"]);
    }

    #[test]
    fn key_removed_on_one_side_is_removed_and_comments_kept() {
        let base = ini("; keep\n[A]\nX=1\nY=1\n");
        let ours = ini("; keep\n[A]\nX=5\nY=1\n");
        let theirs = ini("; keep\n[A]\nX=1\n");
        let r = three_way(&base, &ours, &theirs);
        assert!(r.conflicts.is_empty());
        assert_eq!(r.merged.get("A", "Y"), None);
        assert_eq!(r.merged.to_string_crlf(), "; keep\r\n[A]\r\nX=5\r\n");
    }

    #[test]
    fn removal_against_a_change_is_a_conflict_keeping_ours() {
        let base = ini("[A]\nX=1\n");
        let ours = ini("[A]\nX=5\n");
        let theirs = ini("[A]\n");
        let r = three_way(&base, &ours, &theirs);
        assert_eq!(
            r.conflicts,
            vec![MergeConflict::Both {
                section: "A".into(),
                key: "X".into(),
                base: Some("1".into()),
                ours: Some("5".into()),
                theirs: None,
            }]
        );
        assert_eq!(r.merged.get("A", "X"), Some("5"));
    }

    #[test]
    fn apply_merge_refuses_conflicts_unless_allowed() {
        let dir = temp_dir("refuse");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("car.ini");
        let r = three_way(&ini("[A]\nX=1\n"), &ini("[A]\nX=2\n"), &ini("[A]\nX=3\n"));

        assert!(apply_merge(&path, &r, false).is_err());
        assert!(!path.exists());

        apply_merge(&path, &r, true).unwrap();
        assert_eq!(Ini::load(&path).unwrap().get("A", "X"), Some("2"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_merge_writes_and_backs_up_existing_file() {
        let dir = temp_dir("backup");
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("car.ini");
        ini("[A]\nX=0\n").save_with_backup(&path).unwrap();

        let r = three_way(&ini("[A]\nX=1\n"), &ini("[A]\nX=1\n"), &ini("[A]\nX=7\n"));
        assert!(r.conflicts.is_empty());
        apply_merge(&path, &r, false).unwrap();

        assert_eq!(Ini::load(&path).unwrap().get("A", "X"), Some("7"));
        let backups: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("car.ini.acxs-backup-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(Ini::load(&backups[0]).unwrap().get("A", "X"), Some("0"));
        let _ = fs::remove_dir_all(dir);
    }
}

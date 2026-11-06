//! The replays on this PC: the game's saved folder, its autosave folder and the ACXS archive.
//!
//! Autosaves are overwritten by the game, so [`keep`] copies one to the saved folder, where the
//! game leaves it alone. The screenshot list below works the same way.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where a file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayPlace {
    /// `replay\`: kept by the game.
    Saved,
    /// `replay\temp\`: the game overwrites these.
    Autosave,
    /// The ACXS archive.
    Archive,
}

impl ReplayPlace {
    pub fn label(self) -> &'static str {
        match self {
            ReplayPlace::Saved => "Saved",
            ReplayPlace::Autosave => "Autosave",
            ReplayPlace::Archive => "Archive",
        }
    }
}

/// One replay file.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayFile {
    pub path: PathBuf,
    pub place: ReplayPlace,
    /// A readable name from the file name.
    pub title: String,
    pub online: bool,
    pub bytes: u64,
    pub modified: SystemTime,
}

/// `AC_011126-153530_O_ks_porsche_911_gt3_r_2016_la_canyons_freeroam` as its title and whether it
/// was online. Car and track run together in the name, so it stays one text. Hand-named files
/// keep their name.
pub fn parse_name(stem: &str) -> (String, bool) {
    let mut parts = stem.splitn(4, '_');
    if let (Some("AC"), Some(stamp), Some(kind), Some(rest)) = (parts.next(), parts.next(), parts.next(), parts.next())
        && stamp.len() == 13
        && (kind == "R" || kind == "O")
    {
        let title = rest.strip_prefix("ks_").unwrap_or(rest).trim_end_matches('_').replace('_', " ");
        return (title, kind == "O");
    }
    (stem.to_string(), false)
}

fn list_folder(dir: &Path, place: ReplayPlace, out: &mut Vec<ReplayFile>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("acreplay")) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let (title, online) = parse_name(&stem);
        out.push(ReplayFile {
            path,
            place,
            title,
            online,
            bytes: meta.len(),
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }
}

/// All replays, newest first. A file in the archive that also sits in the game's folders is
/// listed once, from the game's folder.
pub fn list(replay_dir: &Path, archive_dir: &Path) -> Vec<ReplayFile> {
    let mut files = Vec::new();
    list_folder(replay_dir, ReplayPlace::Saved, &mut files);
    list_folder(&replay_dir.join("temp"), ReplayPlace::Autosave, &mut files);
    let known: Vec<_> = files.iter().filter_map(|f| f.path.file_name().map(ToOwned::to_owned)).collect();
    let mut archived = Vec::new();
    list_folder(archive_dir, ReplayPlace::Archive, &mut archived);
    files.extend(archived.into_iter().filter(|f| !f.path.file_name().is_some_and(|n| known.iter().any(|k| k == n))));
    files.sort_by_key(|f| std::cmp::Reverse(f.modified));
    files
}

/// Copies a replay to the saved folder and returns the new path. An existing copy stays.
pub fn keep(file: &ReplayFile, replay_dir: &Path) -> io::Result<PathBuf> {
    let name = file.path.file_name().ok_or_else(|| io::Error::other("the replay has no file name"))?;
    let target = replay_dir.join(name);
    if !target.exists() {
        std::fs::create_dir_all(replay_dir)?;
        std::fs::copy(&file.path, &target)?;
    }
    Ok(target)
}

/// Picture files in the game's screenshot folder, newest first.
pub fn screenshots(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut shots: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| ["png", "jpg", "jpeg", "bmp"].iter().any(|k| x.eq_ignore_ascii_case(k)))
        })
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH), p))
        .collect();
    shots.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    shots.into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_the_game_become_titles() {
        let (title, online) = parse_name("AC_011126-153530_O_ks_porsche_911_gt3_r_2016_la_canyons_freeroam");
        assert_eq!(title, "porsche 911 gt3 r 2016 la canyons freeroam");
        assert!(online);
        let (title, online) = parse_name("AC_051026-204720_R_ks_ferrari_488_gt3_spa_");
        assert_eq!(title, "ferrari 488 gt3 spa");
        assert!(!online);
        assert_eq!(parse_name("my best lap"), ("my best lap".to_string(), false));
    }

    #[test]
    fn list_keep_and_duplicates() {
        let root = std::env::temp_dir().join(format!("acxs-replays-{}", std::process::id()));
        let (saved, archive) = (root.join("replay"), root.join("archive"));
        std::fs::create_dir_all(saved.join("temp")).unwrap();
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::write(saved.join("temp").join("a.acreplay"), b"12").unwrap();
        std::fs::write(archive.join("a.acreplay"), b"12").unwrap();
        std::fs::write(archive.join("b.acreplay"), b"123").unwrap();
        std::fs::write(archive.join("note.txt"), b"x").unwrap();
        let files = list(&saved, &archive);
        assert_eq!(files.len(), 2);
        let autosave = files.iter().find(|f| f.title == "a").unwrap();
        assert_eq!(autosave.place, ReplayPlace::Autosave);
        keep(autosave, &saved).unwrap();
        assert!(saved.join("a.acreplay").is_file());
        assert_eq!(list(&saved, &archive).iter().filter(|f| f.place == ReplayPlace::Saved).count(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn screenshots_are_pictures_only() {
        let root = std::env::temp_dir().join(format!("acxs-shots-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.PNG"), b"x").unwrap();
        std::fs::write(root.join("b.txt"), b"x").unwrap();
        assert_eq!(screenshots(&root).len(), 1);
        assert!(screenshots(&root.join("none")).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}

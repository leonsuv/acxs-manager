//! Car sound (`sfx/`) auditor and sound swapper.
//!
//! Assetto Corsa stores each car's FMOD bank in `<car>/sfx/<car_id>.bank` with
//! event mappings in `<car>/sfx/GUIDs.txt`. Swapping a car's sound from a donor
//! archives the target's existing `sfx/` folder to `<car>/sfx.acxs-backup-<stamp>/`,
//! copies the donor's `.bank` renamed to `<target>.bank`, and rewrites `GUIDs.txt`
//! so every `event:/cars/<donor>/` and `bank:/<donor>` points to `<target>`.
//!
//! Nothing is deleted and every mutation refuses while the game is running.

use chrono::Local;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Diagnostic summary of a car's `sfx/` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfxInfo {
    pub car_id: String,
    pub bank_files: Vec<PathBuf>,
    pub guids_path: Option<PathBuf>,
    /// Distinct car ids referenced inside `GUIDs.txt` (`event:/cars/<id>/...` or `bank:/<id>`).
    pub referenced_car_ids: Vec<String>,
    pub issues: Vec<String>,
}

/// Result of a successful sound swap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfxSwapReport {
    pub target_car_id: String,
    pub donor_car_id: String,
    pub bank_written: PathBuf,
    pub guids_written: PathBuf,
    /// Where the target car's previous `sfx/` directory was moved, if it existed.
    pub backup_dir: Option<PathBuf>,
}

#[derive(Debug)]
pub enum SfxError {
    GameRunning,
    InvalidCarId(String),
    CarNotFound(PathBuf),
    MissingDonorBank(PathBuf),
    MissingDonorGuids(PathBuf),
    SameCar(String),
    Io(io::Error),
}

impl fmt::Display for SfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SfxError::GameRunning => {
                write!(f, "Close Assetto Corsa before modifying car sounds.")
            }
            SfxError::InvalidCarId(id) => write!(f, "{id:?} is not a valid car id"),
            SfxError::CarNotFound(p) => write!(f, "car folder not found: {}", p.display()),
            SfxError::MissingDonorBank(p) => {
                write!(f, "donor car has no matching .bank file at {}", p.display())
            }
            SfxError::MissingDonorGuids(p) => {
                write!(f, "donor car has no GUIDs.txt at {}", p.display())
            }
            SfxError::SameCar(id) => write!(f, "target and donor car are the same ({id})"),
            SfxError::Io(e) => write!(f, "sound operation failed: {e}"),
        }
    }
}

impl std::error::Error for SfxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SfxError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SfxError {
    fn from(e: io::Error) -> Self {
        SfxError::Io(e)
    }
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && !id
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// Inspects `<car_dir>/sfx/` for `.bank` files, `GUIDs.txt`, and car-id mismatches.
pub fn inspect_car_sfx(car_dir: &Path) -> SfxInfo {
    let car_id = car_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sfx_dir = car_dir.join("sfx");
    let mut bank_files = Vec::new();
    let mut issues = Vec::new();

    if !sfx_dir.is_dir() {
        issues.push("sfx/ folder is missing".to_string());
        return SfxInfo {
            car_id,
            bank_files,
            guids_path: None,
            referenced_car_ids: Vec::new(),
            issues,
        };
    }

    if let Ok(entries) = fs::read_dir(&sfx_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("bank"))
            {
                bank_files.push(p);
            }
        }
    }
    bank_files.sort();

    let expected_bank = format!("{car_id}.bank");
    let has_matching_bank = bank_files.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.eq_ignore_ascii_case(&expected_bank))
    });
    if bank_files.is_empty() {
        issues.push("no .bank file found in sfx/".to_string());
    } else if !car_id.is_empty() && !has_matching_bank {
        issues.push(format!("sfx/{expected_bank} is missing (found other .bank file)"));
    }

    let guids_candidate = find_guids_file(&sfx_dir);
    let mut referenced = BTreeSet::new();
    match &guids_candidate {
        None => issues.push("sfx/GUIDs.txt is missing".to_string()),
        Some(path) => match fs::read_to_string(path) {
            Ok(text) => {
                extract_referenced_car_ids(&text, &mut referenced);
                if !car_id.is_empty()
                    && !referenced.is_empty()
                    && !referenced.iter().any(|r| r.eq_ignore_ascii_case(&car_id))
                {
                    let refs: Vec<&str> = referenced.iter().map(String::as_str).collect();
                    issues.push(format!(
                        "GUIDs.txt references {} instead of {car_id}",
                        refs.join(", ")
                    ));
                }
            }
            Err(e) => issues.push(format!("could not read GUIDs.txt: {e}")),
        },
    }

    SfxInfo {
        car_id,
        bank_files,
        guids_path: guids_candidate,
        referenced_car_ids: referenced.into_iter().collect(),
        issues,
    }
}

/// Finds `GUIDs.txt` case-insensitively inside `sfx_dir`.
pub fn find_guids_file(sfx_dir: &Path) -> Option<PathBuf> {
    let direct = sfx_dir.join("GUIDs.txt");
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(sfx_dir).ok()?.flatten().find_map(|entry| {
        let p = entry.path();
        if p.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("guids.txt"))
        {
            Some(p)
        } else {
            None
        }
    })
}

/// Extracts car ids referenced by `event:/cars/<id>/...` or `bank:/<id>` lines in `GUIDs.txt`.
pub fn extract_referenced_car_ids(text: &str, out: &mut BTreeSet<String>) {
    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(pos) = lower.find("event:/cars/") {
            let start = pos + "event:/cars/".len();
            let rest = &trimmed[start..];
            if let Some(slash) = rest.find('/') {
                let id = rest[..slash].trim();
                if !id.is_empty() {
                    out.insert(id.to_string());
                }
            }
        } else if let Some(pos) = lower.find("bank:/") {
            let start = pos + "bank:/".len();
            let rest = trimmed[start..].trim();
            // Ignore common/master banks (e.g. `Master Bank`, `common`).
            if !rest.is_empty()
                && !rest.contains('/')
                && !rest.contains(' ')
                && !rest.eq_ignore_ascii_case("common")
            {
                out.insert(rest.to_string());
            }
        }
    }
}

/// Rewrites `GUIDs.txt` content replacing `from_car_id` with `to_car_id` in
/// `event:/cars/<id>/` and `bank:/<id>` references (matching ASCII case-insensitively).
pub fn rewrite_guids(text: &str, from_car_id: &str, to_car_id: &str) -> String {
    let event_needle = format!("event:/cars/{}/", from_car_id.to_ascii_lowercase());
    let event_repl = format!("event:/cars/{to_car_id}/");
    let bank_needle = format!("bank:/{}", from_car_id.to_ascii_lowercase());
    let bank_repl = format!("bank:/{to_car_id}");

    let mut out = String::with_capacity(text.len() + 64);
    for line in text.split_inclusive('\n') {
        let lower = line.to_ascii_lowercase();
        if let Some(pos) = lower.find(&event_needle) {
            out.push_str(&line[..pos]);
            out.push_str(&event_repl);
            out.push_str(&line[pos + event_needle.len()..]);
        } else if let Some(pos) = lower.find(&bank_needle) {
            let after = pos + bank_needle.len();
            let tail = &line[after..];
            let boundary = tail.is_empty() || tail.starts_with(['\r', '\n', ' ', '\t']);
            if boundary {
                out.push_str(&line[..pos]);
                out.push_str(&bank_repl);
                out.push_str(tail);
            } else {
                out.push_str(line);
            }
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Replaces `target_car_id`'s sound with `donor_car_id`'s `.bank` and rewritten `GUIDs.txt`.
///
/// The target's previous `sfx/` folder (if present) is moved to
/// `<target>/sfx.acxs-backup-<stamp>/`. Refuses while `game_running` is true.
pub fn swap_car_sound(
    ac_root: &Path,
    target_car_id: &str,
    donor_car_id: &str,
    game_running: bool,
) -> Result<SfxSwapReport, SfxError> {
    if game_running {
        return Err(SfxError::GameRunning);
    }
    if !is_safe_id(target_car_id) {
        return Err(SfxError::InvalidCarId(target_car_id.to_string()));
    }
    if !is_safe_id(donor_car_id) {
        return Err(SfxError::InvalidCarId(donor_car_id.to_string()));
    }
    if target_car_id.eq_ignore_ascii_case(donor_car_id) {
        return Err(SfxError::SameCar(target_car_id.to_string()));
    }

    let cars_dir = ac_root.join("content").join("cars");
    let target_dir = cars_dir.join(target_car_id);
    let donor_dir = cars_dir.join(donor_car_id);
    if !target_dir.is_dir() {
        return Err(SfxError::CarNotFound(target_dir));
    }
    if !donor_dir.is_dir() {
        return Err(SfxError::CarNotFound(donor_dir));
    }

    let donor_sfx = donor_dir.join("sfx");
    let donor_bank = find_bank_for_car(&donor_sfx, donor_car_id)
        .ok_or_else(|| SfxError::MissingDonorBank(donor_sfx.join(format!("{donor_car_id}.bank"))))?;
    let donor_guids = find_guids_file(&donor_sfx)
        .ok_or_else(|| SfxError::MissingDonorGuids(donor_sfx.join("GUIDs.txt")))?;

    let guids_raw = fs::read_to_string(&donor_guids)?;
    let rewritten_guids = rewrite_guids(&guids_raw, donor_car_id, target_car_id);

    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let target_sfx = target_dir.join("sfx");
    let staging = target_dir.join(format!(".sfx-staging-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&staging)?;

    let staged_bank = staging.join(format!("{target_car_id}.bank"));
    let staged_guids = staging.join("GUIDs.txt");
    if let Err(e) = fs::copy(&donor_bank, &staged_bank).and_then(|_| fs::write(&staged_guids, &rewritten_guids)) {
        let _ = fs::remove_dir_all(&staging);
        return Err(e.into());
    }

    let backup_dir = if target_sfx.exists() {
        let mut candidate = target_dir.join(format!("sfx.acxs-backup-{stamp}"));
        let mut n = 1u32;
        while candidate.exists() {
            candidate = target_dir.join(format!("sfx.acxs-backup-{stamp}-{n}"));
            n += 1;
        }
        if let Err(e) = fs::rename(&target_sfx, &candidate) {
            let _ = fs::remove_dir_all(&staging);
            return Err(e.into());
        }
        Some(candidate)
    } else {
        None
    };

    if let Err(e) = fs::rename(&staging, &target_sfx) {
        let _ = fs::remove_dir_all(&staging);
        if let Some(backup) = &backup_dir {
            let _ = fs::rename(backup, &target_sfx);
        }
        return Err(e.into());
    }

    Ok(SfxSwapReport {
        target_car_id: target_car_id.to_string(),
        donor_car_id: donor_car_id.to_string(),
        bank_written: target_sfx.join(format!("{target_car_id}.bank")),
        guids_written: target_sfx.join("GUIDs.txt"),
        backup_dir,
    })
}

/// Repairs a `sfx/GUIDs.txt` whose car id references do not match the car folder name,
/// backing up the original `GUIDs.txt` first. Returns the backup path if rewritten.
pub fn fix_guids_mismatch(car_dir: &Path, game_running: bool) -> Result<Option<PathBuf>, SfxError> {
    if game_running {
        return Err(SfxError::GameRunning);
    }
    let info = inspect_car_sfx(car_dir);
    if info.car_id.is_empty() {
        return Err(SfxError::InvalidCarId(String::new()));
    }
    let Some(guids_path) = info.guids_path else {
        return Ok(None);
    };
    if info.referenced_car_ids.is_empty()
        || info
            .referenced_car_ids
            .iter()
            .any(|r| r.eq_ignore_ascii_case(&info.car_id))
    {
        return Ok(None);
    }
    let original = fs::read_to_string(&guids_path)?;
    let mut updated = original.clone();
    for old_id in &info.referenced_car_ids {
        updated = rewrite_guids(&updated, old_id, &info.car_id);
    }
    if updated == original {
        return Ok(None);
    }

    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = guids_path.parent().unwrap_or(car_dir);
    let mut backup = dir.join(format!("GUIDs.txt.acxs-backup-{stamp}"));
    let mut n = 1u32;
    while backup.exists() {
        backup = dir.join(format!("GUIDs.txt.acxs-backup-{stamp}-{n}"));
        n += 1;
    }
    fs::rename(&guids_path, &backup)?;
    if let Err(e) = fs::write(&guids_path, updated) {
        let _ = fs::rename(&backup, &guids_path);
        return Err(e.into());
    }
    Ok(Some(backup))
}

fn find_bank_for_car(sfx_dir: &Path, car_id: &str) -> Option<PathBuf> {
    let expected = format!("{car_id}.bank");
    let direct = sfx_dir.join(&expected);
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(sfx_dir).ok()?.flatten().find_map(|entry| {
        let p = entry.path();
        if p.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(&expected))
        {
            Some(p)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);
    impl TempRoot {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!("acxs-sfx-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempRoot(path)
        }
        fn write(&self, rel: &str, body: &[u8]) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
    }
    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const DONOR_GUIDS: &str = "\
{1111} bank:/Master Bank\r\n\
{2222} bank:/donor_car\r\n\
{3333} event:/cars/donor_car/engine_ext\r\n\
{4444} event:/cars/donor_car/engine_int\r\n";

    #[test]
    fn swap_rewrites_guids_and_renames_bank_while_archiving_original_sfx() {
        let root = TempRoot::new("swap-ok");
        root.write("content/cars/donor_car/sfx/donor_car.bank", b"DONOR_BANK_BYTES");
        root.write("content/cars/donor_car/sfx/GUIDs.txt", DONOR_GUIDS.as_bytes());
        root.write("content/cars/target_car/sfx/target_car.bank", b"OLD_BANK");
        root.write("content/cars/target_car/sfx/GUIDs.txt", b"{9999} bank:/target_car\n");

        let report = swap_car_sound(&root.0, "target_car", "donor_car", false).unwrap();
        assert_eq!(report.target_car_id, "target_car");
        assert_eq!(report.donor_car_id, "donor_car");
        assert!(report.bank_written.is_file());
        assert_eq!(fs::read(&report.bank_written).unwrap(), b"DONOR_BANK_BYTES");

        let new_guids = fs::read_to_string(&report.guids_written).unwrap();
        assert!(new_guids.contains("bank:/target_car"));
        assert!(new_guids.contains("event:/cars/target_car/engine_ext"));
        assert!(new_guids.contains("bank:/Master Bank"));
        assert!(!new_guids.contains("donor_car"));

        let backup = report.backup_dir.expect("original sfx archived");
        assert_eq!(fs::read(backup.join("target_car.bank")).unwrap(), b"OLD_BANK");

        let info = inspect_car_sfx(&root.0.join("content/cars/target_car"));
        assert!(info.issues.is_empty(), "unexpected issues: {:?}", info.issues);
    }

    #[test]
    fn swap_refuses_when_game_running_or_donor_bank_missing() {
        let root = TempRoot::new("swap-guards");
        root.write("content/cars/donor_car/sfx/GUIDs.txt", DONOR_GUIDS.as_bytes());
        fs::create_dir_all(root.0.join("content/cars/target_car")).unwrap();

        assert!(matches!(
            swap_car_sound(&root.0, "target_car", "donor_car", true),
            Err(SfxError::GameRunning)
        ));
        assert!(matches!(
            swap_car_sound(&root.0, "target_car", "donor_car", false),
            Err(SfxError::MissingDonorBank(_))
        ));
    }

    #[test]
    fn inspect_and_fix_guids_mismatch() {
        let root = TempRoot::new("guids-fix");
        root.write("content/cars/my_car/sfx/my_car.bank", b"BANK");
        root.write("content/cars/my_car/sfx/GUIDs.txt", DONOR_GUIDS.as_bytes());

        let car_dir = root.0.join("content/cars/my_car");
        let before = inspect_car_sfx(&car_dir);
        assert!(before.issues.iter().any(|i| i.contains("donor_car")));

        let backup = fix_guids_mismatch(&car_dir, false).unwrap().expect("backup created");
        assert!(backup.is_file());

        let after = inspect_car_sfx(&car_dir);
        assert!(after.issues.is_empty(), "issues remained: {:?}", after.issues);
    }
}

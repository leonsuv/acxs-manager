//! Read-only reader for Assetto Corsa `data.acd` archives.
//!
//! Cars and tracks ship their `data/` folder packed as `data.acd`. The layout
//! below describes the archive format as it appears in shipped files:
//!
//! * Optional header: a little-endian `i32` of `-1111`, followed by one more
//!   `i32` that is ignored. Without the marker the archive starts at offset 0.
//! * Entries, back to back until end of buffer:
//!   * name length (`i32` LE), then the name bytes (ASCII),
//!   * plaintext size (`i32` LE, call it `n`),
//!   * `4 * n` bytes of encoded data. Plaintext byte `i` lives at encoded
//!     offset `4 * i`; the three bytes after it are padding.
//! * Encoding: `encoded = (plain + key[i % key.len()]) mod 256`, decoding is the
//!   reverse. The key is the ASCII bytes of the decimal string
//!   `"o1-o2-...-o8"` built from eight octets computed from the folder name
//!   (see [`derive_key`]). The folder name is the parent directory of the file
//!   when the file name starts with `data`, otherwise the file name itself.
//!
//! The key depends on the folder name, which is not stored in the archive. So
//! [`list_entries`] works on bytes alone, while decoding needs the folder name
//! via [`read_entry_in_folder`]. [`read_entry`] returns `Unsupported` for an
//! existing entry because it has no folder name to derive the key from.

use std::fmt;
use std::io;
use std::path::Path;

/// Little-endian `i32` value that marks the optional archive header.
const HEADER_MARKER: i32 = -1111;
/// Upper bound on the number of entries accepted in one archive.
const MAX_ENTRIES: usize = 65_536;
/// Upper bound on the byte length of an entry name.
const MAX_NAME_LEN: usize = 512;

/// One entry of an archive, as listed from its header and length fields.
#[derive(Debug, Clone)]
pub struct AcdEntry {
    /// Entry name, for example `car.ini`.
    pub name: String,
    /// Plaintext size in bytes.
    pub size: u64,
    /// Byte offset of the entry's encoded data in the archive. The encoded
    /// data spans `size * 4` bytes from here.
    pub offset: u64,
}

/// Errors from reading an ACD archive.
#[derive(Debug)]
pub enum AcdError {
    /// Reading the file failed.
    Io(io::Error),
    /// A length field is invalid (negative, or a name longer than the cap).
    BadHeader,
    /// A read would run past the end of the buffer.
    Truncated,
    /// The data is valid but cannot be handled, for example a folder name
    /// that breaks the key derivation, too many entries, or an entry whose
    /// key is unknown.
    Unsupported(String),
    /// No entry with the requested name.
    NotFound(String),
    /// The game is running, so data archives must not change.
    GameRunning,
    /// An entry name could escape the target folder.
    UnsafeEntry(String),
}

impl fmt::Display for AcdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcdError::Io(e) => write!(f, "I/O error reading ACD archive: {e}"),
            AcdError::BadHeader => write!(f, "invalid ACD archive: bad length field"),
            AcdError::Truncated => write!(f, "ACD archive is truncated"),
            AcdError::Unsupported(what) => write!(f, "unsupported ACD data: {what}"),
            AcdError::NotFound(name) => write!(f, "ACD entry not found: {name}"),
            AcdError::GameRunning => {
                write!(f, "Close Assetto Corsa before unpacking or packing data.acd.")
            }
            AcdError::UnsafeEntry(name) => write!(f, "refusing unsafe ACD entry {name:?}"),
        }
    }
}

impl std::error::Error for AcdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AcdError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for AcdError {
    fn from(e: io::Error) -> Self {
        AcdError::Io(e)
    }
}

/// Bounds-checked little-endian cursor over an archive.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Cursor { bytes, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], AcdError> {
        let end = self.pos.checked_add(n).ok_or(AcdError::Truncated)?;
        if end > self.bytes.len() {
            return Err(AcdError::Truncated);
        }
        let slice = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_i32(&mut self) -> Result<i32, AcdError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Reads a length field, rejecting negative values.
    fn read_len(&mut self) -> Result<usize, AcdError> {
        let v = self.read_i32()?;
        usize::try_from(v).map_err(|_| AcdError::BadHeader)
    }
}

/// An entry with its encoded bytes borrowed from the archive.
struct RawEntry<'a> {
    name: String,
    size: usize,
    data_offset: usize,
    encoded: &'a [u8],
}

/// Walks the whole archive and validates every length field against the buffer.
fn walk(bytes: &[u8]) -> Result<Vec<RawEntry<'_>>, AcdError> {
    let mut cur = Cursor::new(bytes);

    // Read one i32 first. If it is the marker, one more i32 is skipped.
    // Otherwise the stream restarts at offset 0.
    if cur.read_i32()? == HEADER_MARKER {
        cur.read_i32()?;
    } else {
        cur.pos = 0;
    }

    let mut entries = Vec::new();
    while cur.pos < bytes.len() {
        if entries.len() >= MAX_ENTRIES {
            return Err(AcdError::Unsupported(format!(
                "more than {MAX_ENTRIES} entries"
            )));
        }

        let name_len = cur.read_len()?;
        if name_len > MAX_NAME_LEN {
            return Err(AcdError::BadHeader);
        }
        let name = String::from_utf8_lossy(cur.take(name_len)?).into_owned();

        let size = cur.read_len()?;
        let encoded_len = size.checked_mul(4).ok_or(AcdError::Truncated)?;
        let data_offset = cur.pos;
        let encoded = cur.take(encoded_len)?;

        entries.push(RawEntry {
            name,
            size,
            data_offset,
            encoded,
        });
    }

    Ok(entries)
}

/// Lists the entries of an archive held in memory.
///
/// Pure and bounds-checked: every length is checked against the buffer, and
/// the entry count and name length are capped.
pub fn list_entries(bytes: &[u8]) -> Result<Vec<AcdEntry>, AcdError> {
    let raw = walk(bytes)?;
    Ok(raw
        .into_iter()
        .map(|e| AcdEntry {
            name: e.name,
            size: e.size as u64,
            offset: e.data_offset as u64,
        })
        .collect())
}

/// Reads and lists the entries of an archive file.
pub fn read_file(path: &Path) -> Result<Vec<AcdEntry>, AcdError> {
    let bytes = std::fs::read(path)?;
    list_entries(&bytes)
}

/// Returns the decoded bytes of one entry.
///
/// The decoding key comes from the car or track folder name, which this
/// function does not receive. An existing entry therefore returns
/// `Unsupported`. Use [`read_entry_in_folder`] to decode it. An unknown name
/// returns `NotFound`.
pub fn read_entry(bytes: &[u8], name: &str) -> Result<Vec<u8>, AcdError> {
    let raw = walk(bytes)?;
    find_entry(&raw, name)?;
    Err(AcdError::Unsupported(
        "entry decoding needs the car/track folder name; use read_entry_in_folder".to_string(),
    ))
}

/// Returns the decoded bytes of one entry, using `folder_name` for the key.
///
/// `folder_name` is the car or track folder that holds the archive.
/// Matching of `name` ignores ASCII case.
pub fn read_entry_in_folder(
    bytes: &[u8],
    folder_name: &str,
    name: &str,
) -> Result<Vec<u8>, AcdError> {
    let raw = walk(bytes)?;
    let entry = find_entry(&raw, name)?;
    let key = derive_key(folder_name)?;
    Ok(decode(entry.encoded, &key))
}

fn find_entry<'r, 'a>(raw: &'r [RawEntry<'a>], name: &str) -> Result<&'r RawEntry<'a>, AcdError> {
    raw.iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| AcdError::NotFound(name.to_string()))
}

/// Decodes encoded data (4 bytes per plaintext byte) with a repeating key.
fn decode(encoded: &[u8], key: &[u8]) -> Vec<u8> {
    // The key is never empty: it always holds eight decimal octets.
    encoded
        .as_chunks::<4>().0.iter()
        .enumerate()
        .map(|(i, c)| c[0].wrapping_sub(key[i % key.len()]))
        .collect()
}

/// Derives the key from a folder name.
///
/// The name is lowercased and measured in UTF-16 code units. Eight octets are
/// computed, each reduced to `0..=255`, and the key is the ASCII bytes of
/// `"o1-o2-...-o8"`. Integer arithmetic wraps at 32 bits. A zero divisor
/// returns `Unsupported`.
fn derive_key(folder_name: &str) -> Result<Vec<u8>, AcdError> {
    let f: Vec<i32> = folder_name
        .to_lowercase()
        .encode_utf16()
        .map(i32::from)
        .collect();
    let n = f.len() as isize;

    let at = |i: isize| -> Result<i32, AcdError> {
        usize::try_from(i)
            .ok()
            .and_then(|i| f.get(i).copied())
            .ok_or_else(|| AcdError::Unsupported("folder name key index out of range".into()))
    };
    let div_err = || AcdError::Unsupported("division by zero in ACD key derivation".into());
    let octet = |v: i32| v.rem_euclid(256) as u8;

    // Octet 1: sum of all characters.
    let o1 = f.iter().fold(0i32, |a, &c| a.wrapping_add(c));

    // Octet 2: pairs (0,1), (2,3), ...
    let mut num: i32 = 0;
    let mut i: isize = 0;
    while i < n - 1 {
        num = num.wrapping_mul(at(i)?).wrapping_sub(at(i + 1)?);
        i += 2;
    }
    let o2 = num;

    // Octet 3: triples starting at index 1.
    let mut num2: i32 = 0;
    let mut j: isize = 1;
    while j < n - 3 {
        let d = at(j + 1)?.wrapping_add(27);
        let q = num2
            .wrapping_mul(at(j)?)
            .checked_div(d)
            .ok_or_else(div_err)?;
        num2 = q.wrapping_sub(27).wrapping_sub(at(j - 1)?);
        j += 3;
    }
    let o3 = num2;

    // Octet 4: 5763 minus every character from index 1 on.
    let mut num3: i32 = 5763;
    for k in 1..n {
        num3 = num3.wrapping_sub(at(k)?);
    }
    let o4 = num3;

    // Octet 5: chained multiply, stepping by 4 from index 1.
    let mut num4: i32 = 66;
    let mut l: isize = 1;
    while l < n - 4 {
        num4 = (at(l)?.wrapping_add(15))
            .wrapping_mul(num4)
            .wrapping_mul(at(l - 1)?.wrapping_add(15))
            .wrapping_add(22);
        l += 4;
    }
    let o5 = num4;

    // Octet 6: 101 minus every even-indexed character.
    let mut num5: i32 = 101;
    let mut m: isize = 0;
    while m < n - 2 {
        num5 = num5.wrapping_sub(at(m)?);
        m += 2;
    }
    let o6 = num5;

    // Octet 7: remainder of 171 by every even-indexed character.
    let mut num6: i32 = 171;
    let mut p: isize = 0;
    while p < n - 2 {
        num6 = num6.checked_rem(at(p)?).ok_or_else(div_err)?;
        p += 2;
    }
    let o7 = num6;

    // Octet 8: num / c[k] + c[k+1], folded over the name.
    let mut num7: i32 = 171;
    for k in 0..n - 1 {
        num7 = num7
            .checked_div(at(k)?)
            .ok_or_else(div_err)?
            .wrapping_add(at(k + 1)?);
    }
    let o8 = num7;

    let key = [o1, o2, o3, o4, o5, o6, o7, o8]
        .iter()
        .map(|&v| octet(v).to_string())
        .collect::<Vec<_>>()
        .join("-");
    Ok(key.into_bytes())
}

/// Encodes plaintext bytes into the 4-byte-per-plaintext-byte ACD representation.
fn encode_bytes(plain: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(plain.len() * 4);
    for (i, &p) in plain.iter().enumerate() {
        out.push(p.wrapping_add(key[i % key.len()]));
        out.extend_from_slice(&[0, 0, 0]);
    }
    out
}

fn is_safe_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.chars().any(|c| c.is_control())
}

fn backup_path_for(path: &Path) -> std::path::PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".to_string());
    let mut candidate = dir.join(format!("{name}.acxs-backup-{stamp}"));
    let mut n = 1u32;
    while candidate.exists() {
        candidate = dir.join(format!("{name}.acxs-backup-{stamp}-{n}"));
        n += 1;
    }
    candidate
}

/// Unpacks `acd_path` into `dest_dir` using `folder_name` to derive the key.
///
/// - Refuses with [`AcdError::GameRunning`] when `game_running` is true.
/// - Rejects any entry name containing path separators, drive colons, `..`, or control chars.
/// - Extracts into a staging folder first and renames into `dest_dir`; if `dest_dir` already
///   exists, it is archived to `<dest_dir>.acxs-backup-<stamp>` (never deleted).
/// - Also archives `acd_path`? No: `acd_path` itself is left untouched so callers (e.g.
///   `car_tuning`) decide whether to archive `data.acd` aside; use [`unpack_car_data`] to
///   unpack and archive `data.acd` in one step.
pub fn unpack_acd(
    acd_path: &Path,
    folder_name: &str,
    dest_dir: &Path,
    game_running: bool,
) -> Result<usize, AcdError> {
    if game_running {
        return Err(AcdError::GameRunning);
    }
    let bytes = std::fs::read(acd_path)?;
    let raw = walk(&bytes)?;
    let key = derive_key(folder_name)?;
    for entry in &raw {
        if !is_safe_entry_name(&entry.name) {
            return Err(AcdError::UnsafeEntry(entry.name.clone()));
        }
    }

    let parent = dest_dir.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".acxs-acd-unpack-{}-{}",
        std::process::id(),
        chrono::Local::now().format("%Y%m%d%H%M%S%3f")
    ));
    std::fs::create_dir_all(&staging)?;

    for entry in &raw {
        let plain = decode(entry.encoded, &key);
        if let Err(e) = std::fs::write(staging.join(&entry.name), &plain) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.into());
        }
    }

    let backup = if std::fs::symlink_metadata(dest_dir).is_ok() {
        let b = backup_path_for(dest_dir);
        if let Err(e) = std::fs::rename(dest_dir, &b) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e.into());
        }
        Some(b)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&staging, dest_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        if let Some(b) = backup {
            let _ = std::fs::rename(b, dest_dir);
        }
        return Err(e.into());
    }
    Ok(raw.len())
}

/// Packs every regular file in `data_dir` (sorted by name, skipping `.acxs-backup-*` files)
/// into `out_acd` using `folder_name` to derive the key.
///
/// - Refuses with [`AcdError::GameRunning`] when `game_running` is true.
/// - If `out_acd` already exists, it is archived to `<out_acd>.acxs-backup-<stamp>` before rename.
pub fn pack_acd(
    data_dir: &Path,
    folder_name: &str,
    out_acd: &Path,
    game_running: bool,
) -> Result<usize, AcdError> {
    if game_running {
        return Err(AcdError::GameRunning);
    }
    let key = derive_key(folder_name)?;
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(".acxs-backup-") || name.starts_with('.') {
            continue;
        }
        if !is_safe_entry_name(&name) || name.len() > MAX_NAME_LEN {
            return Err(AcdError::UnsafeEntry(name));
        }
        files.push((name, path));
    }
    if files.len() > MAX_ENTRIES {
        return Err(AcdError::Unsupported(format!(
            "more than {MAX_ENTRIES} entries"
        )));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = Vec::new();
    out.extend_from_slice(&HEADER_MARKER.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    for (name, path) in &files {
        let plain = std::fs::read(path)?;
        let name_bytes = name.as_bytes();
        let name_len = i32::try_from(name_bytes.len())
            .map_err(|_| AcdError::Unsupported("entry name too long".into()))?;
        let plain_len = i32::try_from(plain.len())
            .map_err(|_| AcdError::Unsupported("entry file too large".into()))?;
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&plain_len.to_le_bytes());
        out.extend_from_slice(&encode_bytes(&plain, &key));
    }

    let parent = out_acd.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".data.acd.tmp-{}-{}",
        std::process::id(),
        chrono::Local::now().format("%Y%m%d%H%M%S%3f")
    ));
    std::fs::write(&tmp, &out)?;

    let backup = if std::fs::symlink_metadata(out_acd).is_ok() {
        let b = backup_path_for(out_acd);
        if let Err(e) = std::fs::rename(out_acd, &b) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Some(b)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&tmp, out_acd) {
        let _ = std::fs::remove_file(&tmp);
        if let Some(b) = backup {
            let _ = std::fs::rename(b, out_acd);
        }
        return Err(e.into());
    }
    Ok(files.len())
}

/// Unpacks `<car_dir>/data.acd` into `<car_dir>/data/` and archives `data.acd` to
/// `data.acd.acxs-backup-<stamp>` so Assetto Corsa (and `car_tuning`) read `data/`.
pub fn unpack_car_data(car_dir: &Path, game_running: bool) -> Result<(usize, std::path::PathBuf), AcdError> {
    if game_running {
        return Err(AcdError::GameRunning);
    }
    let folder = car_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let acd_path = car_dir.join("data.acd");
    let data_dir = car_dir.join("data");
    let count = unpack_acd(&acd_path, &folder, &data_dir, game_running)?;
    let archived_acd = backup_path_for(&acd_path);
    std::fs::rename(&acd_path, &archived_acd)?;
    Ok((count, archived_acd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Encodes plaintext with the archive's byte-per-four scheme.
    fn encode(plain: &[u8], key: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(plain.len() * 4);
        for (i, &p) in plain.iter().enumerate() {
            out.push(p.wrapping_add(key[i % key.len()]));
            out.extend_from_slice(&[0, 0, 0]);
        }
        out
    }

    /// Builds an archive. Returns the bytes and the end offset of each entry.
    fn build(entries: &[(&str, &[u8])], folder: &str, header: bool) -> (Vec<u8>, Vec<usize>) {
        let key = derive_key(folder).unwrap();
        let mut out = Vec::new();
        if header {
            out.extend_from_slice(&HEADER_MARKER.to_le_bytes());
            out.extend_from_slice(&0i32.to_le_bytes());
        }
        let mut ends = vec![out.len()];
        for (name, data) in entries {
            out.extend_from_slice(&(name.len() as i32).to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&(data.len() as i32).to_le_bytes());
            out.extend_from_slice(&encode(data, &key));
            ends.push(out.len());
        }
        (out, ends)
    }

    const FOLDER: &str = "some1_honda_nsx_1991";
    const INI: &[u8] = b"[HEADER]\r\nVERSION=3\r\n";
    const LUT: &[u8] = b"0|0\r\n10|42\r\n";

    #[test]
    fn key_matches_known_vectors() {
        // Single-letter name, worked out by hand from the derivation steps.
        assert_eq!(derive_key("a").unwrap(), b"97-0-0-131-66-101-171-171");
        // Key for a folder whose shipped data.acd decodes to valid INI and LUT text.
        assert_eq!(derive_key(FOLDER).unwrap(), b"57-47-151-189-118-6-7-50");
    }

    #[test]
    fn key_rejects_zero_divisor_without_panic() {
        // A NUL character makes the octet 7 remainder divide by zero.
        assert!(matches!(
            derive_key("\0ab"),
            Err(AcdError::Unsupported(_))
        ));
    }

    #[test]
    fn lists_entries_without_header() {
        let (buf, ends) = build(&[("aero.ini", INI), ("lut.lut", LUT)], FOLDER, false);
        let entries = list_entries(&buf).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "aero.ini");
        assert_eq!(entries[0].size, INI.len() as u64);
        assert_eq!(entries[0].offset, (4 + 8 + 4) as u64);
        assert_eq!(entries[1].name, "lut.lut");
        assert_eq!(entries[1].size, LUT.len() as u64);
        assert_eq!(entries[1].offset, (ends[1] + 4 + 7 + 4) as u64);
    }

    #[test]
    fn lists_entries_with_header() {
        let (buf, _) = build(&[("aero.ini", INI)], FOLDER, true);
        let entries = list_entries(&buf).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "aero.ini");
        assert_eq!(entries[0].offset, (8 + 4 + 8 + 4) as u64);
    }

    #[test]
    fn empty_archive_and_empty_buffer() {
        let (buf, _) = build(&[], FOLDER, true);
        assert!(list_entries(&buf).unwrap().is_empty());
        assert!(matches!(list_entries(&[]), Err(AcdError::Truncated)));
    }

    #[test]
    fn truncated_at_every_prefix_is_an_error_not_a_panic() {
        let (buf, ends) = build(&[("aero.ini", INI), ("lut.lut", LUT)], FOLDER, true);
        for len in 0..buf.len() {
            let prefix = &buf[..len];
            let listed = list_entries(prefix);
            let decoded = read_entry_in_folder(prefix, FOLDER, "lut.lut");
            let _ = read_entry(prefix, "lut.lut");
            if ends.contains(&len) {
                // Ends exactly on an entry boundary: a valid, shorter archive.
                assert!(listed.is_ok(), "boundary {len} should list");
            } else {
                assert!(
                    matches!(listed, Err(AcdError::Truncated)),
                    "prefix {len} should be Truncated, got {listed:?}"
                );
            }
            assert!(decoded.is_err(), "prefix {len} should not decode lut.lut");
        }
        // The full buffer decodes the second entry.
        assert_eq!(
            read_entry_in_folder(&buf, FOLDER, "lut.lut").unwrap(),
            LUT
        );
    }

    #[test]
    fn hostile_length_fields_are_rejected() {
        // Negative name length.
        let neg_name = (-5i32).to_le_bytes();
        assert!(matches!(list_entries(&neg_name), Err(AcdError::BadHeader)));

        // Name longer than the cap.
        let mut long_name = ((MAX_NAME_LEN + 1) as i32).to_le_bytes().to_vec();
        long_name.extend(std::iter::repeat_n(b'a', MAX_NAME_LEN + 1));
        assert!(matches!(list_entries(&long_name), Err(AcdError::BadHeader)));

        // Negative data size.
        let mut neg_size = 1i32.to_le_bytes().to_vec();
        neg_size.push(b'x');
        neg_size.extend_from_slice(&(-1i32).to_le_bytes());
        assert!(matches!(list_entries(&neg_size), Err(AcdError::BadHeader)));

        // Huge data size with almost no data behind it.
        let mut huge = 1i32.to_le_bytes().to_vec();
        huge.push(b'x');
        huge.extend_from_slice(&i32::MAX.to_le_bytes());
        huge.extend_from_slice(&[0, 0, 0, 0]);
        assert!(matches!(list_entries(&huge), Err(AcdError::Truncated)));
    }

    #[test]
    fn too_many_entries_is_capped() {
        // Each empty entry takes 8 bytes: a zero name length and a zero size.
        let buf = vec![0u8; 8 * (MAX_ENTRIES + 1)];
        assert!(matches!(
            list_entries(&buf),
            Err(AcdError::Unsupported(_))
        ));
        // Exactly the cap is accepted.
        let ok = &buf[..8 * MAX_ENTRIES];
        assert_eq!(list_entries(ok).unwrap().len(), MAX_ENTRIES);
    }

    #[test]
    fn decodes_entry_with_folder_key() {
        let (buf, _) = build(&[("aero.ini", INI), ("empty.lut", b"")], FOLDER, false);
        assert_eq!(
            read_entry_in_folder(&buf, FOLDER, "aero.ini").unwrap(),
            INI
        );
        // Name matching ignores ASCII case.
        assert_eq!(
            read_entry_in_folder(&buf, FOLDER, "AERO.INI").unwrap(),
            INI
        );
        assert!(read_entry_in_folder(&buf, FOLDER, "empty.lut")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn read_entry_without_folder_is_unsupported_and_unknown_is_not_found() {
        let (buf, _) = build(&[("aero.ini", INI)], FOLDER, false);
        assert!(matches!(
            read_entry(&buf, "aero.ini"),
            Err(AcdError::Unsupported(_))
        ));
        match read_entry(&buf, "missing.ini") {
            Err(AcdError::NotFound(name)) => assert_eq!(name, "missing.ini"),
            other => panic!("expected NotFound, got {other:?}"),
        }
        assert!(matches!(
            read_entry_in_folder(&buf, FOLDER, "missing.ini"),
            Err(AcdError::NotFound(_))
        ));
    }

    #[test]
    fn read_file_lists_a_temp_archive_and_cleans_up() {
        let (buf, _) = build(&[("aero.ini", INI)], FOLDER, true);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "acxs_acd_test_{}_{nanos}.acd",
            std::process::id()
        ));
        fs::write(&path, &buf).unwrap();
        let result = read_file(&path);
        let _ = fs::remove_file(&path);
        let entries = result.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "aero.ini");
    }

    #[test]
    fn read_file_missing_path_is_io_error() {
        let path = std::env::temp_dir().join("acxs_acd_test_definitely_missing.acd");
        assert!(matches!(read_file(&path), Err(AcdError::Io(_))));
    }

    #[test]
    fn pack_and_unpack_round_trip_preserves_exact_bytes_for_all_files() {
        let root = std::env::temp_dir().join(format!("acxs_acd_rt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src = root.join("src_data");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("car.ini"), b"[BASIC]\r\nTOTALMASS=1250\r\n").unwrap();
        fs::write(src.join("power.lut"), b"1000|120\r\n6000|350\r\n").unwrap();
        fs::write(src.join("car.ini.acxs-backup-20260101-000000"), b"ignored").unwrap();

        let acd_path = root.join("data.acd");
        let packed = pack_acd(&src, FOLDER, &acd_path, false).unwrap();
        assert_eq!(packed, 2);

        // Repacking over an existing data.acd archives the previous archive.
        let packed2 = pack_acd(&src, FOLDER, &acd_path, false).unwrap();
        assert_eq!(packed2, 2);
        let backups: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("data.acd.acxs-backup-"))
            .collect();
        assert_eq!(backups.len(), 1);

        let unpacked_dir = root.join("unpacked");
        let count = unpack_acd(&acd_path, FOLDER, &unpacked_dir, false).unwrap();
        assert_eq!(count, 2);
        assert_eq!(fs::read(unpacked_dir.join("car.ini")).unwrap(), b"[BASIC]\r\nTOTALMASS=1250\r\n");
        assert_eq!(fs::read(unpacked_dir.join("power.lut")).unwrap(), b"1000|120\r\n6000|350\r\n");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unpack_rejects_traversal_and_game_running_and_preserves_existing_dir() {
        let root = std::env::temp_dir().join(format!("acxs_acd_safe_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let (evil_buf, _) = build(&[("../escape.ini", b"nope")], FOLDER, true);
        let evil_acd = root.join("evil.acd");
        fs::write(&evil_acd, &evil_buf).unwrap();
        assert!(matches!(
            unpack_acd(&evil_acd, FOLDER, &root.join("out"), false),
            Err(AcdError::UnsafeEntry(_))
        ));
        assert!(matches!(
            unpack_acd(&evil_acd, FOLDER, &root.join("out"), true),
            Err(AcdError::GameRunning)
        ));
        assert!(matches!(
            pack_acd(&root, FOLDER, &root.join("out.acd"), true),
            Err(AcdError::GameRunning)
        ));

        let (good_buf, _) = build(&[("aero.ini", INI)], FOLDER, true);
        let good_acd = root.join("good.acd");
        fs::write(&good_acd, &good_buf).unwrap();
        let existing = root.join("data");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("old.txt"), b"keep").unwrap();

        unpack_acd(&good_acd, FOLDER, &existing, false).unwrap();
        assert!(existing.join("aero.ini").is_file());
        let backups: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("data.acxs-backup-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(backups[0].path().join("old.txt")).unwrap(), b"keep");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn car_tuning_succeeds_after_acd_unpack_and_repack_preserves_edits() {
        let root = std::env::temp_dir().join(format!("acxs_acd_tune_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let car_dir = root.join("content/cars").join(FOLDER);
        let tmp_data = root.join("tmp_data");
        fs::create_dir_all(&car_dir).unwrap();
        fs::create_dir_all(&tmp_data).unwrap();
        fs::write(tmp_data.join("car.ini"), "[BASIC]\r\nTOTALMASS=1400\r\n").unwrap();
        pack_acd(&tmp_data, FOLDER, &car_dir.join("data.acd"), false).unwrap();

        // Tuning refuses packed car.
        assert!(matches!(
            crate::car_tuning::apply(&car_dir, crate::car_tuning::Tuning::TotalMass(1200.0)),
            Err(crate::car_tuning::TuneError::PackedData)
        ));

        // Unpack + archive data.acd -> tuning succeeds -> repack preserves edited value.
        let (n, archived) = unpack_car_data(&car_dir, false).unwrap();
        assert_eq!(n, 1);
        assert!(archived.is_file());
        crate::car_tuning::apply(&car_dir, crate::car_tuning::Tuning::TotalMass(1200.0)).unwrap();

        pack_acd(&car_dir.join("data"), FOLDER, &car_dir.join("data.acd"), false).unwrap();
        let bytes = fs::read(car_dir.join("data.acd")).unwrap();
        let car_ini = String::from_utf8(read_entry_in_folder(&bytes, FOLDER, "car.ini").unwrap()).unwrap();
        assert!(car_ini.contains("TOTALMASS=1200"));
        let _ = fs::remove_dir_all(&root);
    }
}

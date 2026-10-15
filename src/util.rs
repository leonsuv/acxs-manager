//! Small helpers shared by the core modules.

use std::fs;
use std::io;
use std::path::Path;

/// Recursively copy `src` into `dst` (which must not exist yet).
pub fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// True when every whitespace-separated term of `query` appears in one of `fields`.
/// Matching is case-insensitive. An empty query matches everything.
pub fn matches(query: &str, fields: &[&str]) -> bool {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if terms.is_empty() {
        return true;
    }
    let haystack = fields.join(" ").to_lowercase();
    terms.iter().all(|term| haystack.contains(term.as_str()))
}

/// Human readable byte count, e.g. `121.0 MB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_requires_every_term() {
        assert!(matches("ferrari gt", &["Ferrari 488 GT3", "ks_ferrari"]));
        assert!(!matches("ferrari lotus", &["Ferrari 488 GT3", "ks_ferrari"]));
        assert!(matches("   ", &["anything"]));
    }

    #[test]
    fn human_size_scales() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(121_008_424), "115.4 MB");
    }
}

//! Universal per-section presets with non-destructive history and soft-delete recovery.
//!
//! Generalizes the `src/presets.rs` store pattern so Video, Controls, FFB, Assists, and Weather
//! sections all share identical atomic save, replacement history, and removal recovery.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionPreset<T> {
    pub name: String,
    pub value: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: for<'de2> Deserialize<'de2>"))]
pub struct SectionPresetStore<T> {
    #[serde(default)]
    presets: Vec<SectionPreset<T>>,
    #[serde(default)]
    history: Vec<SectionPreset<T>>,
    #[serde(default)]
    removed: Vec<SectionPreset<T>>,
}

impl<T> Default for SectionPresetStore<T> {
    fn default() -> Self {
        Self {
            presets: Vec::new(),
            history: Vec::new(),
            removed: Vec::new(),
        }
    }
}

impl<T: Serialize + for<'de> Deserialize<'de>> SectionPresetStore<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("invalid preset store: {e}"))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
    }

    pub fn list(&self) -> &[SectionPreset<T>] {
        &self.presets
    }

    pub fn history(&self) -> &[SectionPreset<T>] {
        &self.history
    }

    pub fn removed(&self) -> &[SectionPreset<T>] {
        &self.removed
    }

    pub fn get(&self, name: &str) -> Option<&SectionPreset<T>> {
        let key = name.trim().to_lowercase();
        self.presets.iter().find(|p| p.name.to_lowercase() == key)
    }

    pub fn upsert(&mut self, name: &str, value: T) -> io::Result<()> {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_CHARS {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Preset name must be 1..=64 characters"));
        }
        let key = trimmed.to_lowercase();
        if let Some(idx) = self.presets.iter().position(|p| p.name.to_lowercase() == key) {
            let old = std::mem::replace(
                &mut self.presets[idx],
                SectionPreset {
                    name: trimmed.to_string(),
                    value,
                },
            );
            self.history.push(old);
        } else {
            self.presets.push(SectionPreset {
                name: trimmed.to_string(),
                value,
            });
        }
        self.presets.sort_by_key(|p| p.name.to_lowercase());
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let key = name.trim().to_lowercase();
        if let Some(idx) = self.presets.iter().position(|p| p.name.to_lowercase() == key) {
            let old = self.presets.remove(idx);
            self.removed.push(old);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_preset_history_and_soft_delete() {
        let dir = std::env::temp_dir().join(format!("acxs-sec-presets-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("video.json");

        let mut store: SectionPresetStore<u32> = SectionPresetStore::new();
        store.upsert("VR 90Hz", 90).unwrap();
        store.upsert("VR 90Hz", 120).unwrap();
        store.upsert("Flat 144Hz", 144).unwrap();
        assert_eq!(store.history().len(), 1);
        assert_eq!(store.history()[0].value, 90);
        assert!(store.remove("Flat 144Hz"));
        assert_eq!(store.removed().len(), 1);

        store.save(&path).unwrap();
        let loaded: SectionPresetStore<u32> = SectionPresetStore::load(&path).unwrap();
        assert_eq!(loaded.list().len(), 1);
        assert_eq!(loaded.get("vr 90hz").unwrap().value, 120);
        assert_eq!(loaded.history().len(), 1);
        assert_eq!(loaded.removed().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}

//! User-defined collections of cars and tracks, such as "GT3 favourites" or
//! "Nordschleife cars". Modelled on the collections feature of Content Manager.
//!
//! The whole store is one JSON file, `acxs-collections.json` in the AC root.
//! Items are identified by content id (the folder name) and kind. Deleting a
//! collection does not drop its data: it moves to a `deleted` list in the same
//! file, so a mistaken delete can still be recovered by hand.
//!
//! Collection names are trimmed and compared case-insensitively, so "GT3" and
//! "gt3" are the same collection.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File name of the store inside the AC root.
pub const FILE_NAME: &str = "acxs-collections.json";

/// Longest accepted collection name, in characters.
pub const MAX_NAME_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Car,
    Track,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    pub items: Vec<(ItemKind, String)>,
}

/// All collections, plus the collections that were deleted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollectionStore {
    collections: Vec<Collection>,
    deleted: Vec<Collection>,
}

#[derive(Debug)]
pub enum CollectionError {
    EmptyName,
    NameTooLong { len: usize },
    DuplicateName(String),
    NotFound(String),
    EmptyItemId,
    /// The file exists but is not a valid store. It is never overwritten.
    Corrupt { path: PathBuf, source: serde_json::Error },
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "collection name must not be empty"),
            Self::NameTooLong { len } => write!(
                f,
                "collection name is {len} characters, the maximum is {MAX_NAME_LEN}"
            ),
            Self::DuplicateName(name) => write!(f, "a collection named \"{name}\" already exists"),
            Self::NotFound(name) => write!(f, "no collection named \"{name}\""),
            Self::EmptyItemId => write!(f, "item id must not be empty"),
            Self::Corrupt { path, source } => write!(
                f,
                "collections file {} is corrupt, it was not changed: {source}",
                path.display()
            ),
            Self::Io { path, source } => write!(f, "collections file {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for CollectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Corrupt { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Path of the store for an Assetto Corsa installation.
pub fn default_path(ac_root: &Path) -> PathBuf {
    ac_root.join(FILE_NAME)
}

/// Trims a name and checks its length. Returns the trimmed name.
fn normalize_name(name: &str) -> Result<String, CollectionError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CollectionError::EmptyName);
    }
    let len = trimmed.chars().count();
    if len > MAX_NAME_LEN {
        return Err(CollectionError::NameTooLong { len });
    }
    Ok(trimmed.to_string())
}

impl CollectionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the store from `path`. A missing file gives an empty store. A file
    /// that cannot be parsed gives [`CollectionError::Corrupt`] and is left
    /// untouched.
    pub fn load(path: &Path) -> Result<Self, CollectionError> {
        match fs::read(path) {
            // Parse bytes, not text, so invalid UTF-8 counts as corrupt too.
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| CollectionError::Corrupt {
                path: path.to_path_buf(),
                source,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(source) => Err(CollectionError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Save to `path` atomically: write a temp file next to it, then rename it
    /// over the target. Parent directories are created if needed.
    pub fn save(&self, path: &Path) -> Result<(), CollectionError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|source| CollectionError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        // Plain struct of strings and enums: serialization cannot fail.
        let text = serde_json::to_string_pretty(self).expect("collection store serializes");
        let tmp = path.with_extension("json.tmp");
        let io_err = |source| CollectionError::Io {
            path: path.to_path_buf(),
            source,
        };
        fs::write(&tmp, text).map_err(io_err)?;
        fs::rename(&tmp, path).map_err(|source| {
            let _ = fs::remove_file(&tmp);
            io_err(source)
        })
    }

    /// Active collections, sorted by name, case-insensitively.
    pub fn list(&self) -> Vec<&Collection> {
        let mut list: Vec<&Collection> = self.collections.iter().collect();
        list.sort_by_key(|c| c.name.to_lowercase());
        list
    }

    /// Deleted collections, in the order they were deleted.
    pub fn deleted(&self) -> &[Collection] {
        &self.deleted
    }

    /// Create an empty collection. Rejects empty, over-long and duplicate names.
    pub fn create(&mut self, name: &str) -> Result<(), CollectionError> {
        let name = normalize_name(name)?;
        if self.index_of(&name).is_some() {
            return Err(CollectionError::DuplicateName(name));
        }
        self.collections.push(Collection {
            name,
            items: Vec::new(),
        });
        Ok(())
    }

    /// Rename a collection. The new name follows the same rules as [`create`](Self::create).
    /// Changing only the letter case of the name is allowed.
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), CollectionError> {
        let idx = self
            .index_of(old)
            .ok_or_else(|| CollectionError::NotFound(old.trim().to_string()))?;
        let new = normalize_name(new)?;
        if self.index_of(&new).is_some_and(|other| other != idx) {
            return Err(CollectionError::DuplicateName(new));
        }
        self.collections[idx].name = new;
        Ok(())
    }

    /// Add an item to a collection. Adding an item that is already there does nothing.
    pub fn add_item(&mut self, name: &str, kind: ItemKind, id: &str) -> Result<(), CollectionError> {
        if id.trim().is_empty() {
            return Err(CollectionError::EmptyItemId);
        }
        let idx = self.require(name)?;
        let items = &mut self.collections[idx].items;
        if !items.iter().any(|(k, i)| *k == kind && i == id) {
            items.push((kind, id.to_string()));
        }
        Ok(())
    }

    /// Remove an item from a collection. Removing an item that is not there does nothing.
    pub fn remove_item(&mut self, name: &str, kind: ItemKind, id: &str) -> Result<(), CollectionError> {
        let idx = self.require(name)?;
        self.collections[idx].items.retain(|(k, i)| !(*k == kind && i == id));
        Ok(())
    }

    /// Move a collection to the `deleted` list. Its items are kept.
    pub fn delete_collection(&mut self, name: &str) -> Result<(), CollectionError> {
        let idx = self.require(name)?;
        let collection = self.collections.remove(idx);
        self.deleted.push(collection);
        Ok(())
    }

    /// Active collections that contain the given item, sorted by name.
    pub fn collections_containing(&self, kind: ItemKind, id: &str) -> Vec<&Collection> {
        self.list()
            .into_iter()
            .filter(|c| c.items.iter().any(|(k, i)| *k == kind && i == id))
            .collect()
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        let wanted = name.trim().to_lowercase();
        self.collections
            .iter()
            .position(|c| c.name.to_lowercase() == wanted)
    }

    fn require(&self, name: &str) -> Result<usize, CollectionError> {
        self.index_of(name)
            .ok_or_else(|| CollectionError::NotFound(name.trim().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp directory, removed when dropped (also after a failed assert).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "acxs-collection-test-{}-{}",
                std::process::id(),
                tag
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn create_and_list_sorted_case_insensitive() {
        let mut store = CollectionStore::new();
        store.create("b tracks").unwrap();
        store.create("A cars").unwrap();
        store.create("c misc").unwrap();
        let names: Vec<&str> = store.list().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["A cars", "b tracks", "c misc"]);
    }

    #[test]
    fn create_rejects_empty_names() {
        let mut store = CollectionStore::new();
        assert!(matches!(store.create(""), Err(CollectionError::EmptyName)));
        assert!(matches!(store.create("   "), Err(CollectionError::EmptyName)));
        assert!(store.list().is_empty());
    }

    #[test]
    fn create_rejects_duplicate_names_ignoring_case() {
        let mut store = CollectionStore::new();
        store.create("GT3").unwrap();
        assert!(matches!(
            store.create("gt3"),
            Err(CollectionError::DuplicateName(_))
        ));
        assert!(matches!(
            store.create(" GT3 "),
            Err(CollectionError::DuplicateName(_))
        ));
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn create_rejects_names_over_64_chars() {
        let mut store = CollectionStore::new();
        let ok = "x".repeat(MAX_NAME_LEN);
        let too_long = "x".repeat(MAX_NAME_LEN + 1);
        store.create(&ok).unwrap();
        assert!(matches!(
            store.create(&too_long),
            Err(CollectionError::NameTooLong { len: 65 })
        ));
    }

    #[test]
    fn add_and_remove_items_without_duplicates() {
        let mut store = CollectionStore::new();
        store.create("GT3").unwrap();

        store.add_item("GT3", ItemKind::Car, "ks_ferrari_488_gt3").unwrap();
        store.add_item("GT3", ItemKind::Car, "ks_ferrari_488_gt3").unwrap();
        // Same id but a different kind is a different item.
        store.add_item("GT3", ItemKind::Track, "ks_ferrari_488_gt3").unwrap();
        assert_eq!(store.list()[0].items.len(), 2);

        store.remove_item("GT3", ItemKind::Car, "ks_ferrari_488_gt3").unwrap();
        store.remove_item("GT3", ItemKind::Car, "ks_ferrari_488_gt3").unwrap();
        assert_eq!(
            store.list()[0].items,
            vec![(ItemKind::Track, "ks_ferrari_488_gt3".to_string())]
        );
    }

    #[test]
    fn item_operations_on_missing_collection_fail() {
        let mut store = CollectionStore::new();
        assert!(matches!(
            store.add_item("nope", ItemKind::Car, "a"),
            Err(CollectionError::NotFound(_))
        ));
        assert!(matches!(
            store.remove_item("nope", ItemKind::Car, "a"),
            Err(CollectionError::NotFound(_))
        ));
        assert!(matches!(
            store.delete_collection("nope"),
            Err(CollectionError::NotFound(_))
        ));
    }

    #[test]
    fn empty_item_id_is_rejected() {
        let mut store = CollectionStore::new();
        store.create("GT3").unwrap();
        assert!(matches!(
            store.add_item("GT3", ItemKind::Car, " "),
            Err(CollectionError::EmptyItemId)
        ));
    }

    #[test]
    fn rename_collection() {
        let mut store = CollectionStore::new();
        store.create("GT3").unwrap();
        store.create("Touring").unwrap();

        store.rename("GT3", "GT3 Favourites").unwrap();
        assert_eq!(store.list()[0].name, "GT3 Favourites");

        // Changing only the case of the own name is allowed.
        store.rename("Touring", "touring").unwrap();

        assert!(matches!(
            store.rename("touring", "gt3 favourites"),
            Err(CollectionError::DuplicateName(_))
        ));
        assert!(matches!(
            store.rename("missing", "other"),
            Err(CollectionError::NotFound(_))
        ));
        assert!(matches!(
            store.rename("touring", ""),
            Err(CollectionError::EmptyName)
        ));
    }

    #[test]
    fn delete_keeps_data_in_deleted_list() {
        let mut store = CollectionStore::new();
        store.create("Nordschleife cars").unwrap();
        store.add_item("Nordschleife cars", ItemKind::Car, "bmw_m3_e30").unwrap();

        store.delete_collection("Nordschleife cars").unwrap();

        assert!(store.list().is_empty());
        assert_eq!(store.deleted().len(), 1);
        assert_eq!(store.deleted()[0].name, "Nordschleife cars");
        assert_eq!(
            store.deleted()[0].items,
            vec![(ItemKind::Car, "bmw_m3_e30".to_string())]
        );

        // The deleted name can be used again.
        store.create("Nordschleife cars").unwrap();
    }

    #[test]
    fn collections_containing_finds_matching_collections() {
        let mut store = CollectionStore::new();
        store.create("B").unwrap();
        store.create("A").unwrap();
        store.create("C").unwrap();
        store.add_item("A", ItemKind::Track, "monza").unwrap();
        store.add_item("B", ItemKind::Track, "monza").unwrap();
        store.add_item("C", ItemKind::Car, "monza").unwrap();

        let found: Vec<&str> = store
            .collections_containing(ItemKind::Track, "monza")
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(found, ["A", "B"]);
        assert!(store
            .collections_containing(ItemKind::Track, "spa")
            .is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = TempDir::new("round-trip");
        let path = default_path(dir.path());
        assert_eq!(path, dir.path().join(FILE_NAME));

        let mut store = CollectionStore::new();
        store.create("GT3 favourites").unwrap();
        store.add_item("GT3 favourites", ItemKind::Car, "ks_audi_r8_lms").unwrap();
        store.create("Old").unwrap();
        store.delete_collection("Old").unwrap();
        store.save(&path).unwrap();

        // The temp file must not be left behind.
        assert!(!dir.path().join("acxs-collections.json.tmp").exists());

        let loaded = CollectionStore::load(&path).unwrap();
        assert_eq!(loaded, store);
        assert_eq!(loaded.deleted().len(), 1);
    }

    #[test]
    fn load_missing_file_gives_empty_store() {
        let dir = TempDir::new("missing");
        let store = CollectionStore::load(&default_path(dir.path())).unwrap();
        assert!(store.list().is_empty());
        assert!(store.deleted().is_empty());
    }

    #[test]
    fn corrupt_file_returns_error_and_is_untouched() {
        let dir = TempDir::new("corrupt");
        let path = default_path(dir.path());
        let garbage = b"{ \"collections\": [ this is not json";
        fs::write(&path, garbage).unwrap();

        let result = CollectionStore::load(&path);
        assert!(matches!(result, Err(CollectionError::Corrupt { .. })));
        assert_eq!(fs::read(&path).unwrap(), garbage);
    }
}

//! Finding the Assetto Corsa install across Steam library folders.
//!
//! Steam keeps its library locations in `steamapps/libraryfolders.vdf` and
//! records each installed game in `steamapps/appmanifest_<appid>.acf`. Both
//! are Valve KeyValues text, which [`parse_vdf`] reads. A hand-written
//! `appmanifest_244210.xml` is also accepted, since the manifest's `installdir`
//! is the only thing needed from it.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::settings::is_ac_root;

/// Steam's app id for Assetto Corsa (store page `store.steampowered.com/app/244210`).
const AC_APP_ID: u32 = 244210;

/// The folder Assetto Corsa installs into when no manifest says otherwise.
const AC_DEFAULT_DIR: &str = "assettocorsa";

/// Deeper nesting than this is skipped rather than parsed, so hostile input
/// cannot exhaust the stack. Real library files nest three or four levels.
const MAX_DEPTH: usize = 64;

/// A node in a parsed KeyValues document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VdfValue {
    /// A quoted (or bare) string value.
    Str(String),
    /// A `{ ... }` block, entries in file order. Keys may repeat.
    Map(Vec<(String, VdfValue)>),
}

impl VdfValue {
    /// Follow `path` through nested blocks. Keys match case-insensitively, as
    /// Steam's own reader does (`LibraryFolders` and `libraryfolders` both occur).
    /// With repeated keys the first one wins.
    pub fn get(&self, path: &[&str]) -> Option<&VdfValue> {
        path.iter().try_fold(self, |node, key| match node {
            VdfValue::Map(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(*key))
                .map(|(_, v)| v),
            VdfValue::Str(_) => None,
        })
    }

    /// The string, if this node is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            VdfValue::Str(s) => Some(s),
            VdfValue::Map(_) => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Text(String),
}

/// Split KeyValues text into braces and strings. Never fails: an unterminated
/// quote runs to the end of the input, and `//` starts a comment.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(e @ ('\\' | '"')) => s.push(e),
                            // Unknown escapes keep their backslash, so a stray
                            // `C:\Steam` survives as written.
                            Some(other) => {
                                s.push('\\');
                                s.push(other);
                            }
                            None => s.push('\\'),
                        },
                        other => s.push(other),
                    }
                }
                tokens.push(Token::Text(s));
            }
            other => {
                let mut s = String::from(other);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '"' | '{' | '}') {
                        break;
                    }
                    s.push(next);
                    chars.next();
                }
                tokens.push(Token::Text(s));
            }
        }
    }
    tokens
}

/// Parse KeyValues text into a tree. The top level is always a `Map` of the
/// file's top-level entries. Malformed input is tolerated: stray braces are
/// skipped, a key with no value is dropped, and a missing `}` closes at the end.
pub fn parse_vdf(text: &str) -> VdfValue {
    let tokens = tokenize(text);
    let mut pos = 0;
    VdfValue::Map(parse_entries(&tokens, &mut pos, 0))
}

fn parse_entries(tokens: &[Token], pos: &mut usize, depth: usize) -> Vec<(String, VdfValue)> {
    let mut entries = Vec::new();

    while let Some(token) = tokens.get(*pos) {
        match token {
            Token::Close => {
                if depth == 0 {
                    // Stray `}` at the top level.
                    *pos += 1;
                    continue;
                }
                // Close of the enclosing block: the caller consumes it.
                break;
            }
            Token::Open => {
                // A block with no key in front of it.
                *pos += 1;
            }
            Token::Text(key) => {
                *pos += 1;
                match tokens.get(*pos) {
                    Some(Token::Text(value)) => {
                        entries.push((key.clone(), VdfValue::Str(value.clone())));
                        *pos += 1;
                    }
                    Some(Token::Open) => {
                        *pos += 1;
                        let children = if depth + 1 < MAX_DEPTH {
                            let children = parse_entries(tokens, pos, depth + 1);
                            if matches!(tokens.get(*pos), Some(Token::Close)) {
                                *pos += 1;
                            }
                            children
                        } else {
                            skip_block(tokens, pos);
                            Vec::new()
                        };
                        entries.push((key.clone(), VdfValue::Map(children)));
                    }
                    // A key with nothing after it is dropped. Any `}` that
                    // follows is left for the caller.
                    Some(Token::Close) | None => {}
                }
            }
        }
    }
    entries
}

/// Consume tokens up to and including the `}` that closes the block the caller
/// has just entered.
fn skip_block(tokens: &[Token], pos: &mut usize) {
    let mut open = 1usize;
    while let Some(token) = tokens.get(*pos) {
        *pos += 1;
        match token {
            Token::Open => open += 1,
            Token::Close => {
                open -= 1;
                if open == 0 {
                    break;
                }
            }
            Token::Text(_) => {}
        }
    }
}

/// Every library path named in a parsed `libraryfolders.vdf`.
///
/// Modern files give each library a block with a `"path"` key. Files from
/// before about 2017 use `"LibraryFolders" { "1" "D:\\SteamLibrary" }`, which
/// is read too.
fn library_folder_paths(root: &VdfValue) -> Vec<String> {
    let mut found = Vec::new();
    collect_path_values(root, &mut found);

    if let Some(VdfValue::Map(entries)) = root.get(&["libraryfolders"]) {
        for (key, value) in entries {
            let numeric = !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit());
            if let (true, VdfValue::Str(path)) = (numeric, value) {
                found.push(path.clone());
            }
        }
    }
    found
}

fn collect_path_values(node: &VdfValue, out: &mut Vec<String>) {
    if let VdfValue::Map(entries) = node {
        for (key, value) in entries {
            match value {
                VdfValue::Str(path) if key.eq_ignore_ascii_case("path") => {
                    out.push(path.clone());
                }
                VdfValue::Map(_) => collect_path_values(value, out),
                VdfValue::Str(_) => {}
            }
        }
    }
}

/// The Steam root followed by every library it lists, deduplicated, keeping
/// only directories that exist. A missing or unreadable `libraryfolders.vdf`
/// leaves just the root.
pub fn library_paths(steam_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![steam_root.to_path_buf()];

    let vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = fs::read_to_string(vdf) {
        let root = parse_vdf(&text);
        candidates.extend(library_folder_paths(&root).into_iter().map(PathBuf::from));
    }

    dedup_existing_dirs(candidates)
}

/// Drop paths that are not directories and repeats. Two spellings of one
/// folder count as a repeat when they canonicalize to the same path.
fn dedup_existing_dirs(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|p| p.is_dir())
        .filter(|p| seen.insert(fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
        .collect()
}

/// Every Assetto Corsa install under the given Steam roots. An install counts
/// when `steamapps/common/<installdir>/acs.exe` exists in one of their libraries.
///
/// `<installdir>` comes from `appmanifest_244210`, falling back to `assettocorsa`
/// when no usable manifest is present.
pub fn find_ac_installs(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut installs = Vec::new();

    for root in steam_roots {
        for library in library_paths(root) {
            let steamapps = library.join("steamapps");
            let dir = manifest_installdir(&steamapps).unwrap_or_else(|| AC_DEFAULT_DIR.to_string());
            let candidate = steamapps.join("common").join(dir);
            if is_ac_root(&candidate) {
                installs.push(candidate);
            }
        }
    }
    dedup_existing_dirs(installs)
}

/// The `installdir` from Assetto Corsa's manifest in `steamapps`, if it has a
/// usable one. Tries the `.acf` Steam writes, then a hand-written `.xml`.
fn manifest_installdir(steamapps: &Path) -> Option<String> {
    let stem = format!("appmanifest_{AC_APP_ID}");

    let from_acf = fs::read_to_string(steamapps.join(format!("{stem}.acf")))
        .ok()
        .and_then(|text| {
            let tree = parse_vdf(&text);
            tree.get(&["AppState", "installdir"])
                .and_then(VdfValue::as_str)
                .map(str::to_owned)
        });

    let from_xml = || {
        fs::read_to_string(steamapps.join(format!("{stem}.xml")))
            .ok()
            .and_then(|text| xml_tag(&text, "installdir"))
    };

    from_acf.or_else(from_xml).and_then(|dir| safe_dir_name(&dir))
}

/// The text inside the first `<tag>...</tag>`, trimmed. Bounds are checked, so
/// a missing or truncated tag gives `None`.
fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(open.as_str())? + open.len();
    let end = start + text.get(start..)?.find(close.as_str())?;
    text.get(start..end).map(|s| s.trim().to_string())
}

/// A single folder name, so a manifest cannot point outside `steamapps/common`.
fn safe_dir_name(name: &str) -> Option<String> {
    let name = name.trim();
    let unsafe_name = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(|c| c == '/' || c == '\\');
    (!unsafe_name).then(|| name.to_string())
}

/// Where Steam is installed on this machine, to search by default.
///
/// Windows only. Elsewhere this is empty, since Steam's Linux and macOS layouts
/// are not probed here.
pub fn default_steam_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            PathBuf::from(r"C:\Program Files (x86)\Steam"),
            PathBuf::from(r"C:\Program Files\Steam"),
        ]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic `libraryfolders.vdf` in the current Steam layout.
    const LIBRARYFOLDERS: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"contentid"		"1234567890123456789"
		"totalsize"		"0"
		"update_clean_bytes_tally"		"123456"
		"time_last_update_corruption"		"0"
		"apps"
		{
			"228980"		"123456789"
			"244210"		"9876543210"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"label"		"Games"
		"contentid"		"9876543210987654321"
		"totalsize"		"1000204886016"
		"apps"
		{
			"244210"		"5555555"
			"1245620"		"4444444"
		}
	}
}
"#;

    /// A temp directory with a name unique to this process and test. It is
    /// removed on drop, including when an assertion panics.
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("acxs-steam-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempTree(dir)
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }

        fn write(&self, rel: &str, contents: &str) {
            let file = self.path(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, contents).unwrap();
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Escape a path for a quoted VDF value, so backslashes survive parsing.
    fn vdf_path(p: &Path) -> String {
        p.display().to_string().replace('\\', "\\\\")
    }

    #[test]
    fn parses_realistic_libraryfolders() {
        let tree = parse_vdf(LIBRARYFOLDERS);

        assert_eq!(
            tree.get(&["libraryfolders", "0", "path"]).and_then(VdfValue::as_str),
            Some(r"C:\Program Files (x86)\Steam")
        );
        assert_eq!(
            tree.get(&["libraryfolders", "1", "path"]).and_then(VdfValue::as_str),
            Some(r"D:\SteamLibrary")
        );
        assert_eq!(
            tree.get(&["libraryfolders", "1", "apps", "244210"])
                .and_then(VdfValue::as_str),
            Some("5555555")
        );
        // Keys match case-insensitively.
        assert!(tree.get(&["LibraryFolders", "1", "label"]).is_some());
        assert!(tree.get(&["libraryfolders", "9"]).is_none());

        assert_eq!(
            library_folder_paths(&tree),
            vec![
                r"C:\Program Files (x86)\Steam".to_string(),
                r"D:\SteamLibrary".to_string(),
            ]
        );
    }

    #[test]
    fn unescapes_quotes_and_backslashes_in_paths() {
        let text = r#""libraryfolders" { "0" { "path" "E:\\Games \"Racing\"\\Steam" } }"#;
        let tree = parse_vdf(text);

        assert_eq!(
            tree.get(&["libraryfolders", "0", "path"]).and_then(VdfValue::as_str),
            Some(r#"E:\Games "Racing"\Steam"#)
        );
        assert_eq!(
            library_folder_paths(&tree),
            vec![r#"E:\Games "Racing"\Steam"#.to_string()]
        );
    }

    #[test]
    fn reads_legacy_numeric_library_layout() {
        let text = r#""LibraryFolders"
{
	"TimeNextStatsReport"		"1400000000"
	"1"		"D:\\SteamLibrary"
}"#;
        let tree = parse_vdf(text);
        assert_eq!(library_folder_paths(&tree), vec![r"D:\SteamLibrary".to_string()]);
    }

    #[test]
    fn tolerates_junk_and_deep_nesting() {
        assert_eq!(parse_vdf(""), VdfValue::Map(Vec::new()));
        let _ = parse_vdf("}}} { \"unterminated");
        let _ = parse_vdf("// only a comment\n\"key\"");
        let _ = parse_vdf("\"\\");

        // Far deeper than MAX_DEPTH: must return instead of overflowing.
        let deep = "\"k\" {\n".repeat(10_000);
        let tree = parse_vdf(&deep);
        assert!(matches!(tree, VdfValue::Map(_)));
    }

    #[test]
    fn library_paths_dedupes_and_skips_missing_dirs() {
        let tree = TempTree::new("library_paths");
        let steam = tree.path("Steam");
        let lib2 = tree.path("SteamLibrary Two");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::create_dir_all(lib2.join("steamapps")).unwrap();

        // Entry 0 repeats the Steam root, entry 2 does not exist.
        tree.write(
            "Steam/steamapps/libraryfolders.vdf",
            &format!(
                "\"libraryfolders\" {{\n\"0\" {{ \"path\" \"{}\" }}\n\"1\" {{ \"path\" \"{}\" }}\n\"2\" {{ \"path\" \"{}\" }}\n}}\n",
                vdf_path(&steam),
                vdf_path(&lib2),
                vdf_path(&tree.path("nowhere")),
            ),
        );

        let libs = library_paths(&steam);
        assert_eq!(libs, vec![steam.clone(), lib2.clone()]);
    }

    #[test]
    fn finds_the_ac_install_in_a_second_library() {
        let tree = TempTree::new("find_acf");
        let steam = tree.path("Steam");
        let lib2 = tree.path("SteamLibrary Two");
        fs::create_dir_all(steam.join("steamapps")).unwrap();

        tree.write(
            "Steam/steamapps/libraryfolders.vdf",
            &format!(
                "\"libraryfolders\" {{\n\"0\" {{ \"path\" \"{}\" }}\n\"1\" {{ \"path\" \"{}\" }}\n}}\n",
                vdf_path(&steam),
                vdf_path(&lib2),
            ),
        );
        tree.write(
            "SteamLibrary Two/steamapps/appmanifest_244210.acf",
            "\"AppState\"\n{\n\t\"appid\"\t\t\"244210\"\n\t\"name\"\t\t\"Assetto Corsa\"\n\t\"installdir\"\t\t\"assettocorsa\"\n}\n",
        );
        tree.write("SteamLibrary Two/steamapps/common/assettocorsa/acs.exe", "");

        let installs = find_ac_installs(&[steam.clone(), steam.clone()]);
        assert_eq!(installs, vec![lib2.join("steamapps/common/assettocorsa")]);
    }

    #[test]
    fn reads_installdir_from_xml_manifest() {
        let tree = TempTree::new("find_xml");
        let steam = tree.path("Steam");
        fs::create_dir_all(steam.join("steamapps")).unwrap();

        tree.write(
            "Steam/steamapps/appmanifest_244210.xml",
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<AppState>\n\t<appid>244210</appid>\n\t<installdir>Custom AC Dir</installdir>\n</AppState>\n",
        );
        tree.write("Steam/steamapps/common/Custom AC Dir/acs.exe", "");

        assert_eq!(
            find_ac_installs(&[steam.clone()]),
            vec![steam.join("steamapps/common/Custom AC Dir")]
        );
    }

    #[test]
    fn rejects_installdir_that_escapes_common() {
        let tree = TempTree::new("find_unsafe");
        let steam = tree.path("Steam");
        fs::create_dir_all(steam.join("steamapps")).unwrap();

        tree.write(
            "Steam/steamapps/appmanifest_244210.acf",
            "\"AppState\" { \"installdir\" \"../../evil\" }",
        );
        // Where the traversal would land, if it were followed.
        tree.write("Steam/evil/acs.exe", "");

        assert!(find_ac_installs(&[steam]).is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn default_roots_are_empty_off_windows() {
        assert!(default_steam_roots().is_empty());
    }
}

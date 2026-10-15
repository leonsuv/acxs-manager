//! Start Assetto Corsa through a third-party launcher profile.
//!
//! A [`LaunchProfile`] names an external program together with the arguments,
//! working folder and environment it starts with. Arguments, the working
//! folder, the program path and environment values may contain two
//! placeholders:
//!
//! * `{ac_root}`: the Assetto Corsa install folder,
//! * `{race_ini}`: the race configuration file the caller wrote for the session.
//!
//! [`expand`] fills the placeholders and returns a [`LaunchSpec`]. It does not
//! touch the file system, so it can be tested anywhere. [`spawn`] checks that the
//! program and working folder exist and then starts the process.
//!
//! Profiles are kept in a versioned JSON file. Saving never removes data: when
//! the content changes, the previous file is copied to a numbered `.bak` file
//! first.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// Placeholder for the Assetto Corsa install folder.
pub const AC_ROOT_TOKEN: &str = "{ac_root}";
/// Placeholder for the race configuration file of the session.
pub const RACE_INI_TOKEN: &str = "{race_ini}";

const FORMAT_VERSION: u32 = 1;
const DEFAULT_FILE_NAME: &str = "launch-profiles.json";

/// One external program that starts the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchProfile {
    /// Unique name, compared case-insensitively and ignoring surrounding spaces.
    pub name: String,
    /// The executable to start. Must be an existing file when spawned.
    pub program: PathBuf,
    /// Arguments in order. Each entry is one argument, so paths with spaces need no quoting.
    #[serde(default)]
    pub args: Vec<String>,
    /// Folder the program starts in. When `None`, the folder that holds `program` is used.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Extra environment variables as `(name, value)` pairs.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// A profile with every placeholder filled in, ready for [`spawn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// `None` means the process inherits the current folder.
    pub working_dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

/// Why a profile could not be expanded or validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// A profile name is empty after trimming.
    EmptyName,
    /// Two profiles share a name (case-insensitive).
    DuplicateName(String),
    /// The profile has an empty program path.
    EmptyProgram { profile: String },
    /// A `{...}` token that is not one of the supported placeholders.
    UnknownPlaceholder { profile: String, token: String },
    /// A `{` with no matching `}`.
    UnclosedPlaceholder { profile: String, text: String },
    /// An environment variable name that is empty or contains `=` or NUL.
    InvalidEnvName { profile: String, key: String },
    /// A path that cannot be stored as UTF-8 text in a profile.
    NonUnicodePath(PathBuf),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "a launch profile needs a name"),
            Self::DuplicateName(name) => {
                write!(f, "launch profile name \"{name}\" is used more than once")
            }
            Self::EmptyProgram { profile } => {
                write!(f, "launch profile \"{profile}\" has no program")
            }
            Self::UnknownPlaceholder { profile, token } => write!(
                f,
                "launch profile \"{profile}\" uses unknown placeholder {token}; \
                 supported are {AC_ROOT_TOKEN} and {RACE_INI_TOKEN}"
            ),
            Self::UnclosedPlaceholder { profile, text } => write!(
                f,
                "launch profile \"{profile}\" has an unclosed '{{' in \"{text}\""
            ),
            Self::InvalidEnvName { profile, key } => write!(
                f,
                "launch profile \"{profile}\" has an invalid environment variable name \"{key}\""
            ),
            Self::NonUnicodePath(path) => write!(
                f,
                "path {} is not valid UTF-8 and cannot be used in a launch profile",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Fills in the placeholders of `profile` for one session.
///
/// `ac_root` replaces `{ac_root}` and `race_ini` replaces `{race_ini}`, in the
/// program, every argument, the working folder and every environment value.
/// Substituted text is not scanned again, so a path that contains brace
/// sequences stays as it is. Any other `{...}` token is an error. A `}` with no
/// `{` is kept as a literal character.
///
/// When the profile has no working folder, the folder that holds the program is
/// used. If the program has no parent folder (a bare file name), the result has
/// no working folder and the process inherits the current one.
pub fn expand(
    profile: &LaunchProfile,
    ac_root: &Path,
    race_ini: &Path,
) -> Result<LaunchSpec, LaunchError> {
    let name = profile.name.as_str();
    let values = Values {
        profile: name,
        ac_root: path_text(ac_root)?,
        race_ini: path_text(race_ini)?,
    };

    if profile.program.as_os_str().is_empty() {
        return Err(LaunchError::EmptyProgram {
            profile: name.to_string(),
        });
    }
    let program = PathBuf::from(values.fill(path_text(&profile.program)?)?);

    let working_dir = match &profile.working_dir {
        Some(dir) => Some(PathBuf::from(values.fill(path_text(dir)?)?)),
        None => None,
    }
    .or_else(|| program.parent().map(Path::to_path_buf))
    .filter(|dir| !dir.as_os_str().is_empty());

    let args = profile
        .args
        .iter()
        .map(|arg| values.fill(arg))
        .collect::<Result<Vec<_>, _>>()?;

    let mut env = Vec::with_capacity(profile.env.len());
    for (key, value) in &profile.env {
        if key.is_empty() || key.contains('=') || key.contains('\0') {
            return Err(LaunchError::InvalidEnvName {
                profile: name.to_string(),
                key: key.clone(),
            });
        }
        env.push((key.clone(), values.fill(value)?));
    }

    Ok(LaunchSpec {
        program,
        args,
        working_dir,
        env,
    })
}

/// Starts the process described by `spec`.
///
/// Fails with `NotFound` when the program is not an existing file or when the
/// working folder is set and is not an existing folder. Nothing is started in
/// either case.
pub fn spawn(spec: &LaunchSpec) -> io::Result<Child> {
    if !spec.program.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("launcher program not found: {}", spec.program.display()),
        ));
    }
    if let Some(dir) = &spec.working_dir {
        if !dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("launcher working folder not found: {}", dir.display()),
            ));
        }
    }

    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    command.envs(spec.env.iter().map(|(key, value)| (key, value)));
    if let Some(dir) = &spec.working_dir {
        command.current_dir(dir);
    }
    command.spawn()
}

/// Reads the profiles from `path`. A missing file gives an empty list.
///
/// Fails with `InvalidData` when the file is not valid JSON, was written by a
/// newer format version, or contains an empty or duplicate name. The file is
/// never changed by a failed load.
pub fn load_profiles(path: &Path) -> io::Result<Vec<LaunchProfile>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(with_path(err, path)),
    };

    let file: FileRecord = serde_json::from_str(&text).map_err(|err| {
        invalid_data(path, &format!("not a valid launch profile file: {err}"))
    })?;
    if file.version > FORMAT_VERSION {
        return Err(invalid_data(
            path,
            &format!(
                "written by a newer version (format {}, this program reads {FORMAT_VERSION})",
                file.version
            ),
        ));
    }
    validate_names(&file.profiles).map_err(|err| invalid_data(path, &err.to_string()))?;
    Ok(file.profiles)
}

/// Writes `profiles` to `path`.
///
/// Names are validated first, so an invalid list changes nothing. If the file
/// already exists with different content, it is copied to `<file>.bak`, or to
/// `<file>.bak.<n>` when that name is taken, before it is replaced. Identical
/// content is not written again and creates no backup. The new content goes to
/// a temporary file that is renamed over `path`.
pub fn save_profiles(path: &Path, profiles: &[LaunchProfile]) -> io::Result<()> {
    validate_names(profiles)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;

    let file = FileRecord {
        version: FORMAT_VERSION,
        profiles: profiles.to_vec(),
    };
    let mut text = serde_json::to_string_pretty(&file).map_err(io::Error::other)?;
    text.push('\n');

    if path.is_file() {
        let current = fs::read_to_string(path).map_err(|err| with_path(err, path))?;
        if current == text {
            return Ok(());
        }
        let backup = next_backup_path(path);
        fs::copy(path, &backup).map_err(|err| with_path(err, &backup))?;
    }

    let tmp = sibling(path, &format!(".tmp-{}", std::process::id()));
    let result = write_synced(&tmp, text.as_bytes()).and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Serde mirror of the file layout.
#[derive(Serialize, Deserialize)]
struct FileRecord {
    version: u32,
    profiles: Vec<LaunchProfile>,
}

/// Fills placeholders in a single string.
struct Values<'a> {
    profile: &'a str,
    ac_root: &'a str,
    race_ini: &'a str,
}

impl Values<'_> {
    fn fill(&self, text: &str) -> Result<String, LaunchError> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let from_open = &rest[open..];
            let close = from_open
                .find('}')
                .ok_or_else(|| LaunchError::UnclosedPlaceholder {
                    profile: self.profile.to_string(),
                    text: text.to_string(),
                })?;
            let token = &from_open[..=close];
            out.push_str(match token {
                AC_ROOT_TOKEN => self.ac_root,
                RACE_INI_TOKEN => self.race_ini,
                other => {
                    return Err(LaunchError::UnknownPlaceholder {
                        profile: self.profile.to_string(),
                        token: other.to_string(),
                    });
                }
            });
            rest = &from_open[close + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }
}

fn path_text(path: &Path) -> Result<&str, LaunchError> {
    path.to_str()
        .ok_or_else(|| LaunchError::NonUnicodePath(path.to_path_buf()))
}

/// Checks that names are non-empty and unique, ignoring case and surrounding spaces.
fn validate_names(profiles: &[LaunchProfile]) -> Result<(), LaunchError> {
    let mut seen = HashSet::new();
    for profile in profiles {
        let name = profile.name.trim();
        if name.is_empty() {
            return Err(LaunchError::EmptyName);
        }
        if !seen.insert(name.to_lowercase()) {
            return Err(LaunchError::DuplicateName(name.to_string()));
        }
    }
    Ok(())
}

/// `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path
        .file_name()
        .unwrap_or(OsStr::new(DEFAULT_FILE_NAME))
        .to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// The first of `<file>.bak`, `<file>.bak.1`, ... that does not exist yet.
fn next_backup_path(path: &Path) -> PathBuf {
    let first = sibling(path, ".bak");
    if !first.exists() {
        return first;
    }
    let mut n = 1;
    loop {
        let candidate = sibling(path, &format!(".bak.{n}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn invalid_data(path: &Path, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}: {detail}", path.display()),
    )
}

fn with_path(err: io::Error, path: &Path) -> io::Error {
    io::Error::new(err.kind(), format!("{}: {err}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder unique to this process and test name, removed when dropped.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("acxs-launcher-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create scratch dir");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profile(name: &str, program: &str) -> LaunchProfile {
        LaunchProfile {
            name: name.to_string(),
            program: PathBuf::from(program),
            args: Vec::new(),
            working_dir: None,
            env: Vec::new(),
        }
    }

    #[test]
    fn expands_ac_root_and_race_ini_in_args() {
        let mut p = profile("Launcher", "/opt/launcher/start.exe");
        p.args = vec![
            "--game".into(),
            "{ac_root}/acs.exe".into(),
            "--race={race_ini}".into(),
        ];
        let spec = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/cfg/race.ini"))
            .expect("expands");
        assert_eq!(spec.program, PathBuf::from("/opt/launcher/start.exe"));
        assert_eq!(
            spec.args,
            vec![
                "--game",
                "/games/AC/acs.exe",
                "--race=/games/AC/cfg/race.ini",
            ]
        );
    }

    #[test]
    fn expands_placeholders_in_program_working_dir_and_env() {
        let mut p = profile("Wrapper", "{ac_root}/tools/wrap.exe");
        p.working_dir = Some(PathBuf::from("{ac_root}/tools"));
        p.env = vec![("RACE_FILE".into(), "{race_ini}".into())];
        let spec = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/cfg/race.ini"))
            .expect("expands");
        assert_eq!(spec.program, PathBuf::from("/games/AC/tools/wrap.exe"));
        assert_eq!(spec.working_dir, Some(PathBuf::from("/games/AC/tools")));
        assert_eq!(
            spec.env,
            vec![("RACE_FILE".to_string(), "/games/AC/cfg/race.ini".to_string())]
        );
    }

    #[test]
    fn working_dir_defaults_to_the_program_folder() {
        let p = profile("Launcher", "/opt/launcher/start.exe");
        let spec = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/cfg/race.ini"))
            .expect("expands");
        assert_eq!(spec.working_dir, Some(PathBuf::from("/opt/launcher")));
    }

    #[test]
    fn bare_program_name_has_no_default_working_dir() {
        let p = profile("Bare", "launcher.exe");
        let spec = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/cfg/race.ini"))
            .expect("expands");
        assert_eq!(spec.working_dir, None);
    }

    #[test]
    fn substituted_values_are_not_expanded_again() {
        let mut p = profile("Literal", "/x/start.exe");
        p.args = vec!["{ac_root}".into()];
        let spec = expand(
            &p,
            Path::new("/games/{race_ini}"),
            Path::new("/games/AC/cfg/race.ini"),
        )
        .expect("expands");
        assert_eq!(spec.args, vec!["/games/{race_ini}"]);
    }

    #[test]
    fn stray_closing_brace_is_literal() {
        let mut p = profile("Brace", "/x/start.exe");
        p.args = vec!["a}b".into()];
        let spec = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/race.ini"))
            .expect("expands");
        assert_eq!(spec.args, vec!["a}b"]);
    }

    #[test]
    fn unknown_placeholder_is_an_error() {
        let mut p = profile("Typo", "/x/start.exe");
        p.args = vec!["{race}".into()];
        let err = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/race.ini"))
            .expect_err("unknown token");
        assert_eq!(
            err,
            LaunchError::UnknownPlaceholder {
                profile: "Typo".into(),
                token: "{race}".into(),
            }
        );
    }

    #[test]
    fn unclosed_placeholder_is_an_error() {
        let mut p = profile("Open", "/x/start.exe");
        p.args = vec!["--race={race_ini".into()];
        let err = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/race.ini"))
            .expect_err("unclosed");
        assert!(matches!(err, LaunchError::UnclosedPlaceholder { .. }), "{err:?}");
    }

    #[test]
    fn empty_program_is_an_error() {
        let p = profile("Nothing", "");
        let err = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/race.ini"))
            .expect_err("empty program");
        assert_eq!(
            err,
            LaunchError::EmptyProgram {
                profile: "Nothing".into()
            }
        );
    }

    #[test]
    fn invalid_env_names_are_errors() {
        for key in ["", "A=B", "A\0B"] {
            let mut p = profile("Env", "/x/start.exe");
            p.env = vec![(key.into(), "v".into())];
            let err = expand(&p, Path::new("/games/AC"), Path::new("/games/AC/race.ini"))
                .expect_err("bad key");
            assert!(matches!(err, LaunchError::InvalidEnvName { .. }), "{key:?}");
        }
    }

    #[test]
    fn names_must_be_unique_ignoring_case_and_spaces() {
        let profiles = vec![profile("Race Box", "/a"), profile(" race box ", "/b")];
        assert_eq!(
            validate_names(&profiles),
            Err(LaunchError::DuplicateName("race box".into()))
        );
        assert_eq!(validate_names(&[profile("   ", "/a")]), Err(LaunchError::EmptyName));
        assert_eq!(validate_names(&[profile("One", "/a"), profile("Two", "/b")]), Ok(()));
    }

    #[test]
    fn missing_file_loads_as_no_profiles() {
        let dir = ScratchDir::new("missing");
        let profiles = load_profiles(&dir.path().join("launch-profiles.json")).expect("loads");
        assert!(profiles.is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = ScratchDir::new("round_trip");
        let path = dir.path().join("launch-profiles.json");
        let mut wrapper = profile("Wrapper", "{ac_root}/wrap.exe");
        wrapper.args = vec!["--race".into(), "{race_ini}".into()];
        wrapper.working_dir = Some(PathBuf::from("{ac_root}"));
        wrapper.env = vec![("MODE".into(), "race".into())];
        let profiles = vec![profile("Plain", "/opt/start.exe"), wrapper];

        save_profiles(&path, &profiles).expect("saves");
        assert_eq!(load_profiles(&path).expect("loads"), profiles);
    }

    #[test]
    fn optional_fields_may_be_left_out_of_the_file() {
        let dir = ScratchDir::new("optional");
        let path = dir.path().join("launch-profiles.json");
        fs::write(
            &path,
            r#"{"version":1,"profiles":[{"name":"Min","program":"/opt/start.exe"}]}"#,
        )
        .expect("write fixture");
        let profiles = load_profiles(&path).expect("loads");
        assert_eq!(profiles, vec![profile("Min", "/opt/start.exe")]);
    }

    #[test]
    fn saving_keeps_the_previous_version_as_a_backup() {
        let dir = ScratchDir::new("backup");
        let path = dir.path().join("launch-profiles.json");
        save_profiles(&path, &[profile("First", "/a")]).expect("first save");
        let first_text = fs::read_to_string(&path).expect("read first");

        save_profiles(&path, &[profile("Second", "/b")]).expect("second save");
        let bak = dir.path().join("launch-profiles.json.bak");
        assert_eq!(fs::read_to_string(&bak).expect("backup exists"), first_text);

        save_profiles(&path, &[profile("Third", "/c")]).expect("third save");
        let bak_1 = dir.path().join("launch-profiles.json.bak.1");
        assert!(bak_1.is_file(), "second backup gets a numbered name");
        assert!(bak.is_file(), "first backup is not overwritten");
        assert_eq!(load_profiles(&path).expect("loads")[0].name, "Third");
    }

    #[test]
    fn saving_identical_content_writes_no_backup() {
        let dir = ScratchDir::new("identical");
        let path = dir.path().join("launch-profiles.json");
        let profiles = vec![profile("Same", "/a")];
        save_profiles(&path, &profiles).expect("first save");
        save_profiles(&path, &profiles).expect("second save");
        assert!(!dir.path().join("launch-profiles.json.bak").exists());
    }

    #[test]
    fn invalid_list_is_refused_and_file_is_untouched() {
        let dir = ScratchDir::new("refused");
        let path = dir.path().join("launch-profiles.json");
        save_profiles(&path, &[profile("Keep", "/a")]).expect("save");
        let before = fs::read_to_string(&path).expect("read");

        let err = save_profiles(&path, &[profile("Dup", "/a"), profile("dup", "/b")])
            .expect_err("duplicate names");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read_to_string(&path).expect("read"), before);
        assert!(!dir.path().join("launch-profiles.json.bak").exists());
    }

    #[test]
    fn load_rejects_newer_format_and_duplicate_names() {
        let dir = ScratchDir::new("bad_files");
        let newer = dir.path().join("newer.json");
        fs::write(&newer, r#"{"version":2,"profiles":[]}"#).expect("write");
        assert_eq!(
            load_profiles(&newer).expect_err("newer").kind(),
            io::ErrorKind::InvalidData
        );

        let dup = dir.path().join("dup.json");
        fs::write(
            &dup,
            r#"{"version":1,"profiles":[{"name":"A","program":"/a"},{"name":"a","program":"/b"}]}"#,
        )
        .expect("write");
        assert_eq!(
            load_profiles(&dup).expect_err("duplicate").kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn load_rejects_invalid_json() {
        let dir = ScratchDir::new("garbage");
        let path = dir.path().join("launch-profiles.json");
        fs::write(&path, "not json").expect("write");
        assert_eq!(
            load_profiles(&path).expect_err("garbage").kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn spawn_refuses_a_missing_program() {
        let dir = ScratchDir::new("no_program");
        let spec = LaunchSpec {
            program: dir.path().join("missing.exe"),
            args: Vec::new(),
            working_dir: None,
            env: Vec::new(),
        };
        assert_eq!(spawn(&spec).expect_err("missing").kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn spawn_refuses_a_missing_working_dir() {
        let dir = ScratchDir::new("no_dir");
        let spec = LaunchSpec {
            program: PathBuf::from("/bin/sh"),
            args: Vec::new(),
            working_dir: Some(dir.path().join("gone")),
            env: Vec::new(),
        };
        assert_eq!(spawn(&spec).expect_err("missing dir").kind(), io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn spawn_passes_args_env_and_working_dir() {
        let dir = ScratchDir::new("spawn");
        let race_ini = dir.path().join("race.ini");
        let spec = LaunchSpec {
            program: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".into(),
                "printf '%s\\n' \"$1\" \"$ACXS_LAUNCH_TEST\" > out.txt; pwd -P >> out.txt".into(),
                "sh".into(),
                race_ini.to_string_lossy().into_owned(),
            ],
            working_dir: Some(dir.path().to_path_buf()),
            env: vec![("ACXS_LAUNCH_TEST".into(), "value".into())],
        };
        let mut child = spawn(&spec).expect("spawns");
        assert!(child.wait().expect("waits").success());

        let out = fs::read_to_string(dir.path().join("out.txt")).expect("child wrote output");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], race_ini.to_string_lossy());
        assert_eq!(lines[1], "value");
        let expected = fs::canonicalize(dir.path()).expect("canonical");
        assert_eq!(PathBuf::from(lines[2]), expected);
    }
}

//! The game's own settings beyond video: audio, force feedback, gameplay, the post-processing
//! filter and Python apps. The options are a table of real keys from the game's config files, so
//! the page stays one editor and a value the table does not name is left as it is.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::ini::Ini;

/// How an option is edited.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    Toggle,
    /// `min`, `max`, `percent`: shown as a percentage of the stored value when true.
    Slider(f64, f64, bool),
}

/// One setting in a config file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameOption {
    pub group: &'static str,
    pub file: &'static str,
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub control: Control,
}

const fn level(section: &'static str, key: &'static str, label: &'static str) -> GameOption {
    GameOption { group: "Audio", file: "audio.ini", section, key, label, control: Control::Slider(0.0, 1.0, true) }
}

const fn ffb(section: &'static str, key: &'static str, label: &'static str, max: f64) -> GameOption {
    GameOption { group: "Force feedback", file: "controls.ini", section, key, label, control: Control::Slider(0.0, max, true) }
}

const fn play(section: &'static str, key: &'static str, label: &'static str) -> GameOption {
    GameOption { group: "Gameplay", file: "gameplay.ini", section, key, label, control: Control::Toggle }
}

pub const OPTIONS: &[GameOption] = &[
    level("LEVELS", "MASTER", "Master"),
    level("LEVELS", "ENGINE", "Engine"),
    level("LEVELS", "SURFACES", "Surfaces"),
    level("LEVELS", "TYRES", "Tyres"),
    level("LEVELS", "BRAKES", "Brakes"),
    level("LEVELS", "WIND", "Wind"),
    level("LEVELS", "OPPONENTS", "Opponents"),
    level("LEVELS", "DIRT_BOTTOM", "Dirt under the car"),
    level("LEVELS", "UISOUNDS", "Interface"),
    level("LEVELS", "MUSIC", "Music"),
    level("LEVELS_EXT", "WEATHER", "Weather"),
    level("LEVELS_EXT", "TRACK", "Track"),
    level("LEVELS_EXT", "WIPERS", "Wipers"),
    level("LEVELS_EXT", "CAR_COMPONENTS", "Car parts"),
    GameOption {
        group: "Force feedback",
        file: "controls.ini",
        section: "STEER",
        key: "LOCK",
        label: "Wheel rotation (degrees)",
        control: Control::Slider(180.0, 1800.0, false),
    },
    ffb("STEER", "FF_GAIN", "Force feedback gain", 2.0),
    ffb("STEER", "FILTER_FF", "Filter", 1.0),
    ffb("FF_TWEAKS", "MIN_FF", "Minimum force", 0.3),
    ffb("FF_TWEAKS", "CENTER_BOOST_GAIN", "Centre boost", 2.0),
    ffb("FF_TWEAKS", "CENTER_BOOST_RANGE", "Centre boost range", 1.0),
    ffb("FF_ENHANCEMENT", "CURBS", "Kerb effect", 1.0),
    ffb("FF_ENHANCEMENT", "ROAD", "Road effect", 1.0),
    ffb("FF_ENHANCEMENT", "SLIPS", "Slip effect", 1.0),
    ffb("FF_ENHANCEMENT", "ABS", "ABS effect", 1.0),
    GameOption {
        group: "Force feedback",
        file: "controls.ini",
        section: "FF_ENHANCEMENT_2",
        key: "UNDERSTEER",
        label: "Lighten the wheel when understeering",
        control: Control::Toggle,
    },
    GameOption {
        group: "Force feedback",
        file: "controls.ini",
        section: "FF_SKIP_STEPS",
        key: "VALUE",
        label: "Skip physics steps (lowers the wheel's load)",
        control: Control::Slider(0.0, 8.0, false),
    },
    play("OPTIONS", "USE_MPH", "Use miles per hour"),
    play("TIME_DIFFERENCE", "IS_ACTIVE", "Show the time difference"),
    play("GUI", "SHOW_SECTORS", "Show sector times"),
    play("DRIVER_NAME_DISPLAYER", "IS_ACTIVE", "Show driver names"),
    play("VIRTUAL_MIRROR", "ACTIVE", "Virtual mirror"),
    play("OVERLAY_LEADERBOARD", "ACTIVE", "Leaderboard overlay"),
    play("OVERLAY_LEADERBOARD", "VERTICAL", "Vertical leaderboard"),
    play("PIT_INDICATOR", "ONLY_IN_PITLINE", "Pit indicator only in the pit lane"),
    play("DOWNSHIFT_PROTECTION_NOTIFICATION", "ACTIVE", "Tell me about downshift protection"),
    play("PYTHON", "ENABLE_PYTHON", "Python apps"),
];

/// The groups in page order.
pub const GROUPS: [&str; 3] = ["Audio", "Force feedback", "Gameplay"];

/// The value of each option in [`OPTIONS`], `None` when the file does not have it.
pub fn load(cfg_dir: &Path) -> Vec<Option<f64>> {
    let files: Vec<(&str, Option<Ini>)> = OPTIONS
        .iter()
        .map(|o| o.file)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|file| (file, Ini::load(&cfg_dir.join(file)).ok()))
        .collect();
    OPTIONS
        .iter()
        .map(|option| {
            let ini = files.iter().find(|(name, _)| *name == option.file)?.1.as_ref()?;
            ini.get_f64(option.section, option.key)
        })
        .collect()
}

/// Writes the values that changed between `old` and `new`, one backup per changed file. Returns
/// how many values were written.
pub fn save(cfg_dir: &Path, old: &[Option<f64>], new: &[Option<f64>]) -> io::Result<usize> {
    let changed: Vec<(&GameOption, f64)> = OPTIONS
        .iter()
        .zip(old.iter().zip(new))
        .filter_map(|(option, (before, after))| match after {
            Some(value) if before != after => Some((option, *value)),
            _ => None,
        })
        .collect();
    let files: BTreeSet<&str> = changed.iter().map(|(option, _)| option.file).collect();
    for file in files {
        let path = cfg_dir.join(file);
        let mut ini = Ini::load(&path)?;
        for (option, value) in changed.iter().filter(|(option, _)| option.file == file) {
            ini.set(option.section, option.key, value);
        }
        ini.save_with_backup(&path)?;
    }
    Ok(changed.len())
}

/// The post-processing filters the game ships plus the player's own, sorted.
pub fn pp_filters(ac_root: &Path, cfg_dir: &Path) -> Vec<String> {
    let mut names = BTreeSet::new();
    for dir in [ac_root.join("system").join("cfg").join("ppfilters"), cfg_dir.join("ppfilters")] {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for path in entries.filter_map(Result::ok).map(|e| e.path()) {
            if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("ini"))
                && let Some(stem) = path.file_stem()
            {
                names.insert(stem.to_string_lossy().into_owned());
            }
        }
    }
    names.into_iter().collect()
}

/// `[POST_PROCESS] FILTER` of `video.ini`.
pub fn active_filter(cfg_dir: &Path) -> Option<String> {
    Ini::load(&cfg_dir.join("video.ini")).ok()?.get("POST_PROCESS", "FILTER").map(|f| f.trim().to_string())
}

pub fn set_filter(cfg_dir: &Path, filter: &str) -> io::Result<()> {
    let path = cfg_dir.join("video.ini");
    let mut ini = Ini::load(&path)?;
    ini.set("POST_PROCESS", "FILTER", filter);
    ini.save_with_backup(&path)
}

/// One Python app and whether the game loads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonApp {
    pub id: String,
    pub active: bool,
}

/// The apps in `apps/python`, without the game's own `system` folder. `python.ini` keeps one
/// section per app in capitals.
pub fn python_apps(ac_root: &Path, cfg_dir: &Path) -> Vec<PythonApp> {
    let ini = Ini::load(&cfg_dir.join("python.ini")).ok();
    let Ok(entries) = std::fs::read_dir(ac_root.join("apps").join("python")) else { return Vec::new() };
    let mut apps: Vec<PythonApp> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|id| !id.eq_ignore_ascii_case("system"))
        .map(|id| {
            let active = ini.as_ref().and_then(|i| i.get_i64(&id.to_uppercase(), "ACTIVE")) == Some(1);
            PythonApp { id, active }
        })
        .collect();
    apps.sort_by_key(|app| app.id.to_lowercase());
    apps
}

pub fn set_python_app(cfg_dir: &Path, id: &str, active: bool) -> io::Result<()> {
    let path = cfg_dir.join("python.ini");
    let mut ini = Ini::load(&path).unwrap_or_else(|_| Ini::parse(""));
    ini.set(&id.to_uppercase(), "ACTIVE", i32::from(active));
    ini.save_with_backup(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("acxs-options-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ppfilters")).unwrap();
        std::fs::write(dir.join("audio.ini"), "[HEADER]\nVERSION=1\n\n[LEVELS]\nMASTER=1\nENGINE=0.5\n").unwrap();
        std::fs::write(dir.join("video.ini"), "[POST_PROCESS]\nDOF=2\nFILTER=movie\n").unwrap();
        std::fs::write(dir.join("python.ini"), "[CHAT]\nACTIVE=1\n\n[GMETER]\nACTIVE=0\n").unwrap();
        dir
    }

    fn index(key: &str) -> usize {
        OPTIONS.iter().position(|o| o.key == key).unwrap()
    }

    #[test]
    fn only_changed_values_are_written_and_the_rest_of_the_file_stays() {
        let dir = cfg("save");
        let old = load(&dir);
        assert_eq!(old[index("MASTER")], Some(1.0));
        assert_eq!(old[index("MUSIC")], None, "a key the file lacks stays unknown");
        let mut new = old.clone();
        new[index("ENGINE")] = Some(0.8);
        assert_eq!(save(&dir, &old, &new).unwrap(), 1);
        let again = Ini::load(&dir.join("audio.ini")).unwrap();
        assert_eq!(again.get_f64("LEVELS", "ENGINE"), Some(0.8));
        assert_eq!(again.get_f64("LEVELS", "MASTER"), Some(1.0));
        assert_eq!(again.get_i64("HEADER", "VERSION"), Some(1));
        assert_eq!(save(&dir, &new, &new).unwrap(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filters_merge_both_folders_and_the_choice_keeps_the_section() {
        let dir = cfg("pp");
        let root = dir.join("root");
        std::fs::create_dir_all(root.join("system/cfg/ppfilters")).unwrap();
        std::fs::write(root.join("system/cfg/ppfilters/movie.ini"), "").unwrap();
        std::fs::write(dir.join("ppfilters/mine.ini"), "").unwrap();
        assert_eq!(pp_filters(&root, &dir), ["mine", "movie"]);
        assert_eq!(active_filter(&dir).as_deref(), Some("movie"));
        set_filter(&dir, "mine").unwrap();
        assert_eq!(active_filter(&dir).as_deref(), Some("mine"));
        assert_eq!(Ini::load(&dir.join("video.ini")).unwrap().get_i64("POST_PROCESS", "DOF"), Some(2));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn python_apps_follow_the_capital_sections_and_skip_system() {
        let dir = cfg("py");
        let root = dir.join("root");
        for app in ["Chat", "gMeter", "system"] {
            std::fs::create_dir_all(root.join("apps/python").join(app)).unwrap();
        }
        let apps = python_apps(&root, &dir);
        assert_eq!(apps, [PythonApp { id: "Chat".into(), active: true }, PythonApp { id: "gMeter".into(), active: false }]);
        set_python_app(&dir, "gMeter", true).unwrap();
        assert!(python_apps(&root, &dir)[1].active);
        std::fs::remove_dir_all(&dir).ok();
    }
}

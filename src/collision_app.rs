//! The ACXS Collisions app: a tiny CSP Lua app that tells wall hits from car hits.
//!
//! Shared memory cannot say what a car hit; Custom Shaders Patch can. The app listens for
//! `ac.onCarCollision` and keeps the recent hits in `apps/lua/ACXSCollisions/hits.log`, inside its
//! own folder because that is where CSP apps are known to be able to write with `io.save`. The
//! recorder reads that file after a session. Installing the app needs the game closed.

use std::path::{Path, PathBuf};

use crate::lua_apps::{self, AppError, AppScope};

/// Folder name, which is also the app id.
pub const APP_ID: &str = "ACXSCollisions";
/// The log the app writes in its folder.
pub const LOG_FILE: &str = "hits.log";

/// `LAZY = NONE` loads the script when the game loads, with no window open, so hits are logged
/// without any step in the game.
const MANIFEST: &str = "[ABOUT]
NAME = ACXS Collisions
AUTHOR = ACXS Manager
VERSION = 1.1
DESCRIPTION = Logs wall and car hits for ACXS Manager statistics.

[CORE]
LAZY = NONE

[WINDOW_...]
ID = main
NAME = ACXS Collisions
ICON = icon.png
FUNCTION_MAIN = windowMain
MIN_SIZE = 160, 60
SIZE = 200, 70
";

const SCRIPT: &str = r#"-- One line per collision start: unix time, wall|car, speed in km/h, track position 0..1.
local logPath = ac.getFolder(ac.FolderID.ACAppsLua) .. '/ACXSCollisions/hits.log'
local maxLines = 500
local walls, cars = 0, 0
local lines = {}

local old = io.load(logPath, '')
for line in (old or ''):gmatch('[^\r\n]+') do
  lines[#lines + 1] = line
end

ac.onCarCollision(0, function()
  local car = ac.getCar(0)
  if not car or ac.getSim().isReplayActive then return end
  local with = car.collidedWith
  if with < 0 then return end
  local kind = with == 0 and 'wall' or 'car'
  if kind == 'wall' then walls = walls + 1 else cars = cars + 1 end
  lines[#lines + 1] = string.format('%d\t%s\t%.1f\t%.4f', os.time(), kind, car.speedKmh, car.splinePosition)
  while #lines > maxLines do table.remove(lines, 1) end
  io.save(logPath, table.concat(lines, '\n') .. '\n')
end)

function script.windowMain(dt)
  ui.text('Walls: ' .. walls .. '   Cars: ' .. cars)
end
"#;

/// What a collision hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    Wall,
    Car,
}

/// One logged collision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collision {
    pub unix: i64,
    pub kind: HitKind,
    pub speed_kmh: f32,
    /// Position on the lap, 0 to 1.
    pub position: f32,
}

fn app_dir(ac_root: &Path) -> PathBuf {
    ac_root.join("apps").join("lua").join(APP_ID)
}

/// `<AC root>\apps\lua\ACXSCollisions\hits.log`.
pub fn log_path(ac_root: &Path) -> PathBuf {
    app_dir(ac_root).join(LOG_FILE)
}

/// Parses the log. Lines that do not fit are skipped.
pub fn parse_log(text: &str) -> Vec<Collision> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.trim().split('\t');
            let unix = parts.next()?.parse().ok()?;
            let kind = match parts.next()? {
                "wall" => HitKind::Wall,
                "car" => HitKind::Car,
                _ => return None,
            };
            let speed_kmh = parts.next()?.parse().ok()?;
            let position = parts.next()?.parse().ok()?;
            Some(Collision { unix, kind, speed_kmh, position })
        })
        .collect()
}

/// Collisions logged between `from` and `to` (unix seconds, inclusive). A missing log gives none.
pub fn collisions_between(ac_root: &Path, from: i64, to: i64) -> Vec<Collision> {
    let text = std::fs::read_to_string(log_path(ac_root)).unwrap_or_default();
    parse_log(&text).into_iter().filter(|c| (from..=to).contains(&c.unix)).collect()
}

/// True when the app folder is in `apps/lua`.
pub fn is_installed(ac_root: &Path) -> bool {
    app_dir(ac_root).join("manifest.ini").is_file()
}

/// A 64 by 64 orange disc for the in-game app bar.
fn icon_png() -> Result<Vec<u8>, crate::png::PngError> {
    const SIZE: u32 = 64;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let centre = (SIZE as f32 - 1.0) / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let distance = (x as f32 - centre).hypot(y as f32 - centre);
            let alpha = ((30.5 - distance).clamp(0.0, 1.0) * 255.0) as u8;
            rgba.extend_from_slice(&[0xFF, 0x7A, 0x1A, alpha]);
        }
    }
    crate::png::encode_rgba(SIZE, SIZE, &rgba)
}

/// Writes the app into a scratch folder and installs it. Refused while the game runs.
pub fn install(ac_root: &Path, game_running: bool) -> Result<PathBuf, AppError> {
    let staging = std::env::temp_dir().join(format!("acxs-collisions-{}", std::process::id())).join(APP_ID);
    std::fs::create_dir_all(&staging)?;
    std::fs::write(staging.join("manifest.ini"), MANIFEST)?;
    std::fs::write(staging.join(format!("{APP_ID}.lua")), SCRIPT)?;
    let icon = icon_png().map_err(|err| std::io::Error::other(err.to_string()))?;
    std::fs::write(staging.join("icon.png"), icon)?;
    let result = lua_apps::install_app(ac_root, &staging, AppScope::User, game_running);
    if let Some(parent) = staging.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_parsed_and_bad_ones_skipped() {
        let log = "1000\twall\t120.5\t0.4200\nnonsense\n1005\tcar\t80.0\t0.5\n1006\tbird\t1\t1\n";
        let hits = parse_log(log);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0], Collision { unix: 1000, kind: HitKind::Wall, speed_kmh: 120.5, position: 0.42 });
        assert_eq!(hits[1].kind, HitKind::Car);
    }

    #[test]
    fn the_script_is_valid_lua() {
        full_moon::parse(SCRIPT).expect("valid Lua");
    }

    #[test]
    fn script_uses_the_window_form_csp_apps_use_and_writes_where_the_reader_looks() {
        assert!(SCRIPT.contains("function script.windowMain(dt)"));
        assert!(SCRIPT.contains(&format!("{APP_ID}/{LOG_FILE}")));
        assert!(MANIFEST.contains("FUNCTION_MAIN = windowMain"));
    }

    #[test]
    fn install_puts_the_app_in_user_apps_and_refuses_while_running() {
        let root = std::env::temp_dir().join(format!("acxs-col-root-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(install(&root, true).is_err());
        assert!(!is_installed(&root));
        install(&root, false).unwrap();
        assert!(is_installed(&root));
        let dir = root.join("apps/lua").join(APP_ID);
        assert!(dir.join("ACXSCollisions.lua").is_file());
        let icon = std::fs::read(dir.join("icon.png")).unwrap();
        assert_eq!(crate::png::decode_rgba(&icon).unwrap().0, 64);
        std::fs::write(log_path(&root), "1000\twall\t100\t0.5\n2000\tcar\t90\t0.6\n").unwrap();
        assert_eq!(collisions_between(&root, 1500, 2500).len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }
}

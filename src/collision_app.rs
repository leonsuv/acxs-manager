//! The ACXS Collisions app: a tiny CSP Lua app that tells wall hits from car hits.
//!
//! Shared memory cannot say what a car hit; Custom Shaders Patch can. The app listens for
//! `ac.onCarCollision` and appends one line per hit to `Documents\Assetto Corsa\logs\
//! acxs-collisions.log`. The recorder reads that file after a session. Installing the app
//! copies it into `apps/lua/ACXSCollisions` and needs the game closed.

use std::path::{Path, PathBuf};

use crate::lua_apps::{self, AppError, AppScope};

/// Folder name, which is also the app id.
pub const APP_ID: &str = "ACXSCollisions";

const MANIFEST: &str = "[ABOUT]
NAME = ACXS Collisions
AUTHOR = ACXS Manager
VERSION = 1.0
DESCRIPTION = Logs wall and car hits for ACXS Manager statistics.

[CORE]
LAZY = NONE

[WINDOW_...]
ID = main
NAME = ACXS Collisions
FUNCTION_MAIN = windowMain
MIN_SIZE = 160, 60
SIZE = 200, 70
";

const SCRIPT: &str = r#"-- Writes one line per collision start: unix time, wall|car, speed in km/h, track position 0..1.
local logPath = ac.getFolder(ac.FolderID.Documents) .. '/Assetto Corsa/logs/acxs-collisions.log'
local walls, cars = 0, 0

local function write(line)
  local ok, file = pcall(io.open, logPath, 'a')
  if ok and file then
    file:write(line, '\n')
    file:close()
  end
end

ac.onCarCollision(0, function()
  local car = ac.getCar(0)
  if not car then return end
  local kind = car.collidedWith == 0 and 'wall' or 'car'
  if kind == 'wall' then walls = walls + 1 else cars = cars + 1 end
  write(string.format('%d\t%s\t%.1f\t%.4f', os.time(), kind, car.speedKmh, car.splinePosition))
end)

function windowMain()
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

/// `Documents\Assetto Corsa\logs\acxs-collisions.log`.
pub fn log_path() -> Option<PathBuf> {
    crate::settings::ac_documents_dir().map(|docs| docs.join("logs").join("acxs-collisions.log"))
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
pub fn collisions_between(from: i64, to: i64) -> Vec<Collision> {
    let text = log_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    parse_log(&text).into_iter().filter(|c| (from..=to).contains(&c.unix)).collect()
}

/// True when the app folder is in `apps/lua`.
pub fn is_installed(ac_root: &Path) -> bool {
    ac_root.join("apps").join("lua").join(APP_ID).join("manifest.ini").is_file()
}

/// Writes the app into a scratch folder and installs it. Refused while the game runs.
pub fn install(ac_root: &Path, game_running: bool) -> Result<PathBuf, AppError> {
    let staging = std::env::temp_dir().join(format!("acxs-collisions-{}", std::process::id())).join(APP_ID);
    std::fs::create_dir_all(&staging)?;
    std::fs::write(staging.join("manifest.ini"), MANIFEST)?;
    std::fs::write(staging.join(format!("{APP_ID}.lua")), SCRIPT)?;
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
    fn install_puts_the_app_in_user_apps_and_refuses_while_running() {
        let root = std::env::temp_dir().join(format!("acxs-col-root-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(install(&root, true).is_err());
        assert!(!is_installed(&root));
        install(&root, false).unwrap();
        assert!(is_installed(&root));
        assert!(root.join("apps/lua").join(APP_ID).join("ACXSCollisions.lua").is_file());
        std::fs::remove_dir_all(&root).ok();
    }
}

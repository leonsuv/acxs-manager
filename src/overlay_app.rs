//! The ACXS Overlay app: draws the HUD Layout in the game, with a delta bar and a session panel.
//!
//! Custom Shaders Patch runs Lua apps with windows the driver opens from the in-game app bar. This
//! module turns a [`Layout`] into such an app in `apps/lua/ACXSOverlay`: **ACXS Dash** draws the
//! layout's widgets, **ACXS Delta** the gain or loss against the best lap, and **ACXS Session**
//! the session timer and a speed trap. Installing needs the game closed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::hud::{self, Color, Layout, SpeedUnit, WidgetKind};
use crate::lua_apps::{self, AppError, AppScope};

/// Folder name, which is also the app id.
pub const APP_ID: &str = "ACXSOverlay";

const MANIFEST: &str = "[ABOUT]
NAME = ACXS Overlay
AUTHOR = ACXS Manager
VERSION = 1.0
DESCRIPTION = Your ACXS HUD layout, a delta bar and a session panel.

[CORE]
LAZY = PARTIAL

[WINDOW_...]
ID = dash
NAME = ACXS Dash
ICON = icon.png
FUNCTION_MAIN = windowDash
FLAGS = NO_BACKGROUND, NO_SCROLLBAR
MIN_SIZE = 240, 120
SIZE = 640, 360

[WINDOW_...]
ID = delta
NAME = ACXS Delta
ICON = icon.png
FUNCTION_MAIN = windowDelta
FLAGS = NO_BACKGROUND, NO_SCROLLBAR
MIN_SIZE = 200, 50
SIZE = 360, 70

[WINDOW_...]
ID = session
NAME = ACXS Session
ICON = icon.png
FUNCTION_MAIN = windowSession
MIN_SIZE = 200, 120
SIZE = 260, 150
";

/// The part of the script that does not depend on the layout.
const SCRIPT_BODY: &str = r#"
local function lapText(ms)
  if not ms or ms <= 0 then return '--:--.---' end
  return string.format('%d:%02d.%03d', math.floor(ms / 60000), math.floor(ms / 1000) % 60, ms % 1000)
end

local function gearText(gear)
  if gear < 0 then return 'R' end
  if gear == 0 then return 'N' end
  return tostring(gear)
end

local function speedText(kmh)
  if speedInMph then return string.format('%.0f mph', kmh * 0.621371) end
  return string.format('%.0f km/h', kmh)
end

local function fontSize(height)
  return math.max(10, height * 0.62)
end

local function drawText(text, p1, p2, color)
  ui.dwriteDrawTextClipped(text, fontSize(p2.y - p1.y), p1, p2, ui.Alignment.Center, ui.Alignment.Center, false, color)
end

local function rpmShare(car)
  local limit = car.rpmLimiter
  if limit <= 0 then limit = 8000 end
  return math.saturate(car.rpm / limit)
end

local function drawBar(p1, p2, share, color)
  ui.drawRectFilled(p1, vec2(p1.x + (p2.x - p1.x) * share, p2.y), color)
end

local function drawLights(p1, p2, share, widget)
  local count = #shiftLights
  local width = (p2.x - p1.x) / count
  for i = 1, count do
    local left = p1.x + width * (i - 1)
    local lit = share >= shiftLights[i]
    ui.drawRectFilled(vec2(left + 1, p1.y), vec2(left + width - 1, p2.y), lit and widget.accent or rgbm(0.2, 0.2, 0.2, 0.6))
  end
end

local function widgetText(widget, car)
  local kind = widget.kind
  if kind == 'speed' then return speedText(car.speedKmh) end
  if kind == 'gear' then return gearText(car.gear) end
  if kind == 'lap' then return lapText(car.lapTimeMs) end
  if kind == 'last' then return lapText(car.previousLapTimeMs) end
  if kind == 'delta' then return string.format('%+.3f', car.performanceMeter) end
  if kind == 'fuel' then return string.format('%.1f L', car.fuel) end
  if kind == 'position' then return car.racePosition > 0 and ('P' .. car.racePosition) or '-' end
  if kind == 'tyres' then
    return string.format('%.0f %.0f  %.0f %.0f', car.wheels[0].tyreCoreTemperature, car.wheels[1].tyreCoreTemperature,
      car.wheels[2].tyreCoreTemperature, car.wheels[3].tyreCoreTemperature)
  end
  return widget.text
end

local function drawWidget(widget, car, size)
  local p1 = vec2(widget.x * size.x, widget.y * size.y)
  local p2 = vec2((widget.x + widget.w) * size.x, (widget.y + widget.h) * size.y)
  if widget.background then ui.drawRectFilled(p1, p2, widget.background) end
  if widget.kind == 'rpm' then
    drawBar(p1, p2, rpmShare(car), widget.color)
  elseif widget.kind == 'lights' then
    drawLights(p1, p2, rpmShare(car), widget)
  else
    drawText(widgetText(widget, car), p1, p2, widget.color)
  end
end

function script.windowDash(dt)
  local car = ac.getCar(0)
  if not car then return end
  local size = ui.windowSize()
  for _, widget in ipairs(widgets) do
    drawWidget(widget, car, size)
  end
end

local deltaRange = 2

function script.windowDelta(dt)
  local car = ac.getCar(0)
  if not car then return end
  local size = ui.windowSize()
  local delta = car.performanceMeter
  local share = math.clamp(delta / deltaRange, -1, 1)
  local middle = size.x / 2
  local top, bottom = size.y * 0.55, size.y * 0.9
  ui.drawRectFilled(vec2(0, top), vec2(size.x, bottom), rgbm(0, 0, 0, 0.4))
  local tip = middle + share * middle
  local color = delta <= 0 and rgbm(0.24, 0.84, 0.55, 1) or rgbm(1, 0.36, 0.42, 1)
  ui.drawRectFilled(vec2(math.min(middle, tip), top), vec2(math.max(middle, tip), bottom), color)
  ui.drawRectFilled(vec2(middle - 1, top), vec2(middle + 1, bottom), rgbm(1, 1, 1, 1))
  drawText(string.format('%+.3f', delta), vec2(0, 0), vec2(size.x, top), color)
end

local topLap, topLast, topBest, lastLapCount = 0, 0, 0, -1

local function trackSpeed()
  local car = ac.getCar(0)
  if not car then return end
  if car.lapCount ~= lastLapCount then
    if lastLapCount >= 0 then
      topLast = topLap
      topLap = 0
    end
    lastLapCount = car.lapCount
  end
  topLap = math.max(topLap, car.speedKmh)
  topBest = math.max(topBest, topLap)
end

function script.update(dt)
  trackSpeed()
end

local function clockText(ms)
  if ms <= 0 then return '--:--:--' end
  local seconds = math.floor(ms / 1000)
  return string.format('%d:%02d:%02d', math.floor(seconds / 3600), math.floor(seconds / 60) % 60, seconds % 60)
end

function script.windowSession(dt)
  trackSpeed()
  local car = ac.getCar(0)
  if not car then return end
  local sim = ac.getSim()
  ui.text('Session left   ' .. clockText(sim.sessionTimeLeft))
  ui.text('Laps   ' .. car.lapCount)
  ui.text('Best lap   ' .. lapText(car.bestLapTimeMs))
  ui.text('Top this lap   ' .. speedText(topLap))
  ui.text('Top last lap   ' .. speedText(topLast))
  ui.text('Top of session   ' .. speedText(topBest))
end
"#;

/// A Lua string literal.
fn lua_string(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn lua_color(color: Color) -> String {
    let part = |v: u8| f32::from(v) / 255.0;
    format!("rgbm({:.3}, {:.3}, {:.3}, {:.3})", part(color.r), part(color.g), part(color.b), part(color.a))
}

fn lua_kind(kind: WidgetKind, source: &str) -> &'static str {
    match kind {
        WidgetKind::Speed => "speed",
        WidgetKind::Gear => "gear",
        WidgetKind::RpmBar => "rpm",
        WidgetKind::ShiftLights => "lights",
        WidgetKind::LapTime if source == hud::SOURCE_LAST_LAP => "last",
        WidgetKind::LapTime => "lap",
        WidgetKind::Delta => "delta",
        WidgetKind::Fuel => "fuel",
        WidgetKind::TyreTemps => "tyres",
        WidgetKind::Position => "position",
        WidgetKind::CustomText => "text",
    }
}

/// The app's script for a layout.
pub fn script(layout: &Layout) -> String {
    let mut out = String::from("-- Made by ACXS Manager from the HUD Layout page. Installing again replaces this file.\n");
    let mph = matches!(layout.speed_unit, SpeedUnit::Mph);
    let _ = writeln!(out, "speedInMph = {mph}");
    let lights: Vec<String> = (0..hud::SHIFT_LIGHT_COUNT).map(|i| format!("{:.3}", hud::shift_light_threshold(i))).collect();
    let _ = writeln!(out, "shiftLights = {{ {} }}", lights.join(", "));
    out.push_str("widgets = {\n");
    for widget in &layout.widgets {
        let rect = widget.rect();
        let background = widget.background.map_or("nil".to_string(), lua_color);
        let _ = writeln!(
            out,
            "  {{ kind = '{}', x = {:.4}, y = {:.4}, w = {:.4}, h = {:.4}, text = {}, color = {}, accent = {}, background = {} }},",
            lua_kind(widget.kind, &widget.source),
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            lua_string(&widget.text),
            lua_color(widget.color),
            lua_color(widget.accent),
            background,
        );
    }
    out.push_str("}\n");
    out.push_str(SCRIPT_BODY);
    out
}

/// A 64 by 64 icon: an orange bar on a clear background.
fn icon_png() -> Result<Vec<u8>, crate::png::PngError> {
    const SIZE: u32 = 64;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let on = (24..40).contains(&y) && (6..58).contains(&x);
            rgba.extend_from_slice(&[0xFF, 0x7A, 0x1A, if on { 255 } else { 0 }]);
        }
    }
    crate::png::encode_rgba(SIZE, SIZE, &rgba)
}

/// True when the app is in `apps/lua`.
pub fn is_installed(ac_root: &Path) -> bool {
    ac_root.join("apps").join("lua").join(APP_ID).join("manifest.ini").is_file()
}

/// Installs the app for `layout`, replacing an earlier one. Refused while the game runs.
pub fn install(ac_root: &Path, layout: &Layout, game_running: bool) -> Result<PathBuf, AppError> {
    let staging = std::env::temp_dir().join(format!("acxs-overlay-{}", std::process::id())).join(APP_ID);
    std::fs::create_dir_all(&staging)?;
    std::fs::write(staging.join("manifest.ini"), MANIFEST)?;
    std::fs::write(staging.join(format!("{APP_ID}.lua")), script(layout))?;
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
    fn the_script_carries_the_layout_and_the_windows_of_the_manifest() {
        let layout = Layout::default_layout();
        let text = script(&layout);
        assert_eq!(text.matches("kind = '").count(), layout.widgets.len());
        for function in ["windowDash", "windowDelta", "windowSession"] {
            assert!(MANIFEST.contains(&format!("FUNCTION_MAIN = {function}")));
            assert!(text.contains(&format!("function script.{function}(dt)")));
        }
        assert!(text.contains("speedInMph = false"));
    }

    #[test]
    fn custom_text_cannot_break_the_script() {
        assert_eq!(lua_string("it's a\\b\nc"), r"'it\'s a\\b c'");
    }

    #[test]
    fn every_block_in_the_script_is_closed() {
        // Not a parser, but it catches a lost `end` or bracket: the script cannot be run here.
        let text = script(&Layout::default_layout());
        let count = |c: char| text.chars().filter(|x| *x == c).count();
        assert_eq!(count('('), count(')'));
        assert_eq!(count('{'), count('}'));
        // Strings first: `'--:--'` would look like a comment.
        let code: Vec<String> = text
            .lines()
            .map(|line| {
                let bare: String = line.split('\'').step_by(2).collect::<Vec<_>>().join(" ");
                bare.split("--").next().unwrap_or("").to_string()
            })
            .collect();
        let words = |word: &str| {
            code.iter()
                .flat_map(|l| l.split(|c: char| !c.is_alphanumeric() && c != '_'))
                .filter(|w| *w == word)
                .count()
        };
        assert_eq!(words("function") + words("for") + words("if"), words("end"));
    }

    #[test]
    fn install_replaces_an_earlier_copy_and_waits_for_the_game() {
        let root = std::env::temp_dir().join(format!("acxs-overlay-root-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let layout = Layout::default_layout();
        assert!(install(&root, &layout, true).is_err());
        install(&root, &layout, false).unwrap();
        install(&root, &layout, false).unwrap();
        assert!(is_installed(&root));
        assert!(root.join("apps/lua").join(APP_ID).join("ACXSOverlay.lua").is_file());
        std::fs::remove_dir_all(&root).ok();
    }
}

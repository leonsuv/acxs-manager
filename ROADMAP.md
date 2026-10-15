# ACXS Manager Feature Coverage Matrix

Goal: one Assetto Corsa manager program that covers the features of the reference
projects collected for this work (about 60 projects, each one a feature source for
car, track, server, telemetry, CSP or launcher behavior).

This file is the coverage matrix. Each row is one feature, grouped by feature area.
It says which ACXS module implements it, which page shows it, and where the work
stands. The statuses were checked against `src/lib.rs` (modules) and
`src/ui/tools/mod.rs` (registered Tools pages) on the date of the last update.

## Status legend

- **Done**: implemented in the core library with unit tests, and reachable from a UI
  page, or part of the build tooling. Works as described in the row.
- **Partial**: some of the feature is in code, but a part is missing. Usually the
  library exists and no page uses it, or the page reads but does not write. The
  note says what is missing.
- **Planned**: not started. No module, or only a placeholder file.
- **Not feasible**: will not be built in this project. The note gives the reason.

## Pages

Main navigation: Home, Cars, Tracks, CSP Lights Patch, Tools. The Tools page lists
the registered tool pages named in the "UI page" column below.

## Summary

Rows: 105. Done: 70. Partial: 13. Planned: 10. Not feasible: 12.

Counts are for the rows in the sections below. Each section is one feature area.

---

## 1. Install and launch

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Find Assetto Corsa installs through Steam library folders | `steam.rs`, `settings.rs` | Install Finder | Done | - |
| Check a game folder picked by hand | `install` tool, `settings.rs` | Install Finder | Done | - |
| Saved game folder and patch library | `settings.rs` | Home | Done | - |
| Detect whether the game is running | `game.rs` | Home, Quick Race, Mods, Game Settings, RTX & ReShade | Done | Uses the Windows process list. Install, uninstall, mod and backup-restore actions refuse while the game runs. |
| Launch the game from the manager | `game.rs` | Home, Quick Race | Done | Same module as the running check. |
| Launch a third-party launcher instead of the game | `launcher.rs` | External Launcher | Done | Starts the game through a saved launch profile: an external program with its arguments. |

## 2. Content browsing

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Scan installed cars and tracks (ui JSON, folder fallback) | `content.rs` | Cars, Tracks | Done | - |
| Text filter over cars and tracks | `content.rs`, `util.rs` | Cars, Tracks | Done | - |
| Field search query language | `query.rs` | Collections, Exports | Done | - |
| Content counts on the Home page | `dashboard_stats.rs` | Home | Done | Category totals for cars, tracks and skins. |
| Car specs (headline values from the car data files) | `cardata.rs` | Car Specs | Done | - |
| Compare two installed cars | `car_compare.rs` | Car Specs | Done | - |
| Track info, layouts and surface table | `track_surfaces.rs`, `content.rs` | Track Info | Done | - |
| Compare two installed tracks | `track_compare.rs` | Track Info | Done | - |
| Export the car and track lists to CSV or JSON | `content_export.rs` | Exports | Done | - |
| Enable and disable cars and tracks without deleting | `mods.rs` | Mods | Done | Disabled items move into a folder inside the game root. Nothing is deleted. |
| Collections of cars and tracks | `collection.rs` | Collections | Done | - |
| Health checks on installed content, with safe repairs | `content_repair.rs` | Metadata & Checks | Done | Reports issues with severity. Repairs are planned first and start selected, so each one can be unticked before it runs. |
| Install a car or track from a zip archive | `installer.rs` | Install Packages | Done | Installer and its unit tests. Used by the Install Packages page. |
| Curated online content catalog | none | - | Planned | Not started. Needs a data source and a license check for each item. |
| Car and track thumbnails | `thumbnails.rs`, `png.rs` | Car Specs, Track Info | Done | Car thumbnails are drawn from the car's KN5 model, track thumbnails top-down. Both are saved as PNG in the item's `acxs-thumbnails` folder. |
| Brand badges | none | - | Planned | Not started. |

## 3. Car and track metadata

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Edit car and track metadata (`ui_car.json`, `ui_track.json`) | `content_meta.rs` | Metadata & Checks | Done | Edits keep the fields the page does not touch. |
| Car physics tuning (tyre, suspension, drivetrain values) | `car_tuning.rs` | Car Tuning | Partial | Engine power and torque, total mass, fuel tank, forward gear ratios and final drive can be edited and applied. The previous file is kept. Tyre and suspension values are not covered. |

## 4. Skins and liveries

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Scan skins per car, with preview and texture counts | `skins.rs` | Skins | Done | - |
| Import a skin folder into an installed car | `skin_import.rs` | Skins | Done | Refuses while the game runs. |
| Paint or edit liveries | `livery.rs` | Skins | Partial | Hue recolour of a skin texture, written to a new PNG file. No painting tools. |
| Licence plate designer | `license_plate.rs`, `png.rs` | Plate Designer | Done | Plate style, text validation and glyph layout. Output is a PNG written to a new file; an existing file is never overwritten. |

## 5. Setups

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| List, import and archive setup files | `setups.rs` | Setups | Done | - |
| Compare two setup files key by key | `setups.rs`, `ini.rs` | Setups | Done | - |
| Three-way merge of setup files | `ini_merge.rs` | Setups | Done | Conflicting keys are written only when the user allows it. |
| Edit single setup values in the page | `setups.rs` | Setups | Done | Values open in an editor. Nothing is written until Save; earlier versions are kept as backups. |

## 6. Models and archives

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Inspect a KN5 model (header, textures, materials, node tree) | `kn5.rs` | Model Tools | Done | Read-only. |
| Export KN5 mesh geometry to OBJ | `kn5_export.rs` | Model Tools | Done | Geometry only. Refuses to overwrite an existing file. |
| Export KN5 models to FBX | `fbx.rs` | Model Tools | Done | ASCII FBX 7.4. The page writes one mesh with no normals. |
| Write KN5 models from a 3D source | `obj_import.rs`, `kn5_writer.rs` | Model Tools | Done | Reads OBJ files and writes a KN5 model. |
| List entries of a `data.acd` archive | `acd.rs` | Model Tools | Done | Lists entries. Extraction is in the library only. |
| Preview image of a KN5 model | `ortho.rs`, `png.rs` | Model Tools | Done | Flat-shaded view from a fixed angle, saved as PNG. |
| Bake vertex ambient occlusion for tracks and cars | `ao_bake.rs` | Model Tools | Partial | CPU bake. The values are computed and reported. They are not written to a file. |

## 7. CSP lights patch

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Identify known lights-patch builds by SHA-256 | `csp.rs` | CSP Lights Patch | Done | Known builds: 0.2.11 and 0.3.0-preview675. Other builds are reported as unrecognized. |
| Install, switch and uninstall, with displaced files archived | `csp.rs` | CSP Lights Patch | Done | Nothing is deleted. Displaced files go to `acxs-patch-archive/`. |
| Roll back a failed install | `csp.rs` | CSP Lights Patch | Done | - |

## 8. CSP configuration and game settings

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Edit CSP config files (key, value, documentation) | `csp_config.rs` | CSP Config | Done | - |
| Force-feedback tuning | `ffb.rs` | CSP Config (FFB) | Done | - |
| Weather presets and weather effects | `weather.rs` | CSP Config (Weather) | Done | Weather effect and preset values can be edited. Presets can be created and copied. Two versions can be compared. |
| Car classes and car years | `car_classes.rs` | CSP Config (Car classes) | Done | Classes can be created and removed in the page. Cars can be added to a class and given a year. |
| Gamepad and keyboard assist settings | `gamepad.rs` | CSP Config (Gamepad) | Partial | Settings editor. The config file location is not yet checked against a real install; the page shows a warning. The assist app is not installed by the manager. |
| Race rules settings for the rules mod | `rare.rs` | Mods | Partial | Installed check and settings editor. The rules app itself is not installed by the manager. |
| List CSP apps and user apps | `lua_apps.rs` | Mods | Done | Lists the apps shipped with CSP and the apps the user installed. |
| Install and manage CSP apps | `lua_apps.rs` | Mods | Partial | User apps can be installed and removed. CSP-shipped apps are listed and protected; the page cannot install or remove them. |
| Shift-light thresholds per car | `shift_lights.rs` | Car Specs | Partial | Preview of the LED thresholds only. Not written to the car. |
| Shift-light output to steering wheel LEDs | none | - | Planned | Not started. Needs a hardware bridge. |
| Video settings editor (`video.ini`) | `game_cfg.rs` | Game Settings | Done | - |
| Game settings file editor (`assetto_corsa.ini`) | `game_cfg.rs` | Game Settings | Done | Read, write and validation. |
| Configuration snapshots: create, verify, restore | `backup.rs` | Game Settings | Done | Restore refuses while the game runs. |

## 9. Quick race

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Write `race.ini` and start the game | `race.rs`, `game.rs` | Quick Race | Done | Writes the config. The start step is in `game.rs`. |
| Named race presets | `presets.rs` | Quick Race | Done | Replacing a preset keeps the old value. |

## 10. Dedicated server

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Edit `server_cfg.ini` | `server.rs` | Dedicated Server | Done | - |
| Edit `entry_list.ini` | `server.rs` | Dedicated Server | Done | - |
| Start and stop the server process | `server_process.rs` | Dedicated Server | Done | Starts the server from the game's server folder, keeps recent log lines, and asks for confirmation before stopping. |
| Live server status over HTTP | `server_status.rs` | Dedicated Server | Done | Status queries are unit-tested. |
| Championships, results and race history | `championship.rs` | Championships | Partial | Standings, round results from CSV and JSON storage are on the Championships page. Server result files and race history are not read. |
| Upload content to the server | none | - | Planned | Not started. |
| Server plugins (AI traffic, dynamic weather, plugin API) | none | - | Planned | Not started. The reference server is large. Would run as an external process, not be ported. |

## 11. Logs and diagnostics

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Summarize game and CSP logs (errors, warnings, info counts) | `ac_log.rs` | Logs | Done | - |
| Diagnostic bundle for bug reports | `diagnostics.rs` | Logs | Done | Writes a zip with diagnostic information. An existing bundle file is never overwritten. |

## 12. Telemetry and HUD

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Decode the built-in UDP telemetry protocol | `telemetry.rs` | Live Telemetry | Done | - |
| UDP client: handshake, car samples, completed laps | `udp_client.rs` | Live Telemetry | Done | Feeds the live charts and the latest values. |
| Telemetry CSV export | `telemetry_csv.rs` | Live Telemetry | Done | - |
| Shared memory reader (static, physics, graphics data) | `sharedmem.rs` | Live Telemetry | Done | Windows only. Other platforms report the reader as unsupported. The page parsers are unit-tested. |
| Live charts (throttle, brake, steering, gear) | `charts.rs` | Live Telemetry | Done | Speed is plotted too. |
| Track map and friction circle | `track_map.rs` | Live Telemetry | Done | - |
| Telemetry output to hardware gauges | none | - | Planned | Not started. Needs a microcontroller bridge. |
| Telemetry web dashboard for other devices | `web_dashboard.rs` | Live Telemetry | Done | Local HTTP server, started from the page. |
| In-game HUD and overlays | `hud.rs` | HUD Layout | Partial | Layout model, validation, JSON save and load, and a preview on the page. Nothing is drawn in the game: that needs a CSP app, which is not built. |

## 13. Laps, delta and replay

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Local lap records (best lap per car and track, full history) | `laptimes.rs` | Lap Times | Done | Stored in a JSON file in the game root. |
| Lap delta trace against a reference lap | `delta.rs` | Lap Times | Done | - |
| Delta bar overlay in game | none | - | Planned | Not started. Would be a CSP app. |
| Shared leaderboard across users | none | - | Planned | Not started. Needs a server. |
| Replay header and lap table parsing | `replay.rs` | Replay Viewer | Done | Format version 16. |
| Replay lap table to CSV | `telemetry_csv.rs` | Replay Viewer | Done | - |
| Replay playback | `replay_player.rs` | Replay Viewer | Done | Play and pause, speed selector, seek slider. |

## 14. Race engineering and broadcast

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Race engineer advice from telemetry | `engineer.rs` | Race Engineer | Done | Rule-based advice from one replay. |
| Broadcast timing overlay (timing tower, speed trap, session timer) | `timing.rs` | Timing Tower | Partial | Race order, gaps and the session fastest lap are on the Timing Tower page. Speed trap, session timer and an on-screen overlay are not built. |
| Setup advice from telemetry analysis | `setup_advice.rs` | Setup Advice | Done | Heuristic estimate of understeer or oversteer in corners, from one replay. The thresholds are not validated against real sessions. |
| Race strategy: fuel, pit window and stop plan | `strategy.rs` | Strategy | Done | Fuel use, laps covered by a tank, pit window and stop plan. |

## 15. Input

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Bundled CSP Lua apps (FPV camera, HUD, livery paint, app template) | none | - | Planned | Not started. Reference apps are installed into the game folder. Check each license before copying. |
| Virtual joystick from mouse and keyboard input | none | - | Not feasible | Needs a signed Windows virtual joystick driver. This project does not ship a kernel driver, and a user-mode app cannot create a joystick device without one. |

## 16. Graphics

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Post-processing shader injection | `reshade.rs`, `graphics_check.rs` | RTX & ReShade | Partial | Installs and removes an injector build folder, edits its config, lists presets and reports proxy DLL clashes. The shader injection is done by the third-party runtime, which this project does not build. |
| DirectX-to-Vulkan translation layer (DX8/DX9 path-tracing runtime) | `remix.rs` | RTX & ReShade | Partial | Installs and removes a runtime build folder and writes its quality settings. The translation runtime itself is third-party and is not built here. |
| Remaster toolkit for DX8/DX9 game assets | none | - | Not feasible | A separate large editor platform for other games' assets. |
| Switch the active graphics injector (lights patch, injector build, runtime build or none) | `graphics_profiles.rs`, `graphics_check.rs` | Graphics Profile | Done | Keeps one injector active and archives whatever it replaces. |

## 17. Content protection and licensing

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| DLC ownership check bypass (AC binary patching tool) | none | - | Not feasible | The manager will not circumvent DLC or licensing checks. |
| Decryption of protected model files | none | - | Not feasible | The manager will not remove content protection. Read-only archive listing is in the Models section. |

## 18. Other games and general tools

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Livery injection for other games | none | - | Not feasible | Targets other games and injects into them. Not an Assetto Corsa feature. |
| Line-art template renderer from 3D models | none | - | Not feasible | A general 3D tool, not an Assetto Corsa feature. |
| One-click launcher for other sims (iRacing, ACC) | none | - | Not feasible | Other games. Out of scope. |

## 19. Platform, build and distribution

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Windows cross-compile from macOS | `scripts/build-windows.sh` | - | Done | Output copied to `dist/ACXS-Manager.exe`. |
| Windows CI: library tests and release build | `.github/workflows/windows-build.yml` | - | Planned | Not started. The workflow file is not in the repository. |
| Signed release builds and an update channel | `update.rs`, `scripts/package-release.sh` | Home | Partial | The opt-in update check reads a JSON feed. The packaging script writes a SHA-256 sum. Builds are not signed. |
| Linux installer (game under a compatibility layer) | none | - | Not feasible | ACXS targets Windows. The Linux setup depends on a compatibility layer and would be a separate project. |
| Browser extension to open content in a third-party manager | none | - | Not feasible | Depends on a third-party launcher and third-party sites. Not a manager feature. |

## 20. Reference material

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Written guides and tutorials (Lua apps, VR setup, Linux setup, setup building) | none | - | Not feasible | Documentation only. There is no feature to build. |
| Duplicate and archived copies of reference projects | none | - | Not feasible | Not features. Only one copy is tracked. |
| Community mod author list | none | - | Not feasible | Opinion data, archived. Not a feature. |

---

## Not implemented, and why

- **Graphics runtimes themselves (DX8/DX9 translation, ReShade and Remix rendering):** the app
  does not implement these renderers. It does manage which injector is installed: a profile switcher
  keeps one of CSP, ReShade, Remix or none active, archiving whatever it replaces.
- **DLC bypass and content protection removal:** the manager will not circumvent
  licensing or content protection.
- **Virtual joystick:** needs a signed Windows driver. This project is user-mode only.
- **Other games, general 3D tools, documentation and archived copies:** not Assetto Corsa features.
- **Linux setup and browser integration:** ACXS targets Windows. The other paths depend
  on third-party software that this project does not control.

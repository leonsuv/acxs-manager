# ACXS Manager Feature Coverage Matrix

Goal: one Assetto Corsa manager program that covers the features of the reference
projects collected for this work (about 60 projects, each one a feature source for
car, track, server, telemetry, CSP or launcher behavior).

This file is the coverage matrix. Each row is one feature, grouped by feature area.
It says which ACXS module implements it, which page shows it, and where the work
stands. The statuses were checked against `src/lib.rs` (modules) and
`src/ui/tools/mod.rs` (registered Tools pages) on the date of the last update.

## Status legend

- **Done**: implemented in the core library and reachable from a UI page, or part
  of the build tooling. Works as described in the row.
- **Partial**: some of the feature is in code, but a part is missing. Usually the
  library exists and no page uses it, or the page reads but does not write. The
  note says what is missing.
- **Planned**: not started. No module, or only a placeholder file.
- **Not feasible**: will not be built in this project. The note gives the reason.

## Pages

Main navigation: Home, Cars, Tracks, CSP Lights Patch, Tools. The Tools page lists
the registered tool pages named in the "UI page" column below.

## Summary

Rows: 98. Done: 44. Partial: 10. Planned: 30. Not feasible: 14.

Counts are for the rows in the sections below. Each section is one feature area.

---

## 1. Install and launch

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Find Assetto Corsa installs through Steam library folders | `steam.rs`, `settings.rs` | Install Finder | Done | - |
| Check a game folder picked by hand | `install` tool, `settings.rs` | Install Finder | Done | - |
| Saved game folder and patch library | `settings.rs` | Home | Done | - |
| Detect whether the game is running | `game.rs` | Home, Quick Race, CSP Lights Patch | Done | Uses the Windows process list. Install, uninstall, mod and backup-restore actions refuse while the game runs. |
| Launch the game from the manager | `game.rs` | Home, Quick Race | Done | - |
| Launch a third-party launcher instead of the game | none | - | Planned | Not started. Only direct launch of the game exists. |

## 2. Content browsing

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Scan installed cars and tracks (ui JSON, folder fallback) | `content.rs` | Cars, Tracks | Done | - |
| Text filter over cars and tracks | `content.rs`, `util.rs` | Cars, Tracks | Done | - |
| Field search query language | `query.rs` | Collections | Done | - |
| Content counts on the Home page | `dashboard_stats.rs` | Home | Partial | Home shows car and track counts itself. The per-category statistics module is not used by any page. |
| Car specs (headline values from the car data files) | `cardata.rs` | Car Specs | Done | - |
| Compare two installed cars | `car_compare.rs` | Car Specs | Done | - |
| Track info, layouts and surface table | `track_surfaces.rs`, `content.rs` | Track Info | Done | - |
| Compare two installed tracks | `track_compare.rs` | Track Info | Done | - |
| Export the car and track lists to CSV or JSON | `content_export.rs` | none | Partial | The export functions exist and are tested. No page has an export action. |
| Enable and disable cars and tracks without deleting | `mods.rs` | Mods | Done | Disabled items move into a folder inside the game root. Nothing is deleted. |
| Collections of cars and tracks | `collection.rs` | Collections | Done | - |
| Read-only health checks on installed content | `content_repair.rs` | Metadata & Checks | Partial | Reports issues with severity. No automatic repair. |
| Curated online content catalog | none | - | Planned | Not started. Needs a data source and a license check for each item. |
| Car and track thumbnails and brand badges | `png.rs` (placeholder) | - | Planned | Not started. Image output module is a placeholder. |

## 3. Car and track metadata

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Edit car and track metadata (`ui_car.json`, `ui_track.json`) | `content_meta.rs` | Metadata & Checks | Done | Edits keep the fields the page does not touch. |
| Car physics tuning (tyre, suspension, drivetrain values) | `car_tuning.rs` (placeholder) | - | Planned | Not started. The Car Specs page reads values only. |

## 4. Skins and liveries

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Scan skins per car, with preview and texture counts | `skins.rs` | Skins | Done | - |
| Import a skin folder into an installed car | `skin_import.rs` | Skins | Done | Refuses while the game runs. |
| Paint or edit liveries | none | - | Planned | Not started. |

## 5. Setups

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| List, import and archive setup files | `setups.rs` | Setups | Done | - |
| Compare two setup files key by key | `setups.rs`, `ini.rs` | Setups | Done | - |
| Three-way merge of setup files | `ini_merge.rs` | Setups | Done | Conflicting keys are written only when the user allows it. |
| Edit single setup values in the page | none | - | Planned | Not started. The page browses, compares, imports, archives and merges. It does not edit values. |

## 6. Models and archives

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Inspect a KN5 model (header, textures, materials, node tree) | `kn5.rs` | Model Tools | Done | Read-only. |
| Export KN5 mesh geometry to OBJ | `kn5_export.rs` | Model Tools | Done | Geometry only. Refuses to overwrite an existing file. |
| Export KN5 models to FBX | none | - | Planned | Not started. OBJ only. |
| Write KN5 models from a 3D source | `kn5_writer.rs` (placeholder) | - | Planned | Not started. The writer module is a placeholder. |
| List entries of a `data.acd` archive | `acd.rs` | Model Tools | Done | Lists entries. Extraction is in the library only. |
| Bake vertex ambient occlusion for tracks and cars | none | - | Planned | Not started. Needs an external GPU baking tool. |

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
| Weather presets and weather effects | `weather.rs` | CSP Config (Weather) | Partial | Read-only. Presets cannot be edited. |
| Car classes and car years | `car_classes.rs` | CSP Config (Car classes) | Partial | Read and compare only. Classes cannot be edited. |
| Gamepad and keyboard assist settings | `gamepad.rs` | CSP Config (Gamepad) | Partial | Settings editor only. The assist app is not installed by the manager. The config file location is not yet checked against a real install. |
| Race rules settings for the rules mod | `rare.rs` | Mods | Partial | Installed check and settings editor. The rules app itself is not installed by the manager. |
| List CSP apps and user apps | `lua_apps.rs` | Mods | Partial | List only. Install and removal are not implemented. |
| Install and manage CSP apps | none | - | Planned | Not started. |
| Shift-light thresholds per car | `shift_lights.rs` | Car Specs | Partial | Preview of the LED thresholds only. |
| Shift-light output to steering wheel LEDs | none | - | Planned | Not started. Needs a hardware bridge. |
| Video settings editor (`video.ini`) | `game_cfg.rs` | Game Settings | Done | - |
| Game settings file editor (`assetto_corsa.ini`) | `game_cfg.rs` | none | Partial | Read, write and validation exist in the library. No page edits this file. |
| Configuration snapshots: create, verify, restore | `backup.rs` | Game Settings | Done | Restore refuses while the game runs. |

## 9. Quick race

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Write `race.ini` and start the game | `race.rs`, `game.rs` | Quick Race | Done | - |
| Named race presets | `presets.rs` | Quick Race | Done | Replacing a preset keeps the old value. |

## 10. Dedicated server

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Edit `server_cfg.ini` | `server.rs` | Dedicated Server | Done | - |
| Edit `entry_list.ini` | `server.rs` | Dedicated Server | Done | - |
| Start and stop the server process | `server_process.rs` (placeholder) | - | Planned | Not started. |
| Live server status over HTTP | none | - | Planned | Not started. |
| Championships, results and race history | none | - | Planned | Not started. |
| Upload content to the server | none | - | Planned | Not started. |
| Server plugins (AI traffic, dynamic weather, plugin API) | none | - | Planned | Not started. The reference server is large. Would run as an external process, not be ported. |

## 11. Logs and diagnostics

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Summarize game and CSP logs (errors, warnings, info counts) | `ac_log.rs` | Logs | Done | - |

## 12. Telemetry and HUD

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Decode the built-in UDP telemetry protocol | `telemetry.rs` | Live Telemetry | Done | - |
| UDP client: handshake, car samples, completed laps | `udp_client.rs` | Live Telemetry | Done | Shows the latest sample only. |
| Telemetry CSV export | `telemetry_csv.rs` | Live Telemetry | Done | - |
| Shared memory reader (static, physics, graphics data) | none | - | Planned | Not started. Would be a Windows memory-mapped reader. |
| Live charts (throttle, brake, steering, gear) | none | - | Planned | Not started. |
| Track map and friction circle | none | - | Planned | Not started. |
| Telemetry output to hardware gauges | none | - | Planned | Not started. Needs a microcontroller bridge. |
| Telemetry web dashboard for other devices | none | - | Planned | Not started. |
| In-game HUD and overlays | `hud.rs` (placeholder) | - | Planned | Not started. Would be a CSP app. The manager does not draw overlays itself. |

## 13. Laps, delta and replay

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Local lap records (best lap per car and track, full history) | `laptimes.rs` | Lap Times | Done | Stored in a JSON file in the game root. |
| Lap delta trace against a reference lap | `delta.rs` | Lap Times | Done | - |
| Delta bar overlay in game | none | - | Planned | Not started. Would be a CSP app. |
| Shared leaderboard across users | none | - | Planned | Not started. Needs a server. |
| Replay header and lap table parsing | `replay.rs` | Replay Viewer | Done | Format version 16. |
| Replay lap table to CSV | `telemetry_csv.rs` | Replay Viewer | Done | - |
| Replay playback | none | - | Planned | Not started. |

## 14. Race engineering and broadcast

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Race engineer advice from telemetry | none | - | Planned | Not started. |
| Broadcast timing overlay (timing tower, speed trap, session timer) | none | - | Planned | Not started. |
| Setup advice from telemetry analysis | none | - | Planned | Not started. The reference claims for this are not visible in the reference code, so they need review first. |

## 15. Input

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Bundled CSP Lua apps (FPV camera, HUD, livery paint, app template) | none | - | Planned | Not started. Reference apps are installed into the game folder. Check each license before copying. |
| Virtual joystick from mouse and keyboard input | none | - | Not feasible | Needs a signed Windows virtual joystick driver. This project does not ship a kernel driver, and a user-mode app cannot create a joystick device without one. |

## 16. Graphics

| Feature | ACXS module(s) | UI page | Status | Note |
| --- | --- | --- | --- | --- |
| Post-processing shader injection | `rtx.rs` (placeholder) | Tools: graphics placeholder page | Not feasible | The page shows "Not implemented yet". Injection runs inside the game process and competes with CSP for the rendering path. |
| DirectX-to-Vulkan translation layer (DX8/DX9 path-tracing runtime) | none | - | Not feasible | A graphics runtime that replaces the game's rendering path. It would have to be injected into the game. Assetto Corsa is not a DX8 or DX9 title, so the runtime has no role here. |
| Remaster toolkit for DX8/DX9 game assets | none | - | Not feasible | A separate large editor platform for other games' assets. |

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
| Windows CI: library tests and release build | `.github/workflows/windows-build.yml` | - | Done | Runs `cargo test --lib`, then a release build, and uploads the executable. |
| Signed release builds and an update channel | none | - | Planned | Not started. |
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

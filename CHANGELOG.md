# Changelog

## Unreleased

Fixes:

- Live UDP telemetry: the client now subscribes to car updates after the handshake (no data arrived before), repeats the handshake until the game answers, and keeps retrying while the game port is unreachable. Handshake (408 bytes) and lap packets (212 bytes) are decoded with UTF-16 names, as the game sends them.
- Live telemetry and the web dashboard keep updating while another page is shown.
- The "game running" state is checked every 2 seconds instead of only on rescan; skin import and graphics profile switches check it again when they run. Older rescans no longer overwrite newer ones.
- Quick Race and launcher profiles write `race.ini` to `Documents/Assetto Corsa/cfg`, where the game reads it, using the keys the game reads (`[WEATHER]`, `SUN_ANGLE`, `RACE_LAPS`, `STARTING_POSITION`, `[HEADER]`, ...).
- The Documents folder is resolved through Windows (OneDrive and moved folders work).
- Tracks with layouts (`ui/<layout>/ui_track.json`) show their layouts, name, country and length.
- Car specs, car comparison and shift lights read packed `data.acd` (all Kunos cars). Car tuning refuses cars with `data.acd`, because the game ignores `data/` then.
- INI files saved as Windows ANSI are read and saved in that encoding, and numbers with an inline `;` comment are read.
- Setups are listed from the per-track folders (`setups/<car>/<track>/`) and imported into a track folder (default `generic`).
- CSP settings, FFB tweaks, gamepad assist and `weather_fx.ini` are written to the user's CSP config in `Documents/Assetto Corsa/cfg/extension` (which overrides the patch defaults) and shown with those overrides applied.
- Configuration snapshots include `Documents/Assetto Corsa/cfg`.
- Shared memory static page: `trackConfiguration` is 33 characters wide, so the car skin and pit window fields are read from the right offsets.
- The dedicated server starts without a console window. Steam is found through the registry, and the first start searches every Steam library for the game.
- Package installer: folder names are matched ignoring case, and loose files at the archive root (readme, licence) are skipped instead of refusing the archive.
- RTX Remix card notes that Remix only hooks Direct3D 9, while the game uses Direct3D 11.
- `race.ini` follows the format Content Manager writes (`CAR_0 MODEL=-`, `[RACE] SKIN`, `[WIND]`, `[GROOVE]`, `[GHOST_CAR]`, Optimum `[DYNAMIC_TRACK]` preset, inactive `[REPLAY]`/`[RESTART]`/`[BENCHMARK]`).
- Update check supports `https://` feeds with certificate checks (Windows TLS stack on Windows, rustls elsewhere); plain `http://` still works but is flagged as unprotected. Redirects that leave HTTPS are refused.
- Gamepad tab: Advanced Gamepad Assist keeps its tuning in CSP's script storage and ignores `gamepad_fx.ini`, so the tab no longer writes those keys. It now selects AGA as CSP's gamepad script (user `gamepad_fx.ini`) and edits the controller values in `controls.ini [X360]` (gamma, deadzone, rumble, gamepad input method).

## 0.3.0

- Install car and track zips from the Install Packages page, with a game-running guard.
- Live server status card on the Dedicated Server page.
- Live shared-memory panel (Windows) on the live telemetry page.
- Track thumbnails rendered from the track model.
- Weather presets can be created, duplicated and edited.
- Car classes can be created and removed.
- assetto_corsa.ini editor on the Game Settings page.
- Game-module tests for the running check and launch errors.

## 0.2.0

- Graphics profile switcher: one injector active at a time (CSP, ReShade, RTX Remix, or none), archiving whatever it replaces.
- ReShade and RTX Remix management: install from a user-picked build folder, archive instead of delete, config editors, quality presets, preset list.
- Diagnostics bundle for bug reports (summary, log tail, settings with paths removed).
- Car tuning page, setup value editing, setup advice (heuristic, from replay paths).
- Livery recolouring (PNG, optional mask), license plate designer, HUD layout editor.
- FBX export, OBJ import to KN5, ambient-occlusion bake report, car thumbnails, KN5 preview render.
- Launcher profiles for starting the game through an external tool.
- Replay playback; live telemetry charts, track map and friction circle; telemetry pedal and g-force fields.
- Web dashboard for telemetry on the local network (opt-in bind to all interfaces) and an opt-in update check.
- Windows-only shared memory reader for the live physics, graphics and static pages.
- Clippy clean across the crate.
- Ghidra analysis of the lights patch loader is recorded in the analysis document.

## 0.1.0

- First unified build: one Windows executable (ACXS-Manager.exe) built with cargo-xwin.
- Library covers content scanning (cars, tracks, skins), content checks and repairs, metadata editing, car specs and tuning, setups and single-value editing, KN5 reading, export (OBJ, FBX) and writing, OBJ import, thumbnails, dedicated server config and process control, server status, championships, lap records, sector bests, replay parsing and playback, live telemetry (UDP client), charts and track map, timing tower, race strategy, CSP lights patch install/switch/uninstall with archiving, CSP config and weather and car-class editing, ReShade and RTX Remix management, graphics proxy conflict checks, graphics profile switching, mod enable/disable, Lua app listing and installation, collections, query language, backups and restores, diagnostics bundle.
- Not in this release: the renderers themselves (the DirectX/Vulkan translation runtime, ReShade and Remix rendering), the game-binary tool, the virtual joystick driver, shared leaderboards, and anything that needs a hosted service.
- Not yet verified in the running game: telemetry packet layouts beyond the documented ones, race.ini details, setup folder layout, server log patterns.

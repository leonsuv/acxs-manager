# Changelog

## Unreleased

Checked against real drives, a live game session and a working Content Manager `race.ini`:

- Quick Race and server joins confirmed on the real game: three Quick Races and a 75-car server session started from files ACXS wrote. The join file now has the same keys as Content Manager's, and the race port, feature list and Steam id matched exactly on a live server.
- Joining says so when Steam is not signed in, and asks the server for its features again if the panel has not heard from it yet.
- `race.ini` keeps the driver's nationality, which also clears the game's only logged error. `race.ini` backups stop at the newest 10.
- The status bar and Lights Patch page name the installed CSP version from its own manifest instead of "unrecognized", and scans no longer hash the 140 MB `dwrite.dll`.
- Recorder fixes from real data: sessions are saved when the game's memory freezes after exit; a crash is found across a short window, because the g spike arrives before the speed loss, so slow crashes count; kerbs, landings and the end of a braking zone do not; counter-steering counts as oversteer; handling is judged only while the wheel is steady; a new session type starts a new session; long drives stay whole on the map.
- Tracks without lap position, such as La Canyons and Drift, get their map from the driven path. Only lasting handling spots get a dot on it.
- Sessions page: installed car and track names, session type, delete a session, handling marked as an estimate. Files are written compactly.
- Replays made while ACXS was closed reach the archive at the next start. A `recorder.log` explains missing sessions.
- ACXS Collisions rewritten against CSP's own apps: `script.windowMain`, `io.save` in its own folder, an icon, hits in replays skipped. It needs no step in the game. Still not run in the game.
- Championships read dedicated server result files.
- Content health checks: a model list naming files the track does not ship is a note, not an error, because stock tracks do it and run fine. An empty car folder is one warning.

Earlier in this cycle:

- Sessions page redesigned: big track map (real outline from the recorded lap, start marker, glowing trouble-spot dots with hover names, legend), stat grid, handling bar and ranked trouble spots. Stretches of understeer/oversteer are merged into one entry; the list reloads when a session is saved.
- Test tools: `scripts/fake-ac-drive.ps1` plays a lap along a real track line (default Nordschleife) with problems at known positions into fake shared memory, `examples/recorder_probe.rs` runs the recorder against it, and `ACXS_SHM_PREFIX` points the reader at the fake mappings. Checked end to end: all seven event kinds landed at the scripted lap positions and the Nordschleife outline matches the game's own map.

- Phase 6: optional "ACXS Collisions" CSP Lua app (installed from the Sessions page) logs wall and car hits; the recorder adds them to each session and to the track map.

- Sessions suggestions card has an "Open Setups" button (tools can now ask the app to open another tool page).

- Replay Viewer: the summary now shows distance, top and average speed, full-throttle share, hard braking time and impact and drop counts worked out from the replay samples. Home ignores impossible "jumps" over a minute (falls off the map).

- Home: "Best laps" card (best valid lap per car and track, from the Content Manager session files and ACXS recordings; laps with cuts are ignored).

- Sessions page draws the track outline from the recorded path with a coloured dot for each trouble spot (crash, off track, lock-up, wheelspin, jump, understeer, oversteer).

- Sessions page: "Keep more game replays" raises the autosave limits in the game's `replay.ini` (race 10, qualifying 5, other 10; larger values stay; old file backed up).

- Sessions page: setup suggestions per car (understeer, oversteer, brake lock-ups, wheelspin) once enough driving is recorded, each with the number behind it.

- Recorder also pins the trouble spots of each lap (100 slices of the track) and copies the game's fresh `.acreplay` into `%APPDATA%\ACXS\replays\` after a session (newest 20 kept) before the game overwrites it.

- Recorder adds understeer/oversteer share (steering gain learned from low-speed driving, no car data needed) and a Sessions page under Driving data listing every recorded session.

- Session recorder: while Assetto Corsa is live, ACXS samples the shared memory at about 30 Hz and saves one JSON file per session to `%APPDATA%\ACXS\sessions\` (time, distance, top speed, laps, crashes, off-track, jumps, brake lock-ups, wheelspin). Shown in Home statistics, with "Recording session" in the status bar.

- Home: "Your driving" section built from the Content Manager session history (drive time, distance, top speed, crashes, jumps, most driven cars and tracks, recent weeks). Read only; sessions started by ACXS are not recorded yet.

Redesign:

- New dark interface: grouped sidebar, status header, command palette (Ctrl+K), toasts, favorites, a card layout with real previews for cars, tracks and skins, and a Showroom with skins, specs and keyboard control.
- Servers page: a compact table of your servers and the public Kunos lobby (read with the Steam id of whoever is signed in), a car chooser from the entry list, and the server's raw info. Joining writes `race.ini` and starts `acs.exe` directly, without Content Manager.
- Quick Race and Launch start `acs.exe` with the Steam app id set so Steam no longer detours through the launcher.
- Before/after view for video and `assetto_corsa.ini` settings; the CSP config editor shows changed values, the old value and a revert button.
- Stock `ui_car.json` files with raw line breaks in strings are read (real car names, brands and specs).
- Quick Race rebuilt: pictures of the track and car, "Choose track" / "Choose car and skin" open the Cars and Tracks pages in a picking mode and bring the choice back; practice, qualifying and race (laps or minutes), weather list, time of day and speed, temperatures, wind, track grip, AI, penalties, jump start, fixed setup, and driver aids (traction control, ABS, stability, automatic gears, throttle blip, automatic clutch, racing line, damage, fuel, tyre wear, tyre blankets, slipstream) written to `assists.ini` with a backup.
- Public servers load by themselves at start: the list of the last session appears at once from a saved file, and a background refresh updates servers in place, adds new ones and drops the ones that came back empty, while the list stays usable; an open Online tab refreshes itself every five minutes. The Online tab now lists servers from the community directory acservers.online (those with players now, plus a search over every listed server), read with a plain ACXS-Manager user agent, one request at a time. Next to the Kunos lobby (which only answers the game's own client) there is a Custom list source for any address serving the lobby JSON, plain or gzip.
- Joining a server that needs a Custom Shaders Patch build now tells the game the track exactly as the server names it (`csp/2651/../la_canyons`), strips the marker after a skin name, and sends an ASCII server name.
- Server join panel rebuilt: the Join button is at the top and names the car it will use (one click joins with the first car you own that has a free slot; your last choice on each server is remembered); a searchable grid of the server's cars with pictures, free slots and "not installed" marks; double-click a car to join with it; skins, drivers and raw info below; clear reasons when Join is not possible (missing track or car, password, server offline).
- Sidebar reorganised: Play (Home, Quick Race, Servers), Garage (Cars, Tracks, Showroom, Skins, Setups, Mods), Tuning (CSP Config, Game Settings, Lights Patch), Driving data (Lap Times, Replays, Live Telemetry), drawn icons, and the other 21 tools folded under "More tools". Right-click any entry to pin it to the top; pins and the folded state are remembered.
- Updates: "Check for updates" is at the top of Settings and linked from the sidebar; it shows the new version, its notes and a button to the download page.
- Servers whose track needs Custom Shaders Patch (`csp/2651/../la_canyons-freeroam`) are matched to your installed track and layout. Public and own servers can be found with Ctrl+K.
- "Race this car" (cars, showroom) and "Race here" (tracks) open Quick Race with the choice filled in.
- Drop a car or track `.zip` on the window to install it. RTX & ReShade is switched off for now.
- The window remembers its size and position. `scripts/package-windows.ps1` builds the release zip on Windows.

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
- Opt-in crash reports: panics are stored locally and, only when enabled on the Home page, sent to the project's GlitchTip server with user paths removed.

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

# Changelog

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

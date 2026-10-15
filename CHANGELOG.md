# Changelog

## 0.1.0 (unreleased)

- First unified build: one Windows executable (ACXS-Manager.exe) built with cargo-xwin.
- Library covers content scanning (cars, tracks, skins), content checks and repairs, metadata editing, car specs and tuning, setups and single-value editing, KN5 reading, export (OBJ, FBX) and writing, OBJ import, thumbnails, dedicated server config and process control, server status, championships, lap records, sector bests, replay parsing and playback, live telemetry (UDP client), charts and track map, timing tower, race strategy, CSP lights patch install/switch/uninstall with archiving, CSP config and weather and car-class editing, ReShade and RTX Remix management, graphics proxy conflict checks, graphics profile switching, mod enable/disable, Lua app listing and installation, collections, query language, backups and restores, diagnostics bundle.
- Not in this release: the renderers themselves (the DirectX/Vulkan translation runtime, ReShade and Remix rendering), the game-binary tool, the virtual joystick driver, shared leaderboards, and anything that needs a hosted service.
- Not yet verified in the running game: telemetry packet layouts beyond the documented ones, race.ini details, setup folder layout, server log patterns.

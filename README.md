# ACXS Manager

ACXS Manager is one Rust desktop program for Assetto Corsa. It brings several
separate management jobs into a single window: browsing installed content,
editing content metadata and CSP settings, managing setups, lap records and
replays, dedicated server files, graphics injectors, and a live telemetry view.

It is a Windows program. The user interface is built with egui/eframe. The core
logic is a library (`acxs_manager`) with no UI code, so it can be tested on the
host and cross-compiled for Windows.

## What it does today

- **Install and launch:** finds installs through Steam library folders, checks a
  folder picked by hand, detects whether the game is running, starts the game or a
  third-party launcher profile, and installs cars and tracks from zip archives.
- **Content:** scans cars and tracks, filters and searches them, shows specs and
  thumbnails, compares two cars or two tracks, edits car and track metadata, runs
  content health checks with optional repairs, enables and disables content without
  deleting it, keeps collections, and exports lists to CSV or JSON.
- **Skins and tuning:** scans and imports skins, recolours skin textures, makes
  licence plates, and edits a car's engine, mass, fuel, gear and final drive values.
- **Models:** inspects KN5 models, exports OBJ and FBX, writes KN5 from OBJ, renders
  preview images, bakes AO values, and lists `data.acd` entries.
- **Setups:** lists, imports and archives setup files, compares them, edits values,
  and runs a three-way merge.
- **CSP and game settings:** installs, switches and removes the CSP lights patch
  (with rollback), edits CSP config, force-feedback, weather presets, car classes
  (create, remove, assign) and car years, reads gamepad settings, manages user Lua
  apps, edits `video.ini` and `assetto_corsa.ini`, and makes and restores
  configuration snapshots.
- **Graphics:** switches the active graphics injector between the lights patch, an
  injector build folder, a runtime build folder or none, and manages those folders.
- **Racing:** writes a quick-race config and starts the game with named presets,
  keeps lap records with a delta trace, views replays, and offers race engineer,
  setup, strategy and timing-tower views. Championships are scored from CSV results.
- **Server:** edits the dedicated server configuration and entry list, starts
  and stops the server process, and shows live server status.
- **Telemetry and logs:** shows live UDP telemetry with charts and a track map,
  reads the game's shared memory on Windows, exports telemetry and laps to CSV,
  serves a local web dashboard, edits a HUD layout, summarizes game logs, and
  writes a diagnostics bundle.

The full feature list, with the status of each feature and the reason for anything
not built, is in [ROADMAP.md](ROADMAP.md).

## Building

See [BUILD.md](BUILD.md) for the full steps. In short:

- Windows release binary from macOS: `./scripts/build-windows.sh`
  (needs Homebrew `llvm` and `lld`, `cargo-xwin` and the
  `x86_64-pc-windows-msvc` target). Output: `dist/ACXS-Manager.exe`.
- Windows native: `cargo build --release` with the MSVC toolchain.

## Tests

```sh
cargo test --lib
```

This runs the unit tests of the core library modules. The user interface is in the
binary and has no automated tests.

Last run: 907 tests, all passed (macOS host, `cargo test --lib --locked`). The
Windows CI workflow listed in ROADMAP.md is not in the repository yet.

## Project layout

```
acxs-manager/
  Cargo.toml            package `acxs-manager` (library `acxs_manager` and a binary)
  src/
    main.rs             eframe entry point: window setup, starts the UI
    lib.rs              core library: declares the modules below
    ui/                 egui front end
      mod.rs            app state, background work, message channel
      pages.rs          Home, Cars, Tracks and CSP Lights Patch pages
      theme.rs          colors, spacing, widget styling
      widgets.rs        pills, stat tiles, section headers, list cards
      tools/            one file per Tools page (registered in tools/mod.rs)
  scripts/
    build-windows.sh   cross-compile for Windows and copy the executable into dist/
    package-release.sh zip the executable and write its SHA-256 sum
  docs/
    csp-lights-patch-analysis.md   static analysis notes for the two known patch builds
  dist/                 release executable, zip and checksums from the scripts
  BUILD.md              build instructions for macOS and Windows
  ROADMAP.md            feature coverage matrix
```

Core modules by feature area (all in `src/`):

- **Install and launch:** `steam.rs`, `settings.rs`, `game.rs`, `launcher.rs`,
  `installer.rs`, `util.rs`.
- **Content:** `content.rs`, `query.rs`, `dashboard_stats.rs`, `mods.rs`,
  `collection.rs`, `content_meta.rs`, `content_repair.rs`, `content_export.rs`,
  `cardata.rs`, `car_compare.rs`, `car_classes.rs`, `track_surfaces.rs`,
  `track_compare.rs`, `skins.rs`, `skin_import.rs`, `livery.rs`,
  `license_plate.rs`, `thumbnails.rs`, `png.rs`, `car_tuning.rs`.
- **Models:** `kn5.rs`, `kn5_export.rs`, `kn5_writer.rs`, `obj_import.rs`, `fbx.rs`,
  `ao_bake.rs`, `ortho.rs`, `acd.rs`.
- **Setups:** `setups.rs`, `ini.rs` (lenient INI reader and writer, keeps comments
  and key order), `ini_merge.rs`, `setup_advice.rs`.
- **CSP, graphics and settings:** `csp.rs`, `csp_config.rs`, `ffb.rs`, `weather.rs`,
  `gamepad.rs`, `rare.rs`, `lua_apps.rs`, `shift_lights.rs`, `game_cfg.rs`,
  `backup.rs`, `graphics_profiles.rs`, `graphics_check.rs`, `reshade.rs`, `remix.rs`.
- **Racing and server:** `race.rs`, `presets.rs`, `laptimes.rs`, `delta.rs`,
  `replay.rs`, `replay_player.rs`, `engineer.rs`, `strategy.rs`, `timing.rs`,
  `championship.rs`, `server.rs`, `server_process.rs`, `server_status.rs`.
- **Telemetry, logs and updates:** `telemetry.rs`, `udp_client.rs`,
  `telemetry_csv.rs`, `charts.rs`, `track_map.rs`, `sharedmem.rs`, `hud.rs`,
  `web_dashboard.rs`, `ac_log.rs`, `diagnostics.rs`, `update.rs`.

## Status

- Working and reachable from the UI: game folder detection, car and track scan,
  CSP lights patch install, switch, uninstall and rollback, content metadata edits,
  health checks with repairs, setups including value edits, skins, collections,
  quick race, dedicated server files and process control, lap records, replay
  viewing, live telemetry with charts, KN5 export and preview, graphics injector
  switching, configuration snapshots, the diagnostics bundle, the install-from-zip
  page, live server status, and the shared memory reader.
- Partial: a few pages stop short of the full feature. Examples: the HUD layout
  (no in-game drawing), AO baking (values not written), and car tuning (tyre and
  suspension values not covered). ROADMAP.md names what is missing for each.
- Not started: the Planned rows in ROADMAP.md, such as the online content catalog,
  shared leaderboards, server uploads, hardware outputs and the Windows CI workflow.

The CSP gamepad config file location has not yet been checked against a real game
install. The page shows a warning about this.

Build and test results above were recorded from the repository state at the time
of writing. Run `cargo test --lib` before relying on them.

## Analysis notes

[docs/csp-lights-patch-analysis.md](docs/csp-lights-patch-analysis.md) records the
static analysis of the two known lights patch builds: file sizes, SHA-256 digests,
PE exports and imports, and the differences between the two `extension/` trees. It
is analysis only. Nothing was executed.

# ACXS Manager

ACXS Manager is one Rust desktop program for Assetto Corsa. It brings several
separate management jobs into a single window: browsing installed content,
editing content metadata and CSP settings, managing setups, lap records and
replays, dedicated server files, and a live telemetry view.

It is a Windows program. The user interface is built with egui/eframe. The core
logic is a library (`acxs_manager`) with no UI code, so it can be tested on the
host and cross-compiled for Windows.

## What it does today

- **Content:** scans the game folder for cars and tracks, filters them, shows
  specs, compares two cars or two tracks, edits car and track metadata, runs
  read-only content checks, enables and disables content without deleting it,
  keeps named collections, and manages skins.
- **CSP lights patch:** recognizes the known builds (0.2.11 and 0.3.0-preview675)
  by SHA-256, and installs, switches or removes the patch. Displaced files are
  moved to an archive folder, a failed install rolls back, and nothing is deleted.
- **CSP and game settings:** edits CSP config files, force-feedback tuning and the
  gamepad assist settings. Edits `video.ini`, and makes and restores configuration
  snapshots.
- **Setups:** lists, imports and archives setup files, compares them, and runs a
  three-way merge.
- **Racing:** writes a quick-race config and starts the game with named presets.
  Keeps local lap records with a lap delta trace, and views replay files.
- **Server:** edits the dedicated server configuration and entry list.
- **Telemetry:** connects to the game's built-in UDP telemetry, shows the latest
  car sample and completed laps, and exports telemetry and laps to CSV.
- **Models:** inspects KN5 models, exports mesh geometry to OBJ, and lists
  `data.acd` archive entries.
- **Launch:** checks whether the game is running and starts it from the game folder.

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
binary and has no automated tests. The same command runs in the Windows CI workflow.

Last run: 396 tests, 395 passed, 1 failed. The failing test is
`csp::tests::patch_differences_cover_version_family_count_and_size`, which expects a
byte total that does not match the value its fixture computes. The test needs a
fix before the suite passes.

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
    build-windows.sh    cross-compile for Windows and copy the executable into dist/
  .github/workflows/
    windows-build.yml   Windows CI: library tests, release build, artifact upload
  docs/
    csp-lights-patch-analysis.md   static analysis notes for the two known patch builds
  dist/                 release executable from the build script
  BUILD.md              build instructions for macOS and Windows
  ROADMAP.md            feature coverage matrix
```

Core modules by feature area (all in `src/`):

- **Install and launch:** `steam.rs` (install detection), `settings.rs` (saved
  folders), `game.rs` (running check and launch), `util.rs` (shared helpers).
- **Content:** `content.rs` (scan), `content_meta.rs` (metadata edits),
  `content_repair.rs` (checks), `content_export.rs` (CSV and JSON lists),
  `query.rs` (search language), `dashboard_stats.rs` (counts), `mods.rs`
  (enable and disable), `collection.rs`, `cardata.rs`, `car_compare.rs`,
  `car_classes.rs`, `track_surfaces.rs`, `track_compare.rs`, `skins.rs`,
  `skin_import.rs`, `kn5.rs`, `kn5_export.rs`, `acd.rs`.
- **Setups:** `setups.rs`, `ini.rs` (lenient INI reader and writer, keeps comments
  and key order), `ini_merge.rs` (three-way merge).
- **CSP and settings:** `csp.rs` (lights patch), `csp_config.rs`, `ffb.rs`,
  `weather.rs`, `gamepad.rs`, `rare.rs` (race rules settings), `lua_apps.rs`,
  `shift_lights.rs`, `game_cfg.rs` (`video.ini`, `assetto_corsa.ini`), `backup.rs`.
- **Racing:** `race.rs` (quick-race config), `presets.rs`, `laptimes.rs`, `delta.rs`,
  `replay.rs`.
- **Server:** `server.rs`.
- **Telemetry and logs:** `telemetry.rs` (UDP packet decoder), `udp_client.rs`,
  `telemetry_csv.rs`, `ac_log.rs`.

Placeholder files, not implemented yet: `car_tuning.rs`, `kn5_writer.rs`, `png.rs`,
`hud.rs`, `installer.rs`, `license_plate.rs`, `ortho.rs`, `server_process.rs`,
`strategy.rs`, and the `rtx.rs` tool page. Each is listed as Planned or Not feasible
in ROADMAP.md.

## Status

- Working and reachable from the UI: game folder detection, car and track scan,
  CSP lights patch install, switch, uninstall and rollback, content metadata edits,
  setups, skins, collections, quick race, dedicated server file editing, lap records,
  replay viewing, live UDP telemetry view, KN5 OBJ export, configuration snapshots.
- Partial: several library modules exist without a UI page or with a read-only
  page. Examples: content export, weather and car class views, the assetto_corsa.ini
  editor and the Lua app list. ROADMAP.md names what is missing for each.
- Not started: the placeholder modules above, server process control, shared
  memory telemetry, and the other Planned rows in ROADMAP.md.

The CSP gamepad config file location has not yet been checked against a real game
install. The page shows a warning about this.

Build and test results above were recorded from the repository state at the time
of writing. Run `cargo test --lib` before relying on them.

## Analysis notes

[docs/csp-lights-patch-analysis.md](docs/csp-lights-patch-analysis.md) records the
static analysis of the two known lights patch builds: file sizes, SHA-256 digests,
PE exports and imports, and the differences between the two `extension/` trees. It
is analysis only. Nothing was executed.

# ACXS Manager: redesign plan

Goal: a polished, deploy-ready manager for many players. Keep this file current; tick items as they land.

## Done
- [x] Dark-only theme: palette, Segoe UI + symbol fallback, type scale, toggles, pills, cards
- [x] New layout: sidebar (grouped, collapsible tools), header with live status + launch, status bar
- [x] Home banner with featured car, Surprise me, jump-off cards, content stats
- [x] Settings page (folders, update check, crash reports); first-run onboarding
- [x] Cars: preview grid, skins, filters, sort, favorites, context menu, detail panel
- [x] Tracks: preview grid, layouts, outline map, detail panel
- [x] Showroom: hero image, skin strip, spec bars, description, keyboard control
- [x] Ctrl+K command palette (pages, tools, cars, tracks, actions)
- [x] Toasts, drag-and-drop zip install overlay
- [x] CSP Config: file list with search, grouped cards, toggles, changed markers, revert, save bar
- [x] Fix: stock `ui_car.json` with raw newlines now parses (real names, brands, specs)

## Next
1. **Servers**
   - [x] Compact virtualised table (thousands of rows), sortable columns, filter, online-only
   - [x] Join panel rebuilt for one-click joining (see changelog)
   - [x] Right panel: track preview, car chooser from the entry list (free slots, skins, installed
     or not), drivers, collapsible server info and raw entry list
   - [x] Favorites, add/remove, polling with a cap of 24 parallel requests
   - [x] Join starts the game directly (race.ini [REMOTE] + acs.exe), no Content Manager; chosen car/skin, driver name, password
   - [x] Direct join confirmed on a real server (75-car session started from a file ACXS wrote); servers that need a booking (`/SUB`) are still unproven
   - [x] Online tab: Kunos lobby list as Content Manager loads it (`lobby.ashx/list?guid=<SteamID64>`),
     Steam id read live from the signed-in Steam client; hide empty/full/password; star adds to My servers
   - [x] Auto-load at start from a saved copy, background refresh in place, no waiting (tests cover merge, prune, failure)
   - [x] Public list found: acservers.online directory (JSON API, ten servers per page, hasPlayers filter, name search); live test passes and returns the Cruisin USA server
   - [x] Kunos lobby: it redirects every request that does not carry the game's own client identity (Content Manager sends a hidden one from its closed DLL); ACXS Manager identifies itself honestly, so use the new Custom list source (any URL serving the lobby JSON, plain or gzip)
   - [x] Direct join to CSP servers: the file matches a working Content Manager one; port, features and Steam id matched live
   - [x] Servers with CSP track ids (`csp/2651/../la_canyons-freeroam`) resolve to the installed track and layout
   - [x] Public and own servers can be found from Ctrl+K
   - [x] Import many servers: paste several addresses or load a text file
   - [x] Quick Race and Launch start acs.exe with the Steam app id set, so Steam no longer detours through the launcher 
   - [x] "Race this car" / "Race here" fill in Quick Race from Cars, Tracks and the Showroom
   - [x] Keep the lobby summary when a listed server does not answer on its HTTP port
2. **Visual settings changes**
   - [x] Game Settings, video: changed rows marked, was → now list, quality vs performance meter, revert all
   - [x] assetto_corsa.ini has the same view
   - [x] Quick Race: car/track chooser through the Cars/Tracks pages with skins, all sessions, conditions, rules and driver aids
   - [ ] Graphics profiles (a switcher, needs its own design); Setups already has a compare table (RTX & ReShade is disabled for now)
   - [ ] Shared "pending changes" bar (count, revert all, save) reused by every config tool
3. **Restyle remaining tools** (same grid/cards/toggles): Skins, Setups, Replay, Lap times,
   Live telemetry, Collections, Mods (Quick Race and Skins now show previews)
4. **Polish**
   - [ ] Hover/tooltips everywhere, empty states with actions, loading skeletons for images
   - [x] Arrow keys and Enter move through the Cars and Tracks grids
   - [x] Window size and position are remembered (window.json)
   - [x] App icon (drawn in code), About card with version, data folder and shortcuts
   - [x] Changelog view in Settings (reads CHANGELOG.md)
5. **Release readiness**
   - [x] Fixed `content_repair::apply_never_overwrites_existing_files` (Windows reports access denied for a folder in the way)
   - [x] Release build compiles; `scripts/package-windows.ps1` builds the zip with checksum
   - [x] Release zip built with the script (dist/ACXS-Manager-0.3.0-windows-x64.zip, 8.7 MB, checksum written)
   - [x] Updates: a visible card at the top of Settings and a "Check for updates" link in the sidebar; result with notes and a download button
   - [ ] README screenshots

6. **Session statistics on Home**: all phases built and checked on real drives, see `docs/session-stats-plan.md`
   - [x] Phase 0: import Content Manager history (145 sessions) and personalbest.ini, Home tiles
   - [x] Phase 1: recorder from shared memory (30 Hz), session files in the ACXS data folder
   - [x] Phase 2: speed, laps, crashes, airborne, brake lock, understeer/oversteer
   - [x] Phase 3: session page with track map and charts
   - [x] Phase 4: replay archive and replay.ini switch
   - [x] Phase 5: recommendations tied to the setup editor
   - [x] Phase 6 (optional): CSP Lua helper for collision type

## Rules for myself
- Implement first, screenshot rarely (one batch at the end of a chunk).
- Write files with the editor tool, not shell heredocs (backslashes get eaten).
- Only touch the C: system disk; no passwords in files.

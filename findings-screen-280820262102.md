# ACXS Manager — Visual QA Findings (`--screen`)

- **Date**: 2026-11-28
- **Target**: `src-tauri/target/release/acxs-tauri.exe`
- **Visual Verdict**: `ISSUES_FOUND`

---

## Screens & Controls Swept

- **Paddock** (`01-paddock-initial.png`)
- **Drive -> Quick Race & Practice** (`02-drive.png`) & **Select Machine Modal** (`09-drive-change-car-modal.png`)
- **Garage -> 3D Showroom** (`03-garage.png`, `11-garage-3d-showroom-loaded.png`, `12-garage-car-chooser.png`)
- **Online** (`04-online.png`)
- **Server -> Dedicated Server Manager** (`05-server.png`)
- **Telemetry -> Sessions & Corner Coach** (`06-telemetry.png`)
- **Setup -> Game Settings** (`07-setup.png` Display/Video, `13-setup-vr-triple.png` Gameplay & Snapshots)

---

## AAA / Enterprise Visual Bar Assessment

- **Overall**: The dark motorsport HUD aesthetic, typography, color tokens, and 3D Showroom rendering (`12-garage-car-chooser.png`) are sharp and cohesive, **but the app fails the AAA/enterprise bar due to severe horizontal viewport clipping at the default launch window size (`1455 x 929`)**.
- **TopBar & Window Controls**: The right side of the TopBar (`CSP 0.3...` badge and the custom Minimize / Maximize / Close buttons) is completely clipped off-screen on every view.
- **Content Overflow**: Nearly every multi-column page (`Drive`, `Online`, `Server`, `Telemetry`, `Setup`) and the `Select Machine` modal clip their right-hand controls, toggles, and close buttons.

---

## Findings

1. **`[CRITICAL][ui]` TopBar / Custom Window Controls** (`01-paddock-initial.png`)
   - At the default launch window size (`1455 x 929`), the right side of the TopBar (`CSP 0.3...` pill and the custom Minimize / Maximize / Close buttons) is completely clipped off-screen, making custom window controls inaccessible.

2. **`[HIGH][ui]` Drive -> Quick Race & Practice** (`02-drive.png`)
   - The right-hand `TIME OF DAY & WEATHERFX` panel overflows horizontally, clipping the `Drag...` badge, `Exact Time of Day` numeric readout, and `pureCtrl` pills.

3. **`[HIGH][ui]` Drive -> Select Machine Modal** (`09-drive-change-car-modal.png`)
   - The modal extends past the right edge of the viewport, clipping the modal close button, search input right edge, brand filter pills, and right-side car cards.

4. **`[HIGH][ui]` Setup -> Game Settings** (`07-setup.png`, `13-setup-vr-triple.png`)
   - Right-hand controls are clipped: `MSAA Anti-Aliasing` (`8x`) and `Anisotropic Filtering` (`16x`) buttons in `Display, Video & Camera`, and the `Auto-Blip on Downshift` toggle in `Gameplay & Snapshots` are cut off.

5. **`[MEDIUM][ui]` Online / Server / Telemetry** (`04-online.png`, `05-server.png`, `06-telemetry.png`)
   - Horizontal overflow clips the `Players:` filter pill (`Online`), `CSP EXTRA OPTIONS` panel (`Server`), and `BRAKE LOCKUPS` / `CIRCUIT TIME-LOSS HOTSPOTS` cards (`Telemetry`).

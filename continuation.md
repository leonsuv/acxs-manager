# continuation.md: Stand und Fortsetzung

**Start hier:** Nach dem Leeren des Chats zuerst diese Datei lesen, dann `plan.md` und
`docs/session-stats-plan.md`, dann `C:\Setup-Notes\WINDOWS_SETUP.md` (Regeln dieses PCs).

Zweck dieser Datei: Wer (oder welche Claude-Sitzung) hier weitermacht, findet den ganzen Stand an
einem Ort. Zuletzt aktualisiert am 6. November 2026. Ergänzend: `plan.md` (Aufgabenliste mit Häkchen),
`docs/session-stats-plan.md` (Plan für die Statistiken), `CHANGELOG.md` (Abschnitt "Unreleased").

## 1. Projekt in einem Absatz

ACXS Manager ist ein Rust-Programm mit egui/eframe (Version 0.37, Edition 2024, Rust 1.88 oder neuer)
für Assetto Corsa unter Windows. Eine Bibliothek (`acxs_manager`, `src/*.rs`) enthält die Logik ohne
Oberfläche, die Oberfläche liegt in `src/ui/`. Repo: `C:\Users\leob\Documents\acsx-manager`
(GitHub `leonsuv/acxs-manager`, privat). Alles ist lokal auf `main` committet, **nichts gepusht**. Das
Repo hat keine Git-Identität: Commits mit `git -c user.name=leonsuv -c user.email=leon.suvorkov@outlook.de`.

## 2. Regeln dieses PCs und des Nutzers (unbedingt beachten)

Aus `C:\Setup-Notes\WINDOWS_SETUP.md` und dem Gespräch:

- **Nur die Systemplatte C: anfassen** ("WD Blue SN580 1TB"). Alle anderen Laufwerke enthalten sensible
  Daten: nie durchsuchen, nie beschreiben. Kein `Get-ChildItem -Recurse` über andere Laufwerksbuchstaben.
- **Keine Passwörter in Dateien.**
- Der Nutzer schreibt **Deutsch und Englisch**, will schnelles Handeln und kurze Antworten.
  Antworten auf Deutsch, wenn er Deutsch schreibt.
- **Keine Unter-Agenten** ("Nix subagents"). Alles selbst erledigen.
- **Wenige Screenshots**: erst bauen und umsetzen, dann einmal prüfen. Nicht viele Testzyklen vor den
  Augen des Nutzers. Er hat auch gesagt: "Don't ask, just go" und "mach weiter, bis der Plan fertig ist".
- Nicht den Fokus stehlen und nichts im **VR-Headset** (HP Reverb G2) aufblitzen lassen, während der
  Nutzer fährt. Das Spiel selbst habe ich deshalb nie gestartet; Joins und Quick Race sind ungetestet.
- Die Anfragen des Nutzers sind knapp und oft ungeduldig. Ehrlich bleiben: was ungetestet ist, sagen.

## 3. Bauen, starten, prüfen

```text
Cargo:        C:\Users\leob\.cargo\bin\cargo.exe   (nicht im PATH der Shell; in Bash:
              export PATH="$PATH:/c/Users/leob/.cargo/bin")
Bauen:        cargo build            (Debug; Release: cargo build --release, rund 3 Minuten)
Testen:       cargo test             (1047 Bibliothekstests + 7 Tests der Oberfläche, 1 Test ignoriert)
Netztest:     cargo test --lib live_directory -- --ignored --nocapture   (ruft acservers.online auf)
Starten:      target\debug\acxs-manager.exe  (mit Start-Process aus PowerShell)
Paket:        powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
              (baut Release, schreibt dist\ACXS-Manager-0.3.0-windows-x64.zip und SHA256SUMS.txt)
```

**Falle:** Läuft die App, ist `target\debug\acxs-manager.exe` gesperrt und `cargo build` scheitert mit
"Zugriff verweigert". Vorher beenden: `taskkill //F //IM acxs-manager.exe` (Bash) oder
`Get-Process acxs-manager | Stop-Process -Force` (PowerShell). Danach neu starten.

Die App-Daten liegen in `%APPDATA%\ACXS\`: `settings.json`, `servers.json` (eigene Server),
`directory-cache.json` (gespeicherte öffentliche Serverliste), `lobby-source.json`, `window.json`
(Fenstergröße), `favorites.json`, `sidebar.json` (angeheftete Einträge; entsteht beim ersten Anheften), Absturzberichte (nur wenn eingeschaltet).

## 4. Architektur

### Bibliothek (`src/`)
- `lobby.rs`: Serverlisten im Lobby-JSON-Format, Steam-ID des angemeldeten Nutzers (Registry
  `ActiveUser`, sonst `loginusers.vdf`), HTTP-Client `web_agent` (auf Windows mit SChannel).
- `directory.rs`: Anbindung an **acservers.online** (`/api/v1/servers`, zehn Server je Seite,
  `hasPlayers=true`, `q=` für die Suche), mit `fetch_stream` für schrittweises Laden.
- `server_status.rs`: fragt den HTTP-Port eines Servers ab (`/INFO`, `/JSON|-1`), liefert Name, Track,
  Spielerzahlen, Rennport, Fahrzeugliste mit Skins, Feature-Liste, Rohantworten.
- `race.rs` und `race_options.rs`: `race.ini` für Quick Race **und** für Online-Joins
  (`OnlineJoin`), Sessions (Training, Qualifying, Rennen), Bedingungen, Regeln und Fahrhilfen
  (`assists.ini`, mit Sicherungskopie). Presets (`presets.rs`) speichern die Extras mit.
- `game.rs`: `launch_game` startet `acs.exe` **mit gesetzten Umgebungsvariablen `SteamAppId` und
  `SteamGameId` (244210)**, damit Steam nicht über den Launcher (hier der Content Manager) umleitet.
- `content.rs`: liest Autos, Tracks (`read_ui_json` kann jetzt rohe Zeilenumbrüche in Strings, wie sie
  in den Kunos-`ui_car.json` stehen).
- `sharedmem.rs`, `telemetry.rs`, `replay.rs`: Shared Memory, UDP-Telemetrie, `.acreplay`-Parser
  (für die geplanten Statistiken wichtig).

### Oberfläche (`src/ui/`)
- `mod.rs`: `App` (Zustand), Seitenleiste (`nav`), Kopfzeile, Statusleiste, Befehlspalette (Strg+K),
  Toasts, Drag-and-drop für Zip-Pakete. `Page` ist Home, Showroom, Servers, Cars, Tracks, Patches,
  Settings oder `Tool(index)`.
- `theme.rs`: Farben, Segoe UI als Schrift (mit Symbolschrift als Ersatz), nur dunkles Design
  (`set_theme(Dark)` ist nötig, sonst greift der helle Windows-Stil).
- `widgets.rs`: Pillen, Kacheln, Schalter (`toggle`), Seitenleisten-Einträge, **gezeichnete Symbole**
  (`Icon`, `draw_icon`), Raster-Hilfen.
- `media.rs`: Vorschaubilder (Auto: `skins\*\preview.jpg`, Strecke: `ui\...\preview.png`), Bilder
  zwischenspeichern, Kartenraster (`grid`) mit Stern, Rechtsklickmenü, Tastatur.
- `pages.rs`: Home (Banner mit Auto), Einstellungen (Updates, Ordner, Absturzberichte, Über),
  Autos, Tracks, Lights-Patch.
- `showroom.rs`: ein Auto groß, Skins, Daten aus `ui_car.json`.
- `servers.rs` und `servers/detail.rs`: Serverliste (eigene und öffentliche), Tabelle mit tausenden
  Zeilen, Join-Panel rechts, Zwischenspeicher der öffentlichen Liste.
- `window_state.rs`: Fenstergröße und -position merken.
- `tools/*.rs`: 30 Werkzeug-Seiten (`Tool`-Trait). RTX & ReShade ist **abgeschaltet**
  (`// rtx::tool()` in `tools/mod.rs`; im Graphics-Profil ausgeblendet über `SHOW_RESHADE_AND_REMIX`).

## 5. Was fertig ist

- **Neues Design** (dunkel, Segoe UI), Seitenleiste mit Bereichen **Play** (Home, Quick Race, Servers),
  **Garage** (Cars, Tracks, Showroom, Skins, Setups, Mods), **Tuning** (CSP Config, Game Settings,
  Lights Patch), **Driving data** (Lap Times, Replay Viewer, Live Telemetry) und aufklappbarem
  **"More tools"** (21 weitere). Rechtsklick heftet Einträge oben an. Settings unten.
- **Home**: Banner mit Auto des Tages ("Surprise me"), Einstieg in Showroom, Server, Quick Race,
  Inhaltsstatistik. Erststart-Seite für den Spielordner.
- **Cars/Tracks**: Kartenraster mit echten Vorschaubildern, Skins, Filter, Sortierung, Favoriten,
  Tastatursteuerung, Detailspalte. **Showroom** mit Skin-Leiste, Datenbalken, Beschreibung.
- **Strg+K**: Seiten, Werkzeuge, Autos, Tracks und Server finden.
- **Updates**: Karte oben in den Einstellungen und Link unten in der Seitenleiste.
- **CSP Config**: Dateiliste mit Suche, Karten, Schalter, geänderte Werte markiert mit Rückgängig,
  Speicherleiste. **Game Settings**: Vorher/Nachher für `video.ini` und `assetto_corsa.ini`,
  Qualitäts- und Leistungsanzeige.
- **Quick Race neu**: Bilder von Strecke und Auto, "Choose track" und "Choose car and skin" öffnen
  Cars/Tracks im Auswahlmodus und bringen die Wahl zurück; Sessions, Wetter, Wind, Temperaturen,
  Regeln, Fahrhilfen mit Schnellprofilen. Aus Autos, Tracks und Showroom: "Race this car", "Race here".
- **Servers**:
  - Tab "My servers": Adressen eintragen, einfügen, importieren, Sterne, Live-Abfrage.
  - Tab "Online": Quelle **AC Servers directory** (acservers.online), Kunos-Lobby, eigene Liste.
    Die Liste lädt **beim Start** automatisch; die gespeicherte Liste der letzten Sitzung erscheint sofort,
    ein Hintergrundabgleich aktualisiert Einträge an Ort und Stelle und entfernt, was nicht
    zurückkam. Offener Tab erneuert sich nach fünf Minuten. Suche über alle rund 10.800 Server.
  - **Join-Panel**: großer JOIN-Knopf oben mit Autoname, automatisch gewähltes Auto, Autoraster
    mit Suche und "Mine only", Skins, Doppelklick auf ein Auto tritt sofort bei, klare Gründe, wenn
    Join nicht geht, gemerkte Wahl je Server.
  - Track-IDs mit `csp/2651/../la_canyons-freeroam` werden dem installierten Track und Layout
    zugeordnet.
- Fenstergröße wird gemerkt, Fenstersymbol, Über-Karte mit Tastenkürzeln und Changelog, Release-Zip-Skript,
  README mit Screenshots (`docs/screenshots/`), Test für Windows-Pfadfehler repariert.

## 6. Stand der Prüfungen (6. November 2026)

**Am echten Spiel belegt:**
- **Quick Race** läuft: `race.ini`-Sicherungen von 15:05, 15:17 und 15:20 am 1. Nov. passen zu Sessions, die
  Sekunden später starteten.
- **Server-Join** läuft: Datei um 15:20:19 von ACXS geschrieben, Session 30 s später mit 75 Autos im Replay,
  877 s lang. Die Join-Datei hat dieselben Schlüssel wie eine funktionierende vom Content Manager;
  `examples/join_check.rs` zeigte live: Rennport, 8 Features und Steam-ID stimmen exakt.
- **Rekorder** läuft (drei echte Sessions). Shared-Memory-Layout live bestätigt mit `examples/shm_dump.rs`:
  Tempo passt zum Geschwindigkeitsvektor, Raddrehzahl zum Tempo, Querbeschleunigung zu Gierrate mal Tempo.
- **Aktualisierung** (`live_update_feed`) und **Verzeichnis** (`live_directory`) bestehen live.

**Nur auf Daten oder Dateiebene geprüft, im Spiel noch nie gelaufen:**
- Lua-App ACXS Collisions: gegen CSP-SDK und CSP-eigene Apps gebaut, aber nie im Spiel gestartet.
  Der Nutzer hat sie nicht installiert. Bei `LAZY = NONE` sollte sie ohne Klick im Spiel laufen.
- Der neue Crash-Stand (2,5 g oder 4,5 g mit Bremse, 15 km/h Verlust) ist gegen das Ferrari-Replay
  durchgerechnet, aber seit der Änderung nicht gefahren. Referenz: `examples/replay_events.rs`.
- Handling ist eine Schätzung ohne Wahrheitsquelle. Normale Fahrt ergab 18 bis 36 %, darum Empfehlung erst ab 35 %.
- Seiten nach den letzten Änderungen (Sessions-Karte mit Namen und Löschen, Championship-Knopf) sind
  gebaut, aber nicht angesehen: ACXS durfte zuletzt nicht gestartet werden. Karten aus echten Daten lassen sich
  ohne App prüfen: `examples/session_map.rs`.
- Server mit Buchung (`/SUB`) sind unbewiesen.

**Offen, nur mit dem Nutzer lösbar:**
- RTX & ReShade bleibt aus (Remix kann nur Direct3D 9). Ob ReShade allein zurückkommen soll, entscheidet er.
- README-Screenshots brauchen eine laufende App.
- Vier Fragen aus Abschnitt 8. Vorgaben gelten: nur eigenes Auto, keine Detaildaten-Löschfrist, App muss offen sein.
- Push und eine neue Version im Update-Feed sind seine Entscheidung.

**Bewusst nicht gebaut:** siehe `ROADMAP.md`: Partial und Planned brauchen Hardware, einen Server, Inhalte Dritter
oder neue In-Game-Apps (HUD-Anzeige, Delta-Balken). Gestalterisch offen: gemeinsame Speicherleiste,
Vorher/Nachher für Graphics Profiles, weitere Werkzeuge im neuen Stil, mehr Hover-Hilfen.

## 7. Wichtige Erkenntnisse (nicht noch einmal herausfinden)

- **Kunos-Lobby** (`93.57.10.21/lobby.ashx/list?guid=<SteamID64>`): Der Content Manager sendet einen
  verborgenen User-Agent (`InternalUtils.GetKunosUserAgent()`, in einer geschlossenen DLL). Jede andere
  Anfrage wird mit HTTP 302 auf die Startseite geleitet. Diese Kennung nachzuahmen wurde abgelehnt
  (wäre Tarnen als fremder Client). Deshalb das Verzeichnis **acservers.online**; die Kunos-Quelle bleibt
  als Option erhalten. Antwort ist JSON-Array mit `ip`, `cport` (HTTP), `port`/`tport` (Rennen), `name`,
  `clients`, `maxclients`, `track`, `cars`, `session`, `pass`.
- **acservers.online**: offene JSON-Schnittstelle, 10 Einträge je Seite (`limit` wirkt nur bis 10),
  `hasPlayers=true` (rund 420 Server), `q=<Text>` sucht über alle, Eintrag enthält `joinUrl` im Format
  `acmanager://race/online/join?ip=...&httpPort=...`. Antworten werden öffentlich zwischengespeichert.
  Die robots.txt verbietet nur KI-Trainings-Crawler; wir fragen höflich einzeln mit ehrlichem
  User-Agent `ACXS-Manager/<Version>`.
- **Steam-Umleitung**: `acs.exe` ohne `SteamAppId` lässt Steam das Spiel über `AssettoCorsa.exe`
  neu starten, und das ist auf diesem PC der Content Manager. Darum die Umgebungsvariablen in
  `game::launch_game`.
- **Online-`race.ini` wie der Content Manager** (aus dessen Quelltext `Game.Properties.cs`):
  `[REMOTE]` mit `SERVER_IP`, `SERVER_PORT`, `SERVER_HTTP_PORT`, `SERVER_NAME`, `REQUESTED_CAR`,
  `GUID`, `PASSWORD`, `ACTIVE`, `__CM_EXTENDED`, `__FEATURES`; ein `[SESSION_0]` vom Typ Training.
- `/INFO` des Servers liefert `port`/`tport` (Rennen) und `cport` (HTTP); `/JSON|-1` liefert
  `Features` und `Cars` (`Model`, `Skin`, `DriverName`, `IsConnected`, `IsRequestedGUID`).
  Skins können `name/ADAn` heißen, der Ordnername ist der Teil vor dem Schrägstrich.
- Das Spiel hat **kein** Ordnerverzeichnis `content\tracks\csp\...`; Servereinträge mit
  `csp/<Version>/../<Track>` verlangen eine ausreichend neue CSP-Version und werden vom Spiel mit CSP
  aufgelöst.
- Der Nutzer hat **keinen** `Documents\Assetto Corsa\out`-Ordner (keine `race_out.json`).

## 8. Geplant: Statistiken auf dem Home Screen (Kurzfassung)

Volltext in `docs/session-stats-plan.md`. Befund auf diesem PC:

- Der **Content Manager** führt schon eine Historie: `%LOCALAPPDATA%\AcTools Content Manager\Progress\
  Profile (Sessions).data` (145 Sessions seit 14.04.2026 mit Crashes, Offroad, Höchstgeschwindigkeit,
  Distanz, Luftzeit, längstem Sprung, Sprit, Reifenverschleiß, Bestzeit) und `Progress\Sessions\*.json`
  (148 Dateien mit Runden, Sektoren, Cuts, Server). Sofort importierbar. Sessions, die ACXS direkt
  startet, fehlen dort.
- **Replays** (`Documents\Assetto Corsa\replay\temp\AC_<Datum>-<Zeit>_<R|O>_<Auto>_<Strecke>.acreplay`)
  sind keine Historie: `cfg\replay.ini` begrenzt auf 26 MB, 2 Rennen, 1 Qualifying, 1 Sonstiges. Sie
  enthalten Tempo, Drehzahl, Gang, Gas, Bremse, Position, aber keinen Schlupf oder Lenkwinkel.
  Plan: nach jeder Session archivieren und die vorhandenen nachträglich auswerten.
- **Eigener Rekorder** aus dem Shared Memory mit rund 30 Hz (Schlupf, Radlast, Federweg, Raddrehzahl,
  Gierrate, G-Kräfte, Schaden, Lenkwinkel, Streckenposition, Reifen- und Bremstemperaturen):
  Untersteuern und Übersteuern (Soll- gegen Ist-Gierrate), Blockieren (Raddrehzahl gegen Tempo),
  Sprünge (alle vier Räder ohne Last, Höhe, Aufprall), Crashes (Schaden, G-Spitze, Tempoeinbruch).
  Kollision mit Auto oder Wand zu unterscheiden braucht später eine kleine CSP-Lua-App.
- Speicherung: eine JSON-Datei je Session in `%APPDATA%\ACXS\sessions\`, optional ein auf 5 Hz
  reduzierter Verlauf. Alles lokal.
- **Phasen:** 0 Import und Kacheln, 1 Rekorder-Kern, 2 Kennzahlen, 3 Session-Seite mit Streckenkarte,
  4 Replay-Archiv und (`replay.ini`-Schalter ist drin: Knopf auf der Sessions-Seite), 5 Empfehlungen (mit Setup-Beratung verbunden),
  6 optionale Lua-App.
- **Offene Fragen an den Nutzer:** Hintergrunddienst beim Windows-Start? Wie lange Detaildaten
  aufheben? Fremde Autos auf Servern mitschreiben? Kollisionsart früh nötig?

## 9. Arbeitsweise und Fallen

- **Dateien schreiben**: Mit dem Schreib- und Bearbeitungswerkzeug, nicht mit Shell-Heredocs und `sed`
  für Code mit Backslashes. Bash frisst `\\` und `\n` (Rust-Zeichenketten wie `"\r\n"` oder `'\\'`
  werden zerstört). Das ist mehrfach passiert.
- **egui 0.37**: `Panel::left/right/top/bottom(..).show(ui, ..)` (nicht `show_inside`),
  `ui.close()` im Menü, `Id::new` ist veraltet (`#[allow(deprecated)]` oder `Id::unique`),
  `ui.scope_builder(...)` **setzt den Cursor des Elternelements nach oben**, wenn der Inhalt
  kürzer ist (darum Abstand nachholen), `ui.allocate_ui` erbt die Anordnung des Elternelements
  (`allocate_ui_with_layout` nehmen), `show_rows` braucht gleich hohe Zeilen.
- **Build-Hygiene**: Nach Änderungen immer `cargo build` und `cargo test`. Keine Warnungen stehen lassen.
- **Screenshots ohne Fokus**: PowerShell-Skript mit `PrintWindow` (DPI-bewusst) und kurzzeitig
  eingebauten Starthaken (`// TMP-A` in `App::new`, per Umgebungsvariable `ACXS_START_PAGE`), die nach
  der Aufnahme wieder entfernt werden (`grep -rn TMP src` muss leer sein). Für Server ein
  Scheinserver (HttpListener auf `127.0.0.1:8081`) und `APPDATA` auf einen Temp-Ordner umbiegen, damit
  Nutzerdaten nicht auftauchen. Skripte liegen im Scratchpad der Sitzung, nicht im Repo.
- **Doku mitpflegen**: `plan.md` (Häkchen), `CHANGELOG.md` (Unreleased) und README bei jeder größeren
  Änderung anfassen.
- **Neue Tests** für Logik in der Bibliothek (Parser, Zusammenführen, Regeln). Oberflächentests nur
  dort, wo reine Funktionen stecken.

## 10. Nächste Schritte und Fallen aus der letzten Runde

Neu seit dem 6. November, gebaut und getestet, aber **noch nie im Spiel gelaufen und nicht angesehen**:
Quick-Race-Modi (Werte aus den Kunos-`event.ini`), Seite Events (379 Events gelesen), Game Options, Replay-Bibliothek,
Screenshots, Knopf "Open in the game" im Showroom (`acShowroom.exe`), Lap-Tabelle je Session, Overlay-App
`ACXSOverlay` (`src/overlay_app.rs`), "Copy content to server", beigetretene Server bleiben in My servers.

**Screenshots sind vom Nutzer verboten, bis er sie wieder erlaubt.** Das schließt `render_pages` in
`src/ui/tools/mod.rs` ein (PNGs ohne Fenster). Erlaubt ist `every_page_draws_with_the_real_install`: zeichnet alle Seiten
ohne Bilder. Windows-CI hat der Nutzer abgelehnt.

Offen im Roadmap: Rest braucht Hardware, Dienste, Drittinhalte oder eine Entscheidung. AO-Werte gehen nur in eine CSV.

- **Nie Quelltext über PowerShell `Get-Content | Set-Content` umschreiben.** Das hat `src/ui/mod.rs` zerstört
  (`…` wurde zu `â€¦`, dazu ein BOM) und ging bis in einen Commit. Danach prüfen:
  `grep -rnP 'â€|Â·' src` muss leer sein.
- **Vor jedem Commit `cargo check --all-targets`**, nicht nur `cargo test --lib`: die Oberfläche ist ein eigenes Ziel.
- Während der Nutzer fährt: `nice -n 19` und `CARGO_BUILD_JOBS=2`, laufendes ACXS nie beenden, Spiel und ACXS
  nicht starten. Lesendes `examples/shm_dump.exe` stört nicht.
- Ein Testlauf ohne Spiel: `scripts/fake-ac-drive.ps1` mit `ACXS_SHM_PREFIX=acxstest_` und `examples/recorder_probe.rs`.
  `APPDATA` dabei auf einen Temp-Ordner umbiegen. Schalter `-NoSpline` und `-FreezeSeconds` spielen die echten Fälle nach.
- Der Rekorder zeichnet nur auf, solange ACXS offen ist. Die Fahrten vom 2., 3. und 6. November fehlen deshalb.

Nächster sinnvoller Schritt: eine Fahrt mit offenem ACXS, dann `%APPDATA%\ACXS\recorder.log` und die neue Sessions-Seite ansehen.

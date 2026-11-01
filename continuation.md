# continuation.md: Stand und Fortsetzung

**Start hier:** Nach dem Leeren des Chats zuerst diese Datei lesen, dann `plan.md` und
`docs/session-stats-plan.md`, dann `C:\Setup-Notes\WINDOWS_SETUP.md` (Regeln dieses PCs).

Zweck dieser Datei: Wer (oder welche Claude-Sitzung) hier weitermacht, findet den ganzen Stand an
einem Ort. Zuletzt aktualisiert am 1. November 2026. Ergänzend: `plan.md` (Aufgabenliste mit Häkchen),
`docs/session-stats-plan.md` (Plan für die Statistiken), `CHANGELOG.md` (Abschnitt "Unreleased").

## 1. Projekt in einem Absatz

ACXS Manager ist ein Rust-Programm mit egui/eframe (Version 0.37, Edition 2024, Rust 1.88 oder neuer)
für Assetto Corsa unter Windows. Eine Bibliothek (`acxs_manager`, `src/*.rs`) enthält die Logik ohne
Oberfläche, die Oberfläche liegt in `src/ui/`. Repo: `C:\Users\leob\Documents\acsx-manager`
(GitHub `leonsuv/acxs-manager`, privat). Es gibt einen Git-Stand, aber **61 Änderungen sind noch nicht
eingecheckt**; es wurde nichts committet oder gepusht, weil es niemand verlangt hat.

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

## 6. Offene Punkte (Reihenfolge nach Nutzen)

1. **Join zu CSP-Servern prüfen.** Stand: Das Spiel startet direkt (ohne Content Manager) und
   verbindet. Zuletzt kam "Can't join this server: track csp/2651/../la_canyons is missing". Daraufhin
   wurde auf das Verhalten des Content Managers umgestellt: `TRACK`/`CONFIG_TRACK` mit dem **lokalen**
   Track (`la_canyons`, `freeroam`), und im `[REMOTE]`-Abschnitt `__CM_EXTENDED=1` und
   `__FEATURES=<Feature-Liste des Servers>`. **Noch nicht vom Nutzer bestätigt.** Wenn es weiter
   scheitert: CSP-Version des Nutzers (`0.3.0-preview649`) gegen die vom Server verlangte Version
   (`2651`) klären, sonst die Skins (`name/ADAn`: der Teil nach dem Schrägstrich wird abgeschnitten)
   und Buchungen (`/SUB`) prüfen.
2. **Quick Race starten** testen (Steam-Umleitung über Content Manager sollte weg sein). Ungetestet.
3. **Statistiken auf dem Home Screen**: siehe Abschnitt 8 und `docs/session-stats-plan.md`.
4. Gemeinsame Speicherleiste für alle Konfigurationswerkzeuge; Vorher/Nachher für Graphics Profiles
   (ist ein Umschalter, braucht ein eigenes Konzept) und Setups (hat schon eine Vergleichstabelle).
5. Weitere Werkzeuge im neuen Stil: Collections, Lap Times, Replay Viewer, Live Telemetry und andere.
6. Hover-Hilfen, leere Zustände mit Aktion, Bildplatzhalter beim Laden.
7. README-Screenshots erneut aufnehmen (nach der neuen Seitenleiste und dem Join-Panel).
8. Aktualisierungs-Feed (`https://test2.azendix.de/acxs/latest.json`) gegen den echten Server prüfen.
9. Änderungen **committen** (nur auf Wunsch des Nutzers).

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

## 10. Nächste Schritte, wenn es weitergeht

(Stand 1. Nov: Phasen 0-6 sind gebaut und committet (lokal, nicht gepusht). Phase 6 = `src/collision_app.rs`, Lua-App "ACXSCollisions", schreibt `DocumentsAssetto Corsaogscxs-collisions.log`; ungetestet im Spiel, ob die App ohne Einschalten in der Spiel-App-Leiste lädt. Als Nächstes: echte Fahrt testen.)

1. Antworten des Nutzers auf die vier Fragen in Abschnitt 8 einholen (oder sinnvolle Vorgaben nehmen:
   nur eigenes Auto, 50 Sessions Detail, Hintergrundbetrieb als Option).
2. **Phase 0 und 1 der Statistiken sind gebaut**: `src/driving_stats.rs` (Import, Home "Your driving"), `src/session_recorder.rs` (Rekorder-Thread 30 Hz, JSON je Session in `%APPDATA%ACXSsessions`, Kennzahlen Crash/Sprung/Blockieren/Wheelspin, Statuspille "Recording session"). **Ungetestet gegen das echte Spiel**: Schwellen (Crash 4,5 g + 25 km/h Verlust, Blockieren unter 70 % Tempo) beim ersten Fahren prüfen. Untersteuern/Übersteuern (selbstkalibrierter Lenk-Gierraten-Gewinn, Schwellen 0,8 / 1,25) und Seite "Sessions" (`tools/sessions.rs`) sind drin. Streckenabschnitte (100 Scheiben, "Trouble spots" auf der Sessions-Seite) und Replay-Archiv (20 neueste, `replays`) sind drin. Empfehlungen (`driving_advice.rs`, auf der Sessions-Seite) sind drin. Streckenkarte mit Problemstellen (Pfad je Session gespeichert) ist drin, aber optisch ungeprüft. `Sessions*.json`-Rundenzeiten sind importiert (Home "Best laps"). Replay-Auswertung (`replay_analysis.rs`, Replay Viewer) ist drin. Knopf "Open Setups" an den Empfehlungen ist drin (kein automatisches Ändern von Setups). Offen: Detailseite, Nachauswertung der Replays, (`replay.ini`-Schalter ist drin: Knopf auf der Sessions-Seite), `Sessions*.json`-Import, `personalbest.ini`.
   `personalbest.ini`, Typen in der Bibliothek mit Tests, Kacheln und Sessionliste auf Home.
3. Ergebnis des Join-Tests (Server LAC) vom Nutzer abfragen und bei Fehlern gemäß Abschnitt 6, Punkt 1
   weiter eingrenzen.

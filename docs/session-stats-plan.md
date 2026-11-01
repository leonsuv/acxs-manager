# Session-Statistiken auf dem Home Screen: Befund und Plan

Stand: 1. November 2026. Dieses Dokument beschreibt, welche Daten es gibt, was sich daraus
berechnen lässt und wie ACXS Manager das aufbauen soll. Es ist ein Plan, noch kein Code.

## 1. Was auf diesem PC schon da ist (gemessen, nicht geraten)

| Quelle | Ort | Inhalt | Eignung |
|---|---|---|---|
| **Content-Manager-Profil** | `%LOCALAPPDATA%\AcTools Content Manager\Progress\Profile (Sessions).data` | 145 Sessions seit 14.04.2026, eine JSON-Zeile je Session: Auto, Strecke, Start, Dauer, **Crashes**, **Offroad**, **Höchstgeschwindigkeit**, **Distanz**, **Luftzeit gesamt und längster Sprung** (alle vier Räder in der Luft), Spritverbrauch, Reifenverschleiß, Strafen, Bestzeit | **sofort importierbar**, damit gibt es ab Tag 1 eine Historie |
| **Content-Manager-Sessions** | `...\Progress\Sessions\*.json` (148 Dateien) | Kopie der `race_out.json` je Session: Runden mit Sektorzeiten, **Cuts**, Reifenmischung, Spieler, Server (aus dem gesicherten `race.ini`) | **importierbar**: Rundenverläufe, Cuts, Server |
| **Replays des Spiels** | `Documents\Assetto Corsa\replay\temp\AC_<Datum>-<Zeit>_<R oder O>_<Auto>_<Strecke>.acreplay` | je Session eine Datei (R offline, O online), 10 bis 28 MB | **nicht konsistent**: siehe unten |
| **Bestzeiten** | `Documents\Assetto Corsa\personalbest.ini` | Bestzeit je Auto und Strecke | importierbar, klein |
| **Spiel-Log** | `Documents\Assetto Corsa\logs\log.txt` | Start und Ende von Sessions, Serverdaten, Runden-Ereignisse; wird bei jedem Start überschrieben | nur als Zusatz |
| **Live-Daten** | Shared Memory des Spiels, UDP-Telemetrie | alles, was zum Fahren gehört, mehrmals pro Sekunde | **die Hauptquelle** |

### Warum Replays allein nicht reichen
`cfg\replay.ini` des Spiels sagt: höchstens **26 MB** je Replay, Qualität 30 Hz, und der Autosave
hebt nur **2 Rennen, 1 Qualifying und 1 sonstige Session** auf. Ältere Dateien verschwinden, lange
Sessions werden gekürzt. Ein Replay enthält je Bild Geschwindigkeit, Drehzahl, Gang, Gas, Bremse und
die Position des Autos (das liest unser Parser schon), aber **keinen Schlupf, keinen Lenkwinkel, keine
Radlasten**. Daraus lassen sich Tempo, Runden, harte Bremsungen, Sprünge (Höhenänderung) und
auffällige Geschwindigkeitseinbrüche (Unfälle) schätzen, Unter- oder Übersteuern aber nicht.

Nützlich bleibt es trotzdem: ACXS kann neue Replays nach jeder Session **kopieren und archivieren**
(bevor das Spiel sie überschreibt) und die vier vorhandenen nachträglich auswerten.

### Wichtig: ACXS startet das Spiel jetzt direkt
Content Manager schreibt seine Session-Historie nur, wenn er das Spiel selbst startet. Sessions, die
über ACXS Manager laufen (Quick Race, Server beitreten), **fehlen** dort. Dafür braucht ACXS einen
eigenen Rekorder.

## 2. Was der Rekorder live messen kann

Das Spiel veröffentlicht drei Seiten im Shared Memory. Unser Code in `sharedmem.rs` liest sie bereits
(Physik, Grafik, Statik). Die Physik-Seite liefert bis zu 333 Mal pro Sekunde:

Geschwindigkeit, Geschwindigkeitsvektor, G-Kräfte (3 Achsen), **Raddrehzahl, Radschlupf, Radlast,
Federweg, Reifendruck, Reifentemperatur (innen, Mitte, außen), Bremstemperatur**, Lenkwinkel, Gas,
Bremse, Kupplung, Gang, Drehzahl, **Gierrate, Nick- und Wankwinkel, Fahrzeugkurs**, ABS- und
TC-Eingriff, **Schadenswerte in 5 Zonen**, Anzahl der Reifen außerhalb der Strecke, Bremsbalance,
Kontaktpunkte der Reifen, Luft- und Straßentemperatur. Die Grafik-Seite liefert Rundenzeit, Sektor,
Runden, Position, **Streckenposition 0 bis 1**, Boxengasse, Strafen, Flaggen und Weltkoordinaten.

### Machbarkeit je Wunschwert

| Wunsch | Machbar? | Wie |
|---|---|---|
| Geschwindigkeit: Höchst, Durchschnitt, Verlauf | **ja, exakt** | `speed_kmh`, Mittel über Fahrzeit und über Strecke getrennt |
| Runden, Rundenzeiten, Sektoren, Bestzeit | **ja** | Grafik-Seite, dazu Cuts |
| Distanz, Fahrzeit, Sprit, Reifenverschleiß | **ja** | Grafik- und Physik-Seite |
| **Untersteuern und Übersteuern** | **ja, sehr gut** | Soll-Gierrate aus Lenkwinkel, Tempo und Radstand (Fahrzeugdaten) gegen gemessene Gierrate; dazu Schlupfwinkel vorn gegen hinten. Ergebnis je Kurve und je Streckenposition |
| **Bremsen blockieren (Brake Locking)** | **ja** | Raddrehzahl mal Reifenradius gegen Fahrzeugtempo: fällt ein Rad deutlich darunter, blockiert es. Mit Unterscheidung vorn und hinten, ABS-Eingriffe zählen extra |
| Durchdrehende Räder (Wheelspin) | ja | gleiche Rechnung in Gegenrichtung beim Beschleunigen |
| **Alle vier Räder in der Luft, Sprünge, "Klippe hinunter"** | **ja** (macht der Content Manager schon) | Radlast aller vier Räder nahe null und Federweg ausgefedert; Dauer = Luftzeit, Höhenunterschied aus der Weltkoordinate, Aufprall aus dem G-Kraft-Spitzenwert |
| **Crashes** | **ja, als Ereignis** | Sprung der Schadenswerte, G-Kraft-Spitze oder Tempoeinbruch |
| Kollision mit Auto oder Wand unterscheiden | **eingeschränkt** | Shared Memory sagt nicht, womit. Online lässt sich mit Nachbarautos raten, sicher geht es nur mit einem kleinen **CSP-Lua-App** (Custom Shaders Patch meldet Kollisionen mit Auto oder Umgebung). Das ist ein späterer, optionaler Schritt |
| Neben der Strecke, Streckenlimits | **ja** | `Reifen außerhalb` und Cuts |
| Temperaturen, Reifenfenster | ja | Reifen- und Bremstemperaturen je Runde |

Ehrliche Grenzen: Der Rekorder läuft nur, **solange ACXS Manager geöffnet ist** (er kann im Hintergrund
laufen). Offline und online geht es gleich, weil das Spiel dieselben Seiten füllt. Bei Servern mit
anderen Spielern messen wir nur das eigene Auto.

## 3. Aufbau

### 3.1 Rekorder (neuer Kern in der Bibliothek, kein UI-Code)
- Ein Hintergrund-Thread. Er prüft alle 2 Sekunden, ob `acs.exe` läuft (gibt es schon), öffnet dann das
  Shared Memory und liest **mit 30 Hz** (reicht für alle Werte, schont CPU und VR-Frames, niedrige
  Thread-Priorität, keine Speicherallokation pro Probe).
- Eine Zustandsmaschine erkennt Sessionbeginn und -ende (Status "live", Session-Typ, Strecke und Auto
  aus der Statik-Seite, Server aus `race.ini`).
- Je Probe werden nur **Aggregatoren** aktualisiert (laufende Summen, Maxima, Zähler). Je Runde und
  je Streckenabschnitt (1000 Abschnitte nach Streckenposition) entsteht eine kleine Zusammenfassung.
- Ereignisse mit Zeitstempel und Ort: Crash, Sprung, Blockieren, Cut, Boxengasse.
- Speicherung unter `%APPDATA%\ACXS\sessions\`: **eine JSON-Datei je Session** (Kopf, Runden,
  Ereignisse, Abschnittsstatistik) plus optional ein **auf 5 Hz heruntergerechneter Verlauf** für
  Diagramme (rund 200 KB je Stunde). Beim Absturz bleibt eine unvollständige Datei lesbar.
- Alles bleibt lokal. Nichts wird hochgeladen.

### 3.2 Importer
- `Profile (Sessions).data` und `Sessions\*.json` des Content Managers lesen (nur lesen, nie
  verändern) und in dasselbe Sessionformat bringen, mit Quelle "Content Manager". Doppelte werden über
  Startzeit und Auto erkannt.
- `personalbest.ini` für Bestzeiten.
- Replay-Archiv: neue `.acreplay`-Dateien nach jeder Session in einen ACXS-Ordner kopieren; die
  vorhandenen vier nachträglich mit dem bestehenden Parser auswerten (Tempo, Runden, Sprünge,
  harte Bremsungen, Einbrüche).
- Hilfsschalter in den Einstellungen: "Replays aufheben" setzt in `replay.ini` höhere Grenzen
  (`RACE=-1`, größere `MAX_SIZE_MB`), mit Sicherung der alten Datei.

### 3.3 Empfehlungen
Regelbasiert, nur mit Mindestmenge an Daten (zum Beispiel 5 gefahrene Runden mit dem Auto), immer mit
Begründung und Zahl:

- **Untersteuern** in langsamen Kurven: Vorderachse entlasten (Stabilisator vorn weicher, Federn vorn
  weicher, mehr Vorderspur, weniger Sturzlast vorn), Sperre weniger Vorspannung.
- **Übersteuern beim Gasgeben**: Sperre mehr Vorspannung, hinten weicherer Stabilisator, mehr
  Heckflügel, TC höher.
- **Übersteuern beim Anbremsen**: Bremsbalance nach vorn, weniger Motorbremse.
- **Vorderräder blockieren**: Bremsbalance nach hinten, Bremsdruck senken, ABS hoch.
- **Hinterräder blockieren**: Bremsbalance nach vorn.
- **Durchdrehende Räder am Kurvenausgang**: TC höher, später auf das Gas, Sperre prüfen.
- **Reifen zu heiß oder zu kalt** (Innen, Mitte, Außen): Reifendruck und Sturz.
- **Viele Cuts oder Abflüge an derselben Stelle**: Kurve markieren ("hier passiert es dir 6 Mal").
- Verknüpfung mit der vorhandenen Setup-Beratung (`setup_advice.rs`) und dem Setup-Editor, damit ein
  Vorschlag direkt als Änderung angezeigt werden kann.

### 3.4 Oberfläche
- **Home**: Block "Dein Fahren" mit Kacheln (Fahrzeit, Distanz, Sessions, Runden, Crashes je 100 km,
  Höchstgeschwindigkeit, längster Sprung), Balken je Woche, die meistgefahrenen Autos und Strecken und
  eine Liste der letzten Sessions als Karten mit Strecken- und Autobild.
- **Session-Seite**: Rundentabelle, Tempo- und Pedalverlauf, Streckenkarte mit farbigen Problemstellen
  (Untersteuern, Übersteuern, Blockieren, Cuts, Crashes, Sprünge), Ereignisliste mit Zeitpunkt,
  Empfehlungen.
- **Statistik-Seite** (Sidebar, "Driving data"): Filter nach Auto, Strecke, Zeitraum, Online oder
  Offline; Vergleich zweier Sessions; Entwicklung der Bestzeit.
- Aufnahmestatus als kleine Anzeige in der Kopfzeile ("Aufnahme läuft", mit Pause-Knopf).

## 4. Phasen

| Phase | Inhalt | Ergebnis |
|---|---|---|
| **0** | Importer für Content-Manager-Daten und Bestzeiten, Home-Kacheln | Statistik ab sofort, mit Historie seit April |
| **1** | Rekorder-Kern, Sessionformat, Speicherung, Statusanzeige | alle neuen Sessions werden aufgezeichnet |
| **2** | Kennzahlen: Tempo, Runden, Sprünge, Crashes, Blockieren, Untersteuern und Übersteuern | die Werte, die du genannt hast |
| **3** | Session-Seite mit Streckenkarte und Diagrammen | Auswertung im Detail |
| **4** | Replay-Archiv, Nachauswertung der vorhandenen Replays, `replay.ini`-Schalter | Historie ohne Lücken |
| **5** | Empfehlungen mit Setup-Verknüpfung | konkrete Änderungen |
| **6** (optional) | CSP-Lua-App für Kollisionsart und genaue Ereignisse | Auto-gegen-Auto und Wand unterscheiden |

Jede Phase ist für sich nutzbar. Phase 0 und 1 liefern schon sichtbaren Nutzen.

## 5. Risiken und Gegenmaßnahmen

- **VR und Leistung**: 30 Hz, wenig Arbeit je Probe, niedrige Priorität, Auswertung der Abschnitte
  erst nach der Runde. Messen mit dem Spiel in VR.
- **Falsche Alarme bei Crashes und Blockieren**: Schwellen je Auto aus dessen Daten (Radstand, Reifenradius,
  Höchstmoment), Tests mit aufgezeichneten Proben.
- **App geschlossen während der Session**: Lücke. Gegenmittel: Replay-Archiv und Content-Manager-Import
  schließen sie teilweise; später ein Minimal-Hintergrunddienst, falls gewünscht.
- **Datenschutz**: nur lokale Dateien, Löschknopf je Session und für alles, Export als CSV oder JSON.
- **Formatänderungen** des Spiels oder von CSP: Parser sind getestet und lehnen unbekannte Formate ab,
  statt zu raten.

## 6. Offene Fragen an dich

1. Soll die App im **Hintergrund beim Windows-Start** laufen (kleines Symbol), damit nie eine Session
   fehlt, oder reicht "geöffnet während du fährst"?
2. Wie lange sollen Detaildaten (5-Hz-Verlauf) aufgehoben werden: immer, 90 Tage, oder nur die letzten
   50 Sessions? Die Kurzfassung je Session bleibt immer.
3. Soll der Rekorder auch **fremde Autos** auf Servern erfassen (zum Beispiel für Vergleiche)? Der Plan
   geht von "nur dein Auto" aus.
4. Reicht für Kollisionen "Crash erkannt", oder willst du schon früh die Unterscheidung Auto gegen Wand
   (dann kommt Phase 6 vor 5)?

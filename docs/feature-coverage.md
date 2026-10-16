# Feature coverage overview

This is a short tour of what ACXS Manager does, grouped by function. It is meant
for someone new to the project. For the status of each feature and what is still
missing, see ROADMAP.md.

ACXS Manager is a single Windows desktop program for managing the racing
simulator's files. It gathers several jobs that would otherwise need separate
tools into one window. Most of the work is reading and writing the game's own
files, so the program keeps those edits reversible where it can.

## Installing and launching

The program finds the game through the game platform's library folders, or through
a folder the user picks by hand. It reports whether the game is running, starts the game
or a launcher profile, and installs cars and tracks from zip archives.

## Content library

This is the largest area. It scans the installed cars and tracks, lets the user
filter and search them, and shows specs and thumbnails. Two cars or two tracks can
be compared side by side. Car and track metadata can be edited, and content
health checks report problems and offer repairs for some of them. Content can be
switched off without being deleted. Users can keep named collections and export
lists to CSV or JSON.

## Appearance and tuning

Skins can be scanned, imported and recoloured, and licence plates can be made. A
car's engine, mass, fuel, gears and final drive values can be edited. Setups get
their own area: setup files can be listed, imported, archived, compared and edited,
and two setups can be merged with a three-way merge. The program also gives
advice on setup changes.

## Models

The model tools read the game's KN5 model format and export models to OBJ and
FBX. They can write a KN5 model from an OBJ file, render preview images, and
calculate ambient-occlusion values. They can also list the entries of the
packed data archive that cars use.

## Graphics and game configuration

The program can install, switch and remove the lighting patch, and it can roll
back a change. It can also switch the active graphics injector between a few
choices, including none, and manage the folders those injectors come from. Users
can edit the patch's configuration, force feedback, weather presets, car classes
and car years, and read gamepad settings. The program manages user Lua apps and
edits the game's video and main configuration files. Configuration snapshots can
be taken and restored.

## Racing

Quick race writes a race configuration and starts the game with a named preset.
Lap records are kept with a delta trace, and replays can be viewed. Race engineer,
setup, strategy and timing views sit alongside these. Championships are scored
from CSV results files.

## Dedicated server

The program edits a dedicated server's configuration and entry list, starts and
stops the server process, and shows the live server status.

## Telemetry, logs and diagnostics

Live telemetry comes in over UDP and is shown with charts and a track map. On
Windows, the program can also read the game's shared memory. Telemetry and lap
data can be exported to CSV. A local web dashboard can serve the same numbers to
a browser, and a HUD layout can be edited. Game logs are summarized, and a
diagnostics bundle can be written for bug reports.

## Out of scope

Some things are deliberately not part of the project. It does not implement the
graphics renderers themselves, and it does not bypass licensing or content
protection. It does not provide a virtual joystick driver, because that needs a
signed Windows driver. The target platform is Windows.

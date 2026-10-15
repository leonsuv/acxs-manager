#!/usr/bin/env bash
# Build the Windows release and package it as dist/ACXS-Manager-<version>-windows-x64.zip.
#
# The zip holds ACXS-Manager.exe, README.md and CHANGELOG.md at its root.
# The SHA-256 of the zip is printed and written to dist/SHA256SUMS.txt.
# CHANGELOG.md must already exist; this script never creates or writes it.
#
# Usage: scripts/package-release.sh [--allow-dirty] [--dry-run]
#   --allow-dirty  build even if the git working tree has uncommitted changes
#   --dry-run      print the planned zip name and exit without building
set -euo pipefail

ALLOW_DIRTY=0
DRY_RUN=0

for arg in "$@"; do
    case "$arg" in
        --allow-dirty) ALLOW_DIRTY=1 ;;
        --dry-run) DRY_RUN=1 ;;
        -h|--help)
            echo "Usage: $0 [--allow-dirty] [--dry-run]"
            exit 0
            ;;
        *)
            echo "error: unknown argument: $arg" >&2
            echo "Usage: $0 [--allow-dirty] [--dry-run]" >&2
            exit 2
            ;;
    esac
done

cd "$(dirname "$0")/.."

# Version is the version = "..." line inside the [package] table of Cargo.toml.
VERSION=$(sed -n '/^\[package\]/,/^\[/{s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p;}' Cargo.toml | head -n 1)
version_pattern='^[0-9A-Za-z.+-]+$'
if [[ -z "$VERSION" || ! "$VERSION" =~ $version_pattern ]]; then
    echo "error: could not read a valid package version from Cargo.toml (got '$VERSION')" >&2
    exit 1
fi

ZIP_NAME="ACXS-Manager-${VERSION}-windows-x64.zip"
ZIP_PATH="dist/${ZIP_NAME}"

if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "Dry run: version ${VERSION}"
    echo "Planned zip: ${ZIP_PATH}"
    exit 0
fi

if [[ "$ALLOW_DIRTY" -eq 0 ]]; then
    dirty=$(git status --porcelain)
    if [[ -n "$dirty" ]]; then
        echo "error: working tree has uncommitted changes:" >&2
        echo "$dirty" >&2
        echo "Commit them, or pass --allow-dirty to package anyway." >&2
        exit 1
    fi
fi

for required in README.md CHANGELOG.md; do
    if [[ ! -f "$required" ]]; then
        echo "error: ${required} is missing; create it before packaging (this script does not write it)" >&2
        exit 1
    fi
done

echo "Building Windows release ${VERSION}..."
./scripts/build-windows.sh

if [[ ! -f dist/ACXS-Manager.exe ]]; then
    echo "error: dist/ACXS-Manager.exe was not produced by the build" >&2
    exit 1
fi

# Remove only this version's zip so stale entries cannot linger in it.
rm -f "$ZIP_PATH"
zip -j -X "$ZIP_PATH" dist/ACXS-Manager.exe README.md CHANGELOG.md

# Hash is written in sha256sum format, relative to dist/, so it can be checked with
# shasum -a 256 -c SHA256SUMS.txt from inside dist/.
(
    cd dist
    shasum -a 256 "$ZIP_NAME" > SHA256SUMS.txt
    cat SHA256SUMS.txt
)

echo "Packaged: $(pwd)/${ZIP_PATH}"

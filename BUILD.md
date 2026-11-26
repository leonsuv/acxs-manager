# Building ACXS Manager

ACXS Manager uses **Rust (Edition 2024, Rust 1.88+)** for the core library (`acxs_manager`) and **Tauri v2 + React 19 + TypeScript + Tailwind CSS v4 + Three.js** for the desktop application (`src-tauri/` + `ui/`).

## Build the Tauri v2 Desktop App on Windows (Recommended)

### Prerequisites
- **Rust (MSVC toolchain)** via `rustup` + Visual Studio Build Tools ("Desktop development with C++")
- **Node.js 20+** (`npm`)
- **WebView2 Runtime** (pre-installed on Windows 10/11)

### Release Build (`.exe`)
```bat
cd ui
npm install
npm run build
cd ..\src-tauri
cargo build --release
```
Output: `src-tauri\target\release\acxs-tauri.exe`.

### Development Mode (Live Reload)
```bat
npm --prefix ui run dev -- --port 1420 --strictPort
```
In a second terminal:
```bat
cargo run --manifest-path src-tauri\Cargo.toml
```

### Tests
```bat
cargo test --lib
```

---

## Legacy egui Binary — Build on macOS / Windows

One-time setup:

```sh
brew install llvm lld
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
```

Homebrew LLVM provides `clang-cl` and `llvm-lib`, and Homebrew `lld` provides
`lld-link`. Rust's bundled `rust-lld` does not work on this Mac, so these must
come first on `PATH`.

Build:

```sh
./scripts/build-windows.sh
```

The script puts the Homebrew LLVM and lld `bin` directories first on `PATH`, runs
`cargo xwin build --release --target x86_64-pc-windows-msvc`, and copies the
result to `dist/ACXS-Manager.exe`. It assumes the default Homebrew install
location used on Apple Silicon Macs.

The equivalent manual commands:

```sh
export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld)/bin:$PATH"
cargo xwin build --release --target x86_64-pc-windows-msvc
```

The first build downloads the Windows SDK and C runtime, so it needs network
access and takes longer than later builds.

Output: `target/x86_64-pc-windows-msvc/release/acxs-manager.exe`. The script also
copies it to `dist/ACXS-Manager.exe`; the manual commands stop at the `target/` path.

## Build on Windows

Install Rust with the MSVC toolchain (Visual Studio Build Tools with the
"Desktop development with C++" workload), then from the project directory:

```bat
cargo build --release
```

Output: `target\release\acxs-manager.exe`.

Release builds use the Windows GUI subsystem, so no console window opens.
Debug builds keep the console for log output.

## Package a release

`scripts/package-release.sh` builds the Windows release and packages it as
`dist/ACXS-Manager-<version>-windows-x64.zip`. The version is read from the
`[package]` table in `Cargo.toml`.

Check the planned output first. This prints the version and the zip name, then
exits without building or checking the git state:

```sh
./scripts/package-release.sh --dry-run
```

Then create the package:

```sh
./scripts/package-release.sh
```

The real run:

- Stops if the git working tree is not clean. Uncommitted changes and untracked
  files both count. Commit everything first. `--allow-dirty` skips this check,
  but then the package does not match a commit.
- Stops if `README.md` or `CHANGELOG.md` is missing from the repository root. The
  script does not create or edit either file.
- Runs `scripts/build-windows.sh`, which must produce `dist/ACXS-Manager.exe`.
- Zips `ACXS-Manager.exe`, `README.md` and `CHANGELOG.md` at the zip root. It
  replaces the zip for the same version if one already exists.
- Writes the SHA-256 of the zip to `dist/SHA256SUMS.txt`. This overwrites the file,
  so the file lists only the new zip. To check it, run `shasum -a 256 -c SHA256SUMS.txt`
  from inside `dist/`.

Usage: `scripts/package-release.sh [--allow-dirty] [--dry-run]`.

## Tests

Run the library tests on macOS or Windows:

```sh
cargo test --lib
```

These are the unit tests of the core library. The user interface is in the binary
and has no automated tests.

## Continuous integration

This repository has no CI workflow, by choice. Builds and tests are run locally:
the Windows build from macOS, or the native build on Windows. Because nothing runs
the tests automatically, run `cargo test --lib` before packaging a release.

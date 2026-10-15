# Building ACXS Manager

ACXS Manager is a Rust project (edition 2024, egui/eframe 0.37). Edition 2024 needs
Rust 1.85 or newer, so use a recent stable toolchain. The release binary is a single
`.exe` for Windows.

## Build on macOS (cross-compile for Windows)

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

The script sets `PATH` to the Homebrew LLVM and lld directories, runs
`cargo xwin build --release --target x86_64-pc-windows-msvc`, and copies the
result to `dist/ACXS-Manager.exe`.

The equivalent manual commands:

```sh
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH"
cargo xwin build --release --target x86_64-pc-windows-msvc
```

The first build downloads the Windows SDK and CRT from Microsoft, so it needs
network access and takes longer than later builds.

Output: `target/x86_64-pc-windows-msvc/release/acxs-manager.exe`, copied to
`dist/ACXS-Manager.exe` by the script. The manual commands stop at the `target/` path.

## Build on Windows

Install Rust with the MSVC toolchain (Visual Studio Build Tools with the
"Desktop development with C++" workload), then from the project directory:

```bat
cargo build --release
```

Output: `target\release\acxs-manager.exe`.

Release builds use the Windows GUI subsystem, so no console window opens.
Debug builds keep the console for log output.

## Tests

Run the library tests on macOS or Windows:

```sh
cargo test --lib
```

The Windows CI workflow runs the same command. The user interface is in the binary
and has no automated tests.

## Continuous integration

`.github/workflows/windows-build.yml` runs on every push, on every pull request and
on manual dispatch. It runs on `windows-latest` and does four things:

1. Installs stable Rust with the `x86_64-pc-windows-msvc` target.
2. Caches the Cargo registry, Cargo git data and `target/`.
3. Runs `cargo test --lib`, then `cargo build --release`.
4. Uploads `target/release/acxs-manager.exe` as the artifact `ACXS-Manager`.

The CI build uses the native Windows toolchain, so it does not use the macOS
cross-compile script or `dist/`.

#!/usr/bin/env bash
# Cross-compile ACXS Manager for Windows (MSVC target) from macOS.
#
# Requires: brew install llvm lld; cargo install cargo-xwin;
#           rustup target add x86_64-pc-windows-msvc
# Homebrew LLVM (clang-cl, llvm-lib) and lld (lld-link) must come before
# Rust's bundled rust-lld, which does not work on this Mac.
# cargo-xwin downloads the Windows SDK/CRT from Microsoft on first use.
set -euo pipefail

cd "$(dirname "$0")/.."

export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH"

TARGET="x86_64-pc-windows-msvc"
cargo xwin build --release --target "$TARGET"

mkdir -p dist
cp "target/$TARGET/release/acxs-manager.exe" dist/ACXS-Manager.exe
echo "Built: $(pwd)/dist/ACXS-Manager.exe"

#!/usr/bin/env bash
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

echo "Building tern-projects indexer..."
cargo build --locked --release --manifest-path indexer/Cargo.toml

mkdir -p bin
cp indexer/target/release/tern-projects bin/tern-projects

echo "Binary installed to $REPO/bin/tern-projects"

# Also install to ~/.cargo/bin as a fallback when the plugin is loaded from a
# different path (e.g. after tern plugin link resolves symlinks).
mkdir -p "${CARGO_HOME:-$HOME/.cargo}/bin"
cp bin/tern-projects "${CARGO_HOME:-$HOME/.cargo}/bin/tern-projects"
echo "Done."

#!/usr/bin/env bash
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

echo "Building tern-projects indexer..."
cargo build --locked --release --manifest-path indexer/Cargo.toml

mkdir -p bin
cp indexer/target/release/tern-projects bin/tern-projects

# Nix toolchains can link libiconv from /nix/store. Distribute against macOS's
# ABI-compatible system library instead, and refresh the ad-hoc signature.
if [[ "$(uname -s)" == Darwin ]]; then
    while read -r dep _; do
        case "$dep" in
            /nix/store/*/lib/libiconv.2.dylib)
                install_name_tool -change "$dep" /usr/lib/libiconv.2.dylib bin/tern-projects
                ;;
        esac
    done < <(otool -L bin/tern-projects)
    while read -r dep _; do
        case "$dep" in
            /*)
                case "$dep" in
                    /usr/lib/*|/System/Library/*) ;;
                    *) echo "Non-system runtime dependency: $dep" >&2; exit 1 ;;
                esac
                ;;
        esac
    done < <(otool -L bin/tern-projects)
    codesign --force --sign - bin/tern-projects
fi
echo "Binary installed to $REPO/bin/tern-projects"

# Also install to ~/.cargo/bin as a fallback when the plugin is loaded from a
# different path (e.g. after tern plugin link resolves symlinks).
mkdir -p "${CARGO_HOME:-$HOME/.cargo}/bin"
cp bin/tern-projects "${CARGO_HOME:-$HOME/.cargo}/bin/tern-projects"
echo "Done."

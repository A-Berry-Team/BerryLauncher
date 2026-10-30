#!/bin/sh
# Regenerate build-aux/flatpak/cargo-sources.json from Cargo.lock so Flatpak builds can run
# without network access (needed for reproducible / Flathub builds).
#
# Needs network once, plus: python3, `pip install aiohttp tomlkit PyYAML`.
# Re-run whenever Cargo.lock changes, and commit the result.
set -eu
cd "$(dirname "$0")/.."
GENERATOR="${FLATPAK_CARGO_GENERATOR:-build-aux/flatpak/flatpak-cargo-generator.py}"
if [ ! -f "$GENERATOR" ]; then
  curl -fsSL -o "$GENERATOR" \
    https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py
fi
python3 "$GENERATOR" Cargo.lock -o build-aux/flatpak/cargo-sources.json
echo "wrote build-aux/flatpak/cargo-sources.json"

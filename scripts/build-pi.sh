#!/usr/bin/env bash
# Builds the Pi binary inside an arm64 container (native speed on Apple Silicon).
set -euo pipefail
cd "$(dirname "$0")/.."
docker build --platform linux/arm64 -t ziggy-build -f deploy/Dockerfile.build deploy
docker run --rm --platform linux/arm64 \
    -v "$PWD":/src \
    -v ziggy-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/src/target/pi \
    ziggy-build cargo build --release -p ziggy-player
docker run --rm --platform linux/arm64 -v "$PWD":/src ziggy-build \
    sh -c 'file target/pi/release/ziggy-player 2>/dev/null || true; target/pi/release/ziggy-player --help'
echo "Built target/pi/release/ziggy-player"

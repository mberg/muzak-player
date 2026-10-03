#!/usr/bin/env bash
# Builds the Pi binary inside an arm64 container (native speed on Apple Silicon).
set -euo pipefail
cd "$(dirname "$0")/.."
docker build --platform linux/arm64 -t muzak-build -f deploy/Dockerfile.build deploy
docker run --rm --platform linux/arm64 \
    -v "$PWD":/src \
    -v muzak-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/src/target/pi \
    muzak-build cargo build --release -p muzak-player
docker run --rm --platform linux/arm64 -v "$PWD":/src muzak-build \
    sh -c 'file target/pi/release/muzak-player 2>/dev/null || true; target/pi/release/muzak-player --help'
echo "Built target/pi/release/muzak-player"

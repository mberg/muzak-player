#!/usr/bin/env bash
# Usage: scripts/deploy.sh <ssh-host> <config.toml> [secrets-dir]
set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${1:?usage: scripts/deploy.sh <ssh-host> <config.toml> [secrets-dir]}
CONFIG=${2:?missing config.toml}
SECRETS=${3:-}
BIN=target/pi/release/muzak-player
[ -f "$BIN" ] || { echo "No Pi binary; run scripts/build-pi.sh first" >&2; exit 1; }

scp "$BIN" "$HOST":/tmp/muzak-player
scp "$CONFIG" "$HOST":/tmp/muzak-config.toml
ssh "$HOST" 'sudo install -m 755 /tmp/muzak-player /usr/local/bin/muzak-player \
    && sudo install -m 644 /tmp/muzak-config.toml /etc/muzak/config.toml \
    && rm /tmp/muzak-player /tmp/muzak-config.toml'
if [ -n "$SECRETS" ]; then
    scp "$SECRETS/librespot/credentials.json" "$HOST":/tmp/muzak-credentials.json
    ssh "$HOST" 'sudo install -m 600 -o muzak -g muzak /tmp/muzak-credentials.json /var/lib/muzak/librespot/credentials.json \
        && rm /tmp/muzak-credentials.json'
fi
ssh "$HOST" 'sudo systemctl restart muzak-player && sleep 3 && systemctl --no-pager --lines=20 status muzak-player'

#!/usr/bin/env bash
# Usage: scripts/deploy.sh <ssh-host> <config.toml> [secrets-dir]
# Assumes passwordless sudo on the Pi (the Pi OS default user).
set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${1:?usage: scripts/deploy.sh <ssh-host> <config.toml> [secrets-dir]}
CONFIG=${2:?missing config.toml}
SECRETS=${3:-}
BIN=target/pi/release/ziggy-player
[ -f "$BIN" ] || { echo "No Pi binary; run scripts/build-pi.sh first" >&2; exit 1; }

scp "$BIN" "$HOST":/tmp/ziggy-player
scp "$CONFIG" "$HOST":/tmp/ziggy-config.toml
ssh "$HOST" 'sudo install -m 755 /tmp/ziggy-player /usr/local/bin/ziggy-player \
    && sudo install -m 644 /tmp/ziggy-config.toml /etc/ziggy/config.toml \
    && rm /tmp/ziggy-player /tmp/ziggy-config.toml'
if [ -n "$SECRETS" ]; then
    CRED="$SECRETS/librespot/credentials.json"
    [ -f "$CRED" ] || { echo "Missing $CRED" >&2; exit 1; }
    ssh "$HOST" 'sudo install -m 600 -o ziggy-player -g ziggy-player /dev/stdin /var/lib/ziggy/librespot/credentials.json' < "$CRED"
    WEB="$SECRETS/web-auth.json"
    if [ -f "$WEB" ]; then
        ssh "$HOST" 'sudo install -m 600 -o ziggy-player -g ziggy-player /dev/stdin /var/lib/ziggy/web-auth.json' < "$WEB"
    fi
fi
ssh "$HOST" 'sudo systemctl restart ziggy-player && sleep 3 && systemctl --no-pager --lines=20 status ziggy-player'

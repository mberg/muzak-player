#!/usr/bin/env bash
# Usage: scripts/provision.sh <ssh-host>
# Assumes passwordless sudo on the Pi (the Pi OS default user).
set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${1:?usage: scripts/provision.sh <ssh-host>}
D=$(ssh "$HOST" mktemp -d)
scp deploy/provision-remote.sh deploy/muzak-player.service "$HOST":"$D"/
ssh "$HOST" "sudo bash '$D/provision-remote.sh'; rc=\$?; rm -rf '$D'; exit \$rc"

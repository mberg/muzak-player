#!/usr/bin/env bash
# Usage: scripts/provision.sh <ssh-host>
set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${1:?usage: scripts/provision.sh <ssh-host>}
ssh "$HOST" 'rm -rf /tmp/muzak-provision && mkdir -p /tmp/muzak-provision'
scp deploy/provision-remote.sh deploy/muzak-player.service "$HOST":/tmp/muzak-provision/
ssh "$HOST" 'sudo bash /tmp/muzak-provision/provision-remote.sh'

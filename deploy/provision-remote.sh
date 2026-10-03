#!/usr/bin/env bash
# Runs on the Pi as root. Safe to run more than once.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

apt-get update
apt-get install -y --no-install-recommends \
    libasound2t64 libinput10 libudev1 libxkbcommon0 libgbm1 libegl1 libgles2 libdrm2 libfontconfig1 \
    libegl-mesa0 libgl1-mesa-dri \
    bluez bluez-alsa-utils libasound2-plugin-bluez

id muzak >/dev/null 2>&1 || useradd --system --create-home --shell /usr/sbin/nologin --groups video,input,audio,render,bluetooth muzak
install -d -m 700 -o muzak -g muzak /var/lib/muzak /var/lib/muzak/librespot
install -d /etc/muzak

CONFIG=/boot/firmware/config.txt
grep -q '^dtoverlay=vc4-kms-dsi-7inch' "$CONFIG" || echo 'dtoverlay=vc4-kms-dsi-7inch' >> "$CONFIG"
grep -q '^dtparam=audio=on' "$CONFIG" || echo 'dtparam=audio=on' >> "$CONFIG"
CMDLINE=/boot/firmware/cmdline.txt
grep -q 'vt.global_cursor_default=0' "$CMDLINE" || sed -i '1 s/$/ vt.global_cursor_default=0 consoleblank=0/' "$CMDLINE"

cat > /etc/udev/rules.d/90-muzak-backlight.rules <<'EOR'
SUBSYSTEM=="backlight", ACTION=="add", RUN+="/bin/chgrp video /sys%p/brightness", RUN+="/bin/chmod g+w /sys%p/brightness"
EOR

mkdir -p /etc/systemd/journald.conf.d
printf '[Journal]\nSystemMaxUse=50M\n' > /etc/systemd/journald.conf.d/muzak.conf

install -m 644 "$HERE/muzak-player.service" /etc/systemd/system/muzak-player.service
systemctl daemon-reload
systemctl enable bluealsa.service muzak-player.service
systemctl disable getty@tty1.service || true

echo "Provisioned. Reboot to apply display settings: sudo reboot"

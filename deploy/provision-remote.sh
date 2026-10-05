#!/usr/bin/env bash
# Runs on the Pi as root. Safe to run more than once.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

apt-get update
apt-get install -y --no-install-recommends \
    libasound2t64 libinput10 libudev1 libxkbcommon0 libgbm1 libegl1 libgles2 libdrm2 libfontconfig1 \
    libegl-mesa0 libgl1-mesa-dri \
    bluez bluez-alsa-utils libasound2-plugin-bluez \
    curl bzip2 ca-certificates

id muzak >/dev/null 2>&1 || useradd --system --create-home --shell /usr/sbin/nologin --groups video,input,audio,render,bluetooth muzak
install -d -m 700 -o muzak -g muzak /var/lib/muzak /var/lib/muzak/librespot /var/lib/muzak/models
install -d /etc/muzak

# Display and boot settings; they need a reboot, so say when anything changed.
CONFIG=/boot/firmware/config.txt
CMDLINE=/boot/firmware/cmdline.txt
BEFORE=$(cat "$CONFIG" "$CMDLINE" | md5sum)
grep -q '^dtoverlay=vc4-kms-dsi-7inch' "$CONFIG" || echo 'dtoverlay=vc4-kms-dsi-7inch' >> "$CONFIG"
grep -q '^dtparam=audio=on' "$CONFIG" || echo 'dtparam=audio=on' >> "$CONFIG"
# Boot faster: no rainbow splash, no boot delay.
grep -q '^disable_splash=1' "$CONFIG" || echo 'disable_splash=1' >> "$CONFIG"
grep -q '^boot_delay=0' "$CONFIG" || echo 'boot_delay=0' >> "$CONFIG"
grep -q 'vt.global_cursor_default=0' "$CMDLINE" || sed -i '1 s/$/ vt.global_cursor_default=0 consoleblank=0/' "$CMDLINE"
grep -q 'loglevel=3' "$CMDLINE" || sed -i '1 s/$/ quiet loglevel=3/' "$CMDLINE"
[ "$(cat "$CONFIG" "$CMDLINE" | md5sum)" = "$BEFORE" ] || echo MUZAK_REBOOT_NEEDED

cat > /etc/udev/rules.d/90-muzak-backlight.rules <<'EOR'
SUBSYSTEM=="backlight", ACTION=="add", RUN+="/bin/chgrp video /sys%p/brightness", RUN+="/bin/chmod g+w /sys%p/brightness"
EOR

mkdir -p /etc/systemd/journald.conf.d
printf '[Journal]\nSystemMaxUse=50M\n' > /etc/systemd/journald.conf.d/muzak.conf

install -m 644 "$HERE/muzak-player.service" /etc/systemd/system/muzak-player.service
systemctl daemon-reload
# Voice: make a USB microphone the default recording device. The Pi's own default device
# (the headphone jack) can't record. Playback stays on the headphone jack by default; the
# player names its Bluetooth or headphone output itself. An asound.conf someone else wrote
# is left alone.
MIC=$(arecord -l 2>/dev/null | sed -n 's/^card [0-9]*: \([^ ]*\) \[.*USB.*/\1/p' | head -1)
if [ -n "$MIC" ] && { [ ! -e /etc/asound.conf ] || grep -q '^# Ziggy' /etc/asound.conf; }; then
    cat > /etc/asound.conf <<EOA
# Ziggy: play through the headphone jack by default, record from the USB microphone.
pcm.!default {
    type asym
    playback.pcm "plughw:CARD=Headphones"
    capture.pcm "plughw:CARD=$MIC"
}
ctl.!default {
    type hw
    card Headphones
}
EOA
fi
# Many USB microphones start with their gain at zero, so the wake word needs shouting. 75%
# (12 of 16 on a common one) hears ordinary speech across a room. Saved for the next boot.
if [ -n "$MIC" ]; then
    for control in Mic Capture; do
        amixer -q -c "$MIC" sset "$control" 75% cap 2>/dev/null && break
    done
    alsactl store 2>/dev/null || true
fi

# Raspberry Pi OS Lite starts with the Bluetooth radio blocked; the player needs it on.
rfkill unblock bluetooth 2>/dev/null || true
systemctl enable bluealsa.service muzak-player.service
systemctl disable getty@tty1.service || true
# Nothing waits for the network at boot, and package-list and manual-page jobs don't run
# while the player starts. (Updates come through `muzak update`.)
for unit in NetworkManager-wait-online.service systemd-networkd-wait-online.service \
    apt-daily.timer apt-daily-upgrade.timer man-db.timer; do
    systemctl disable "$unit" 2>/dev/null || true
done

echo "Provisioned. Reboot to apply display settings: sudo reboot"

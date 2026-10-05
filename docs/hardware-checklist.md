# Ziggy hardware checklist

This checklist is the per-release record of bringing up and testing each Ziggy device. Fill in pass/fail and notes for each section as you work through setup and verification on real hardware.

## 1. Mac checks (before touching a Pi)

These checks verify the development environment and Spotify integration work correctly on the Mac before deployment to hardware.

### Fake-mode UI check

Set these lines temporarily in `dev/config.toml` for testing, then remove them afterwards:
```toml
dim_after_secs = 10
off_after_secs = 20
```

Run: `cargo run -p ziggy-player -- --fake`

Visual and interaction checks:
- [ ] "Loading…" appears briefly, then six playlist tiles. Long names are cut off with "…".
- [ ] Rail buttons (Playlists, Albums, Liked, Recent) switch the content. Liked opens a track list directly.
- [ ] Tapping a tile opens its track list. "Play" button opens Now Playing. "Shuffle" starts on the last song.
- [ ] Progress bar moves every second. Dragging it seeks. Next and previous buttons work. Volume slider works.
- [ ] Back button returns to track list. Mini player shows at the bottom. Tapping mini player reopens Now Playing.
- [ ] Shuffle and repeat buttons highlight when active. Repeat shows a "1" badge on the third tap.
- [ ] After 10s without touch (with playback paused), clock overlay appears. After 20s, screen goes black. A tap wakes it.
- [ ] StateMessage, Banner, and clock text are centered.
- [ ] Detail screen track list fills its area.
- [ ] 4-column grid and Now Playing screen fit 800×480 without clipping. (Screenshot: `screencapture -l$(osascript -e 'tell app "System Events" to id of window 1 of process "ziggy-player"') /tmp/ziggy.png`)

**Result: PASS / FAIL**  
**Notes:**

### Spotify sign-in

Sign in with the kid's Spotify account on the Mac. This creates credentials for the Web API.

Run:
```bash
cargo run -p ziggy-setup -- auth --state-dir secrets/dev
cargo run -p ziggy-setup -- probe --state-dir secrets/dev
```

Expected:
- [ ] Browser opens for Spotify sign-in.
- [ ] After sign-in, credentials file created at `secrets/dev/librespot/credentials.json`.
- [ ] Probe command prints HTTP 200 lines for each endpoint group.
- [ ] Final output: "All endpoints work with librespot tokens."

If probe fails with 401/403, apply **Contingency A** in `docs/superpowers/plans/2026-10-03-ziggy-player-phase1.md` before proceeding.

**Result: PASS / FAIL**  
**Notes:**

### Real playback on the Mac

Verify the player works with real Spotify data and audio on the Mac.

Setup:
```bash
mkdir -p dev/state/librespot
cp secrets/dev/librespot/credentials.json dev/state/librespot/
cargo run -p ziggy-player
```

Playback and data checks:
- [ ] Kid's real playlists and albums appear with cover art. Recent and Liked sections populate.
- [ ] Playing a playlist produces audio through Mac speakers within ~1s. Title and artist match the track.
- [ ] Pause, next, previous, seek, volume, shuffle, and repeat all respond within 0.5s.
- [ ] Spotify app on a phone shows "Ziggy Dev" device as the playing device.
- [ ] Liked Songs plays: open Liked in the rail, press Play, audio starts.
- [ ] Transfer playback to your phone's Spotify app, then transfer back or press Play on Ziggy Dev: playback resumes on the same song.
- [ ] Wi-Fi off: cached grids remain visible; Play shows "No internet right now". Turning Wi-Fi back on: library refreshes and playback resumes within ~1 minute.
- [ ] Moving `credentials.json` away and restarting shows "Ask a grown-up for help" screen.

**Result: PASS / FAIL**  
**Notes:**

## 2. Pi bring-up (per device)

These steps prepare the Raspberry Pi hardware and deploy the player. Complete the following for each device.

### Step 1: Flash and boot

- [ ] In Raspberry Pi Imager, choose Raspberry Pi OS Lite (64-bit).
  - Set hostname: `ziggy-<kid>`
  - Set a user
  - Configure Wi-Fi
  - Add SSH with your public key
- [ ] With the board unpowered, attach the DSI ribbon cable to the Pi and the display.
- [ ] Power on the Pi.
- [ ] Run: `ssh ziggy-<kid>.local uname -m`
  - Expected output: `aarch64`

**Result: PASS / FAIL**  
**Notes:**

### Step 2: Provision and check the display

Run:
```bash
scripts/provision.sh ziggy-<kid>.local
ssh ziggy-<kid>.local sudo reboot
```

After reboot, run:
```bash
ssh ziggy-<kid>.local 'ls /dev/dri; cat /sys/class/drm/*DSI*/status; ls /sys/class/backlight'
```

Expected:
- [ ] `/dev/dri/` shows `card0` or `card1`.
- [ ] DSI status line reads `connected`.
- [ ] `/sys/class/backlight` has an entry (backlight control available). If missing, dimming falls back to black overlay, which is acceptable.

If the UI doesn't appear after provisioning, check `journalctl -u ziggy-player -n 50` for EGL/DRM errors. Verify Mesa runtime packages `libegl-mesa0` and `libgl1-mesa-dri` are installed.

**Result: PASS / FAIL**  
**Notes:**

### Step 3: Sign in, build, deploy

Create a device secrets directory and sign in to Spotify using the kid's account.

Run on the Mac:
```bash
cargo run -p ziggy-setup -- auth --state-dir secrets/<kid>
cargo run -p ziggy-setup -- probe --state-dir secrets/<kid>
```

Create the device config:
```bash
cp devices/example.toml devices/<kid>.toml
# Edit devices/<kid>.toml: set device_name, e.g. "Leo's Ziggy"
```

Build and deploy:
```bash
scripts/build-pi.sh
scripts/deploy.sh ziggy-<kid>.local devices/<kid>.toml secrets/<kid>
```

Expected:
- [ ] Deployment completes without errors.
- [ ] SSH to the Pi: `systemctl status ziggy-player` shows `active (running)`.
- [ ] Touchscreen displays the playlists grid.

**Result: PASS / FAIL**  
**Notes:**

### Step 4: Release checklist

Test the device with real usage patterns. Record pass/fail and notes for each item.

- [ ] **Cold boot:** Unplug the Pi for 5 seconds, plug it back in. Device reaches the playlists grid with no console text visible. Note the boot time: _____ seconds.
- [ ] **Touch responsiveness:** Taps land where touched. Swipe-scrolling the grid and track lists is smooth.
- [ ] **Audio playback:** Play a playlist on the headphone jack. Audio starts within 1s. Pause, skip (next/previous), and volume changes respond within 0.5s.
- [ ] **Memory usage:** After 10 minutes of browsing and playing, run `systemctl status ziggy-player | grep Memory`. Should be under 250MB. Also run `free -m` and record free RAM: _____ MB.
- [ ] **Idle behavior:** Pause playback. After 3 minutes without touch, clock overlay appears. After 10 minutes, screen goes black. A tap wakes the screen.
- [ ] **Wi-Fi drop recovery:** Run `ssh ziggy-<kid>.local sudo ip link set wlan0 down` for 30 seconds, then `up`. Cached playlists/albums remain visible. Playback resumes after pressing Play within ~1 minute.
- [ ] **Crash recovery:** Run `ssh ziggy-<kid>.local sudo pkill -9 ziggy-player`. UI reappears within ~3 seconds. Systemd auto-restart works.
- [ ] **Logs:** Run `ssh ziggy-<kid>.local journalctl -u ziggy-player -n 50`. No repeating error messages.

**Result: PASS / FAIL**  
**Notes:**

### Step 5: Bluetooth speaker (if used)

If the device uses a Bluetooth speaker instead of the headphone jack, configure and test it here.

Pair the speaker:
```bash
ssh -t ziggy-<kid>.local bluetoothctl
# Inside bluetoothctl:
# power on
# agent on
# scan on
# (wait for the speaker to appear)
# pair <MAC>
# trust <MAC>
# connect <MAC>
# quit
```

Configure the player:
```bash
# Edit devices/<kid>.toml:
# audio_device = "bluealsa:DEV=<MAC>,PROFILE=a2dp"
# bluetooth_speaker = "<MAC>"

scripts/deploy.sh ziggy-<kid>.local devices/<kid>.toml secrets/<kid>
```

Bluetooth tests:
- [ ] Audio plays on the speaker.
- [ ] Turning the speaker off: "Speaker not connected" banner appears within ~10s.
- [ ] Turning the speaker back on: device reconnects and the banner clears.
- [ ] If music stutters while browsing: this is Wi-Fi and Bluetooth sharing one radio (Pi 3 limitation). Note if observed.

**Result: PASS / FAIL**  
**Notes:**

## 3. Results

Record device boot time, memory usage, and free RAM for the release record. Include one row per device.

| Device | Date | Boot time (s) | Memory (systemctl) | free -m | Notes |
|--------|------|---------------|-------------------|---------|-------|
| Device A |  |  |  |  |  |
| Device B |  |  |  |  |  |

## Next steps

After completing all checks:

```bash
git add docs/hardware-checklist.md devices/<kid>.toml
git commit -m "docs: record hardware checklist for <kid>'s device"
```

Reference: [Phase 1 design spec](superpowers/specs/2026-10-03-ziggy-player-design.md) | [Phase 1 plan](superpowers/plans/2026-10-03-ziggy-player-phase1.md)

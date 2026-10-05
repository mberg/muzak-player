# Muzak Player — Design

Date: 2026-10-03
Status: Draft for review

## Purpose

A dedicated music player for two kids (one device each). It streams Spotify on a touchscreen and by voice, and does nothing else: no browser, no general internet access. Later it may show a calendar or similar.

Success means:

- It boots straight into the player and feels snappy and native to touch.
- A kid can find and play their playlists, albums, and liked songs without help.
- A kid can say a wake word and ask for a song, album, artist, or playlist, and it plays.
- No configuration happens on the device; everything is managed from the parent's Mac.

## Hardware (per device)

- Raspberry Pi 3 Model A+ (quad Cortex-A53 @ 1.4GHz, 512MB RAM, one USB 2.0 port, Wi-Fi + Bluetooth 4.2).
- Hosyond 7" IPS DSI touchscreen, 800×480, capacitive, driver-free (uses the standard Pi 7" display driver).
- Small USB microphone (uses the only USB port).
- Speaker over the 3.5mm jack or Bluetooth. GPIO stays free, so an I2S DAC HAT is a possible later upgrade.

## Decisions

| Topic | Decision |
|---|---|
| Language | Rust |
| UI | Slint, `linuxkms` backend on the Pi (no X/Wayland), GPU renderer; normal window on macOS |
| Playback | librespot embedded as a library (fallback: go-librespot as a separate process) |
| Content data | Spotify Web API, using access tokens from the librespot session (no developer app needed). Fallback if Spotify rejects those tokens: the parent's own developer app. |
| Voice | openWakeWord models run in-process (fallback: Porcupine), then Gemini Flash with audio input and function calling |
| OS | Raspberry Pi OS Lite 64-bit, no desktop |
| Configuration | Done off-device with `muzak-setup` on the Mac, pushed over SSH. No settings UI on the device. |
| Accounts | Each kid's own Spotify Premium Family member account |
| Parental controls | None |

## Architecture

One Rust binary, `muzak-player`, runs as a systemd service started at boot.

```
┌──────────────────────── muzak-player ─────────────────────────┐
│ ui (Slint) ◄── state ── app core ── commands ──► player        │
│    └── taps ───────────►  (event loop)           (librespot)  │
│                           │      ▲                  │ audio   │
│                library ◄──┘      └── intents ── voice         │
│          (Web API + local cache)                              │
└───────────────────────────────────────────────────────────────┘
```

Components communicate only through typed messages over channels (tokio).

- **player** wraps librespot: session login, load a context (playlist, album, artist) or track, play/pause/next/previous/seek/volume, shuffle/repeat. Emits track-changed, position, state, and error events. It also registers as a Spotify Connect device, so a phone can cast to it.
- **library** reads the kid's playlists, saved albums, liked songs, recently played, track lists, and search results from the Web API. It caches everything on disk and serves cached data first, then refreshes in the background. It downloads cover art at 300px, decodes it off the UI thread, and caches it on disk.
- **app core** owns application state. It turns UI taps and voice intents into player/library commands and pushes state changes to the UI. This is the main unit-tested piece.
- **ui** holds the Slint screens. It renders state and sends taps. It has no network or audio access.
- **voice** (phase 2): wake word, recording, Gemini, then an intent to the app core.

Audio output: ALSA to the 3.5mm jack by default; Bluetooth speakers via bluez-alsa. Selected in config.

On macOS the same binary runs as an 800×480 window, using CoreAudio output and the Mac mic.

## Screens (800×480 landscape)

Persistent elements: a left rail with large icons (Playlists, Albums, Liked Songs, Recent) and a bottom mini player (cover, title, play/pause; tap to open Now Playing).

1. **Library grid**: cover tiles with names, 4 per row, swipe to scroll.
2. **Playlist/album detail**: cover, title, large Play and Shuffle buttons, track list. Tapping a track plays the context from that track.
3. **Now Playing**: large cover, title, artist, draggable progress bar, previous/play/next, volume slider, shuffle and repeat toggles.
4. **Idle**: after a few minutes with no playback and no touch, the screen dims to a clock, then blanks. A tap wakes it.
5. **Listening overlay** (phase 2): listening indicator, then "Playing *X*" or "Sorry, I didn't catch that." Includes an on-screen mic button that triggers the same flow as the wake word.

Feel requirements:

- Optimistic updates: the UI changes on tap, before the action completes.
- Cover art fades in once loaded.
- Touch targets are at least 64px.
- No animation that cannot hold 60fps on a Pi 3.

Out of v1: typed search (needs a custom on-screen keyboard; voice covers search), Wi-Fi and Bluetooth pairing screens (done at provisioning), and playlist creation (phase 3).

## Setup, authentication, configuration

**Web API access.** librespot can issue Web API access tokens for the signed-in account (this is how other librespot-based players browse Spotify). That avoids a developer app and its development-mode limits. `muzak-setup probe` checks this works for each endpoint we use. If Spotify rejects these tokens, the fallback is the parent's own developer app at developer.spotify.com with both kids' accounts allowlisted.

**Sign-in.** Spotify only allows OAuth redirects to HTTPS or to loopback (`127.0.0.1`), so sign-in runs on the Mac. `muzak-setup auth --state-dir secrets/<kid>`:

1. Runs a loopback OAuth flow in the Mac browser; the parent signs in as the kid.
2. Connects a librespot session once, which saves reusable credentials to `secrets/<kid>/librespot/credentials.json`.

`scripts/deploy.sh` copies that file to the device. The device mints fresh access tokens from it as needed.

**Configuration.** Each device has a TOML file in `devices/<kid>.toml`, installed by `scripts/deploy.sh` as `/etc/muzak/config.toml`:

- device name (also the Spotify Connect name)
- audio backend and ALSA device (headphone jack or a bluez-alsa Bluetooth device)
- Bluetooth speaker address, if any
- initial volume
- dim and sleep timeouts

Gemini and wake word settings are added in phase 2. The device never edits its own config.

**Cache.** JSON files for playlist/album lists and track lists, plus image files for covers, under `/var/lib/muzak/`. No database.

## Data flow example

1. A tap on a playlist tile sends `OpenPlaylist(id)`. The core shows cached tracks immediately; the library refreshes in the background and the screen updates if anything changed.
2. A tap on Play: the core sets the state to Now Playing for the first track immediately, then sends `player.load(context_uri, start_index)`.
3. librespot events (track changed, position, paused) keep the UI in sync.

## Voice (phase 2)

1. **Wake word** is detected locally from the always-on mic. No audio leaves the device before detection. Engine: openWakeWord ONNX models run in-process (via `ort` or `tract`). A custom phrase (for example "Hey Muzak") is trained with the openWakeWord training notebook. Fallback engine: Porcupine. The on-screen mic button triggers the same flow.
2. **Duck and record.** Music volume drops to about 20%, a chime plays, and the device records 16kHz mono until voice activity ends, up to 6 seconds.
3. **Understand.** The clip is sent to Gemini Flash with a system prompt that includes the kid's playlist names and these tools:
   - `play(query, kind: track|album|playlist|artist)`
   - `pause`, `resume`, `next`, `previous`
   - `set_volume(level)`
   - `whats_playing`
4. **Resolve and play.** Fuzzy-match the kid's own playlists and albums first, then fall back to Web API search. Show the result in the overlay and restore the volume.

Target latency: about 3 seconds from end of speech to playback start.

Wake word reliability while music plays: v1 relies on a tuned threshold plus the on-screen mic button. Later, if needed, add acoustic echo cancellation (`webrtc-audio-processing`) using librespot's output samples as the reference signal. This is effective on the 3.5mm jack and less so over Bluetooth because of its variable latency.

Privacy: audio is sent to Google only after the wake word. Use a paid Gemini API key so the audio is not used for training.

## Failures and recovery

| Situation | Kid sees | Device does |
|---|---|---|
| No Wi-Fi | Cached grids; "No internet" on Play | Retries in the background and recovers automatically |
| Spotify auth invalid | "Ask a grown-up" screen | Logs it; the parent reruns `muzak-setup auth` |
| Track unavailable | Skips to the next track | Logs the track |
| Account playing elsewhere | Paused (librespot does not report the other device's name) | Normal Spotify Connect behavior |
| Bluetooth speaker missing | "Speaker not connected" | Retries the connection |
| Gemini error or no match | "Sorry, I didn't catch that" | Restores the volume |
| Process crash | Brief blank screen | systemd restarts it within about 2 seconds |

Logs go to journald.

## Development, testing, deployment

- **Mac development:** `cargo run` opens the 800×480 window with real playback. A `--fake` mode uses stub player and library implementations for UI work without Spotify.
- **Tests:**
  - Unit tests for app-core state transitions (events in, state out).
  - Tests for voice intent resolution and fuzzy matching.
  - Tests for cache read/write, using recorded Web API fixtures.
  - No hardware or network needed.
- **Build:** `scripts/build-pi.sh` compiles inside an arm64 Debian Trixie container (native speed on Apple Silicon). This avoids cross-compiling the ALSA, DRM and libinput system libraries.
- **Commands:**
  - `muzak-setup auth` and `muzak-setup probe`: sign-in and Web API check, as above.
  - `scripts/provision.sh <host>`: one-time OS setup over SSH (display overlay, audio, bluez-alsa, systemd unit, journald limits).
  - `scripts/deploy.sh <host> <config> [secrets-dir]`: copy the binary, config and credentials, then restart the service.
- **Memory check:** read the service's memory use on the Pi (`systemctl status`) during the hardware checklist.
- **Hardware checklist per release:**
  - boots into the app
  - touch works
  - audio on jack and on Bluetooth
  - wake word triggers
  - recovers after a Wi-Fi drop

## Phases

1. **Playback and touchscreen UI**, plus `muzak-setup` (provision, auth, deploy) and Mac development mode.
2. **Voice:** wake word, Gemini, resolve and play.
3. **Playlist creation and editing.**
4. **Later:** calendar or other screens, typed search with an on-screen keyboard, echo cancellation.

Each phase gets its own implementation plan.

## Memory budget (target)

| Part | RAM |
|---|---|
| Raspberry Pi OS Lite | ~80MB |
| Slint UI | ~15–30MB |
| librespot and audio buffers | ~30–50MB |
| Wake word | ~20–50MB |
| In-memory cover art | ~20MB |
| **Total** | **~165–230MB of 512MB** |

## Open items to verify during implementation

- Whether the Web API accepts librespot session tokens for every endpoint we use (`muzak-setup probe`). If not, switch to the developer-app fallback and check its development-mode limits.
- Whether the playlist items endpoint is `/playlists/{id}/items` or `/playlists/{id}/tracks` in 2026. The client tries `items` first and falls back to `tracks`.
- Slint `linuxkms` backend with the vc4 KMS driver on Pi 3 A+: renderer choice (FemtoVG vs Skia) and touch input via libinput.
- openWakeWord inference cost on the Cortex-A53, and the choice between `ort` and `tract`.
- Porcupine free-tier terms, if the fallback is needed.

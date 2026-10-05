# Muzak Player

A music player with a touchscreen, built on a Raspberry Pi. It plays your Spotify playlists, albums and liked songs, searches Spotify, and does nothing else: no apps, no feeds, no ads.

- **Spotify:** your library, search, playlist editing and hearting songs, with recently played kept on the player.
- **Speakers:** the headphone jack, a Bluetooth speaker, or a Sonos room.
- **Audiobooks:** browse an [Audiobookshelf](https://www.audiobookshelf.org/) server at home.
- **Voice control:** say "ziggy, play Road Trip" or "ziggy, heart this". Voice is optional and needs a USB microphone.
- **Bedtime:** a sleep timer that fades the music out, and a screen that dims and turns itself off.

## What you need

- A Raspberry Pi 3 A+ or newer, with the official 7" touchscreen, a power supply and a microSD card (8 GB or more).
- Speakers or headphones: the Pi's headphone jack, a Bluetooth speaker, or a Sonos system.
- A Spotify Premium account for each person.
- A Mac or Linux computer to run setup from, on the same Wi-Fi as the Pi.
- Optional: a USB microphone for voice control, and a Google Cloud project or Gemini API key for it.

## Get started

### 1. Install the setup tool

On your computer, run:

```bash
curl -fsSL https://raw.githubusercontent.com/mberg/muzak-player/main/install.sh | sh
```

This installs `muzak` into `~/.local/bin`. If the installer says that folder isn't on your PATH, follow the line it prints.

### 2. Prepare the SD card

1. Install [Raspberry Pi Imager](https://www.raspberrypi.com/software/).
2. Choose your Pi model, then **Raspberry Pi OS Lite (64-bit)**.
3. Open the settings (the gear, or "Edit settings") and set:
   - a hostname, such as `muzak-kitchen`;
   - a username and password;
   - your Wi-Fi network;
   - **SSH**, using your public key. Without a key, setup can't reach the Pi.
4. Write the card, put it in the Pi, connect the display, and power the Pi on.

### 3. Run setup

```bash
muzak setup
```

Setup asks one thing at a time:

1. **The Pi's address**, such as `muzak-kitchen.local`. It checks it can connect.
2. **The player's name and sound quality.** Spotify shows the name when you pick where to play. Quality is 320 kbps unless you choose a lower setting to save data.
3. **Spotify.**
   - The first time, it walks you through creating a free Spotify developer app. One app serves every player and every person.
   - Then a browser opens twice to sign in, once for playback and once for the library, and setup checks it all works.
4. **Extras.** An Audiobookshelf address and voice control, both optional.
5. **Install.** It prepares the Pi, installs the newest player and copies your settings across. The first time takes a few minutes and restarts the Pi.

When setup finishes, the player is on the screen.

### 4. On the touchscreen

- **Settings → Speaker** pairs a Bluetooth speaker or picks a Sonos room. Put a Bluetooth speaker in pairing mode first.
- **Settings → Audiobooks** signs in to Audiobookshelf. Each player can use its own Audiobookshelf user.
- **Settings → Colours** and **Sleep timer** take effect at once.

## Looking after a player

Run these from your computer:

| Command | What it does |
|---|---|
| `muzak config muzak-kitchen.local` | Change settings from a menu: the name, sound quality, volume, screen timeouts, Audiobookshelf and voice. Saving restarts the player. |
| `muzak update muzak-kitchen.local` | Install the newest player and restart it. |
| `muzak status muzak-kitchen.local` | Check the player is running, and which version it is. |
| `muzak logs muzak-kitchen.local` | Watch what the player is doing, for when something's wrong. |
| `muzak signin muzak-kitchen.local` | Sign a different Spotify account in on that player. |

`muzak setup` can be run again on the same Pi at any time. It keeps the Spotify sign-in unless you choose to sign in again.

Choices made on the touchscreen take priority over the config. For example, a name changed in Settings stays until you change it there again.

## How it starts

The player runs as a system service, so it starts by itself whenever the Pi powers on. If it ever stops, it restarts. There's no desktop: the Pi boots straight into the player.

Setup trims the boot:
- there's no splash screen or boot delay, and no console login;
- background package jobs are off;
- **the player doesn't wait for Wi-Fi.** It shows your saved library straight away and connects to Spotify once the network is up.

## Voice control

Voice needs a USB microphone and access to Google's Gemini. Plug the microphone into the Pi before running setup, so setup can make it the Pi's recording device. If you add one later, run `muzak setup` again. Setup offers two ways to reach Gemini:

- **A Google Cloud service account key file (Vertex AI).** Use this if you have Google Cloud credits. In a Google Cloud project, turn on Vertex AI and create a service account with the "Vertex AI User" role. Then create a JSON key for it and give setup the file.
- **A Gemini API key** from [Google AI Studio](https://aistudio.google.com/).

How it works:
1. **Wake word.** "Ziggy" is heard on the Pi itself. No audio leaves the Pi until then.
2. **Gemini.** The request is sent to Gemini, which picks one action.
3. **Action.** The player carries it out exactly as if you'd tapped it.

The microphone button in Search does the same without the wake word.

You can ask to play anything in your library or on Spotify, pause, skip, change the volume, set the sleep timer, heart the song, save its album, or add it to one of your playlists.

To change the wake word or its sensitivity, run `muzak config`.

## Troubleshooting

- **Setup can't connect to the Pi.** Check that `ssh muzak-kitchen.local` works. The hostname and SSH key must match what you set in Imager. If your username on the Pi differs from your computer's, use `pi@muzak-kitchen.local`.
- **Setup says sudo asks for a password.** Newer Raspberry Pi OS images do this for the Imager user. Run the one-line command setup prints, which asks for the Pi's password once, then continue.
- **Spotify says it's limiting requests.** Spotify sometimes rate-limits a developer app for up to a day. Setup saves the sign-in and carries on. The library loads once the limit ends, and playing music isn't affected.
- **Spotify says some requests were refused.** Add the account's email under "User Management" in the Spotify developer app, then run `muzak signin`.
- **The screen is blank.** Run `muzak status`, then `muzak logs` to see why.

## For developers

The code:
- `crates/muzak-player` is the touchscreen app, in Rust with [Slint](https://slint.dev). It uses librespot for playback and the Spotify Web API for the library.
- `crates/muzak-setup` builds the `muzak` tool.
- `deploy/` holds what's installed on the Pi.
- `docs/` holds the design specs and the [manual test checklists](docs/testing.md).

To build and test, you need Rust 1.97.1 (`asdf install` reads `.tool-versions`) and Xcode command line tools on a Mac:

```bash
cargo test --workspace
cargo run -p muzak-player -- --fake      # the UI with a made-up library, no Spotify needed
```

Docker Desktop is needed only to build the Pi player:

```bash
scripts/build-pi.sh                                                   # builds target/pi/release/muzak-player
cargo run -p muzak-setup --bin muzak -- update muzak-kitchen.local \
    --player-binary target/pi/release/muzak-player                    # tries it on a Pi
```

**Releasing.** Raise `version` in `Cargo.toml`, then push a tag:

```bash
git tag v0.2.0 && git push origin v0.2.0
```

The release workflow builds the Pi player and `muzak` for Macs and Linux, then publishes them as a GitHub release. `install.sh`, `muzak setup` and `muzak update` all use the newest release.

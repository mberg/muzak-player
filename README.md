# Ziggy

Ziggy is a music player with a touchscreen, built on a Raspberry Pi. It plays your Spotify playlists, albums and liked songs, and you can ask it for music out loud. It does nothing else: no apps, no feeds, no ads.

- **Spotify:** your library, search, playlist editing and hearting songs, with recently played kept on the player.
- **Voice:** say "ziggy, play Road Trip" or "ziggy, heart this song". Voice is optional and needs a USB microphone.
- **Speakers:** the headphone jack, a Bluetooth speaker, or a Sonos room.
- **Audiobooks:** browse an [Audiobookshelf](https://www.audiobookshelf.org/) server at home.
- **Bedtime:** a sleep timer that fades the music out, and a screen that dims and turns itself off.
- **One person per player:** each Ziggy is signed in to one Spotify account, so everyone's library, likes and history stay their own.

## How it's built

Ziggy is written in [Rust](https://www.rust-lang.org/). The touchscreen interface is built with [Slint](https://slint.dev), a toolkit for fast, native user interfaces. Slint draws straight to the Pi's screen with its GPU, with no desktop underneath, so Ziggy starts in seconds and runs smoothly on a Pi 3.

It's built on these open-source projects:

| Project | What Ziggy uses it for |
|---|---|
| [Slint](https://slint.dev) | The touchscreen interface |
| [librespot](https://github.com/librespot-org/librespot) | Playing Spotify on the Pi, and appearing in Spotify's device list |
| [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) | Hearing the wake word "ziggy", on the Pi itself |
| [BlueALSA](https://github.com/arkq/bluez-alsa) | Playing through Bluetooth speakers |
| [sonor](https://github.com/jakobhellermann/sonor) | Controlling Sonos speakers on your home network |

It also uses Google's Gemini to understand spoken requests, and the Spotify Web API for your library.

## What you need

- **A Raspberry Pi:** a Pi 3 A+ or newer, with a microSD card (8 GB or more). A Pi 3 needs a power supply that gives 5V at 2.5A or more.
- **A screen:** a 7" DSI touchscreen (800×480), such as the official Raspberry Pi display.
- **Something to play through:** the Pi's headphone jack, a Bluetooth speaker, or a Sonos system.
- **Spotify Premium:** one account for each person. Family plan accounts work.
- **A Mac** to run setup from, on the same Wi-Fi as the Pi.
- **For voice (optional):** a USB microphone, plus a Google Cloud project or a Gemini API key.

## Get started

### 1. Install the setup tool

On your Mac, run:

```bash
curl -fsSL https://raw.githubusercontent.com/mberg/ziggy/main/install.sh | sh
```

This installs `ziggy` into `~/.local/bin`. If the installer says that folder isn't on your PATH, follow the line it prints.

### 2. Get your Mac's SSH key ready

Setup reaches the Pi over SSH, using a key instead of a password. Your Mac needs a key, and the Pi needs a copy of its public half. That's the half that's safe to share; the private half never leaves your Mac.

1. In Terminal, check whether your Mac already has a key:

   ```bash
   ls ~/.ssh/id_ed25519.pub
   ```

2. If it says "No such file or directory", make one. Press Enter at each question to accept the defaults:

   ```bash
   ssh-keygen -t ed25519
   ```

3. Copy the public key, so you can paste it into Imager in the next step:

   ```bash
   pbcopy < ~/.ssh/id_ed25519.pub
   ```

   It's one line that starts with `ssh-ed25519`.

### 3. Prepare the SD card

1. Install [Raspberry Pi Imager](https://www.raspberrypi.com/software/) and put the microSD card in your Mac.
2. In Imager, choose:
   - **Device:** your Pi model, such as Raspberry Pi 3.
   - **Operating system:** Raspberry Pi OS (other) → **Raspberry Pi OS Lite (64-bit)**.
   - **Storage:** the microSD card.
3. When Imager offers to customise the image, set:
   - **Hostname:** a name for this player, such as `ziggy-kitchen`. Each Ziggy needs its own.
   - **Username and password:** for example `ziggy`. You'll use the username to reach the Pi.
   - **Wi-Fi:** your network name and password, and your country.
   - **SSH:** turn it on and choose **public-key authentication only**. Paste the key you copied in step 2. If Imager says "no SSH keys configured", that's this paste step.
   - **Raspberry Pi Connect:** leave it off. Ziggy doesn't need it.
4. Write the card. Put it in the Pi, plug in the screen and the USB microphone if you have one, then power the Pi on.

The first boot takes a couple of minutes. To check the Pi is ready, run this with your username and hostname:

```bash
ssh ziggy@ziggy-kitchen.local
```

If it logs you in without asking for a password, the key works. Type `exit` to come back.

### 4. Run setup

```bash
ziggy setup
```

Setup asks one thing at a time:

1. **The Pi's address**, such as `ziggy@ziggy-kitchen.local`. It checks it can connect.
2. **The player's name and sound quality.** Spotify shows the name when you pick where to play. Quality is 320 kbps unless you choose a lower setting to save data.
3. **Spotify.**
   - The first time, it walks you through creating a free Spotify developer app. One app serves every player and every person.
   - Then a browser opens twice to sign in, once for playback and once for the library, and setup checks it all works.
4. **Extras.** An Audiobookshelf address and voice control, both optional.
5. **Install.** It prepares the Pi, installs the newest player and copies your settings across. The first time takes a few minutes and restarts the Pi.

When setup finishes, the player is on the screen.

To set up a Ziggy for someone else:
1. Add their email under "User Management" in the Spotify developer app.
2. When setup opens the Spotify pages, open them in a private browser window and sign in as that person.

### 5. On the touchscreen

Settings has, from the top:
- **Speaker:** pair a Bluetooth speaker or pick a Sonos room. Put a Bluetooth speaker in pairing mode first.
- **Sleep timer:** 30 or 60 minutes.
- **Colours:** five themes, including Northern Lights.
- **Audiobooks:** sign in to Audiobookshelf. Each player can use its own Audiobookshelf user.
- **Device name** and the **Spotify account** in use.

The Pi's temperature is shown at the top of Settings.

## Looking after a player

Run these from your Mac:

| Command | What it does |
|---|---|
| `ziggy status ziggy@ziggy-kitchen.local` | Check the player is running and which version it is. It also reports crashes, restarts, power or heat trouble, memory and temperature. |
| `ziggy update ziggy@ziggy-kitchen.local` | Install the newest player and restart it. |
| `ziggy config ziggy@ziggy-kitchen.local` | Change settings from a menu: the name, sound quality, volume, screen timeouts, Audiobookshelf and voice. Saving restarts the player. |
| `ziggy signin ziggy@ziggy-kitchen.local` | Sign a different Spotify account in on that player. |
| `ziggy logs ziggy@ziggy-kitchen.local` | Watch what the player is doing, for when something's wrong. |

`ziggy setup` can be run again on the same Pi at any time. It keeps the Spotify sign-in unless you choose to sign in again.

Choices made on the touchscreen take priority over the config. For example, a name changed in Settings stays until you change it there again.

The Pi keeps its logs across restarts, so `ziggy status` can still show what went wrong after a crash.

## How it starts

The player runs as a system service, so it starts by itself whenever the Pi powers on. If it ever stops, it restarts. There's no desktop: the Pi boots straight into the player.

Setup trims the boot:
- there's no splash screen or boot delay, and no console login;
- background package jobs are off;
- **the player doesn't wait for Wi-Fi.** It shows your saved library straight away and connects to Spotify once the network is up.

## Voice control

Voice needs a USB microphone and access to Google's Gemini. Plug the microphone into the Pi before running setup, so setup can make it the Pi's recording device and turn up its gain. If you add one later, run `ziggy setup` again. Setup offers two ways to reach Gemini:

- **A Google Cloud service account key file (Vertex AI).** Use this if you have Google Cloud credits. In a Google Cloud project, turn on Vertex AI and create a service account with the "Vertex AI User" role. Then create a JSON key for it and give setup the file.
- **A Gemini API key** from [Google AI Studio](https://aistudio.google.com/).

How it works:
1. **Wake word.** "Ziggy" is heard on the Pi itself, by sherpa-onnx. No audio leaves the Pi until then.
2. **Gemini.** The request is sent to Gemini, which picks one action.
3. **Action.** The player carries it out exactly as if you'd tapped it, and shows what it did at the bottom of the screen.

The microphone button in Search does the same without the wake word.

You can ask it to:
- play anything in your library or on Spotify;
- pause, skip, or change the volume;
- set the sleep timer;
- heart the song, save its album, or add it to one of your playlists.

To change the wake word or its sensitivity, run `ziggy config`.

## Troubleshooting

- **Setup can't connect to the Pi.**
  - Check that `ssh ziggy@ziggy-kitchen.local` works, with the username and hostname you set in Imager.
  - If it asks for a password, the key didn't reach the Pi. Write the card again and paste the key in Imager's SSH setting (see steps 2 and 3).
- **Setup says sudo asks for a password.** Newer Raspberry Pi OS images do this for the Imager user, and setup needs sudo without one. Run this on your Mac, with your username and hostname. It asks for the Pi's password once:

  ```bash
  ssh -t ziggy@ziggy-kitchen.local 'echo "ziggy ALL=(ALL) NOPASSWD:ALL" | sudo tee /etc/sudoers.d/010_ziggy-nopasswd >/dev/null && sudo chmod 440 /etc/sudoers.d/010_ziggy-nopasswd'
  ```

  If your username isn't `ziggy`, replace each `ziggy` inside the quotes with it. Then run `ziggy setup` again.
- **Spotify says it's limiting requests.** Spotify sometimes rate-limits a developer app for up to a day. Setup saves the sign-in and carries on. The library loads once the limit ends, and playing music isn't affected.
- **Spotify says some requests were refused.** Add the account's email under "User Management" in the Spotify developer app, then run `ziggy signin`.
- **Some songs skip straight to the next one.** On a Family plan, Spotify filters explicit songs for everyone except the plan manager. The manager can allow them for each member at spotify.com → Account → Premium Family.
- **You have to shout "ziggy".** The microphone's gain may be low. Run `ziggy setup` again with the microphone plugged in.
- **The screen is blank, or the player keeps restarting.** Run `ziggy status`, then `ziggy logs` to see why.
- **`ziggy status` mentions low power.** Use a power supply that gives 5V at 2.5A or more, with a short cable.

## For developers

The code:
- `crates/ziggy-player` is the touchscreen app, in Rust with Slint. The interface lives in `ui/` as `.slint` files.
- `crates/ziggy-setup` builds the `ziggy` tool.
- `deploy/` holds what's installed on the Pi.
- `docs/` holds the design specs and the [manual test checklists](docs/testing.md).

To build and test, you need Rust 1.97.1 (`asdf install` reads `.tool-versions`) and the Xcode command line tools:

```bash
cargo test --workspace
cargo run -p ziggy-player -- --fake      # the interface with a made-up library, no Spotify needed
```

Docker Desktop is needed only to build the Pi player:

```bash
scripts/build-pi.sh                                                   # builds target/pi/release/ziggy-player
cargo run -p ziggy-setup --bin ziggy -- update ziggy@ziggy-kitchen.local \
    --player-binary target/pi/release/ziggy-player                    # tries it on a Pi
```

**Releasing.** Raise `version` in `Cargo.toml`, then push a tag:

```bash
git tag v0.3.0 && git push origin v0.3.0
```

The release workflow builds the Pi player and `ziggy` for Macs, then publishes them as a GitHub release. `install.sh`, `ziggy setup` and `ziggy update` all use the newest release.

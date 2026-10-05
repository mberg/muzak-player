# Muzak Player Phase 1 (Playback + Touchscreen UI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust program that runs on a Raspberry Pi 3 A+ with a 7" 800×480 DSI touchscreen and on a Mac. It shows a kid's Spotify playlists, albums, liked songs and recently played as touch tiles, and plays them through librespot.

**Architecture:** One binary, `muzak-player`. A pure, unit-tested app core (`app`) turns inputs (taps, player events, library results, ticks) into state changes and effects. Effects are carried out by:
- a librespot player task,
- a library service (Spotify Web API plus a disk cache),
- a platform module (backlight, Bluetooth speaker).

A Slint UI renders a view model built from state. A `--fake` mode swaps Spotify for an in-memory catalog and player, so UI work needs no account. A second small binary, `muzak-setup`, signs a kid's account in on the Mac. Shell scripts build for the Pi in Docker, provision the Pi, and deploy.

**Tech Stack:**
- Rust 1.97 (edition 2024), tokio
- librespot 0.8.0 (connect, playback, oauth)
- Slint 1.18: winit backend on macOS; linuxkms-noseat + FemtoVG on the Pi
- reqwest 0.13 with rustls, serde, `image` 0.25
- Raspberry Pi OS Lite 64-bit (Debian Trixie), systemd, bluez-alsa

**Spec:** `docs/superpowers/specs/2026-10-03-ziggy-player-design.md`

## Global Constraints

**Toolchain and libraries**
- Rust toolchain `1.97.1`, pinned in `.tool-versions` (asdf). Edition 2024.
- librespot `0.8.0` with `default-features = false` and `rustls-tls-webpki-roots`. Audio backend: `rodio-backend` on macOS, `alsa-backend` on Linux.
- Slint `1.18`, `default-features = false`. Backend: `backend-winit` on macOS, `backend-linuxkms-noseat` on Linux. Renderer: FemtoVG. On the Pi the service sets `SLINT_BACKEND=linuxkms-femtovg`.

**Window and UI**
- The window is exactly 800×480, landscape.
- The UI never touches the network or audio. It renders state and sends `UiAction`s.
- Touch targets are at least 64px. Cover art is decoded at most 300px on its longest side.

**Configuration and storage**
- No settings UI on the device. Configuration comes from `/etc/muzak/config.toml` (on the Mac, `dev/config.toml`).
- On-device storage, under `state_dir`:
  - `librespot/credentials.json`
  - `cache/*.json`
  - `images/*`
- Secrets on the Mac live in `secrets/<kid>/`, which is gitignored.

**Spotify**
- The liked-songs pseudo-collection URI is `muzak:liked`. The player maps it to `spotify:user:<username>:collection`.
- Web API scopes: `playlist-read-private,playlist-read-collaborative,user-library-read,user-read-recently-played`.

**Kid-facing copy** (exact strings)
- Section titles: "Playlists", "Albums", "Liked Songs", "Recent"
- Errors and notices:
  - "Ask a grown-up for help"
  - "This player needs to sign in to Spotify again."
  - "No internet right now"
  - "That song can't play, skipping"
  - "Speaker not connected"
- Empty and loading states:
  - "Nothing here yet"
  - "No songs here yet"
  - "Can't load this right now"
  - "Loading…"

## Review Focus

1. **Double taps on Play.** Kids double-tap. Two identical play requests within 1 second must produce one load, not a restart. The test is in Task 3.
2. **An empty library.** A new kid account has no playlists. The grid must show "Nothing here yet", not a blank or a spinner forever. The test is in Task 4.
3. **Odd playlist entries.** Playlists can contain removed tracks (`null`), local files, and podcast episodes. These must be skipped, not crash the parser or show broken rows. The test is in Task 5.
4. **Expired access tokens.** A request that gets HTTP 401 must retry once with a fresh token before reporting an auth problem. The test is in Task 5.
5. **Wi-Fi down at boot.** The device must show cached grids. When the player reconnects, it must re-request the library so the screens fill in. The test is in Task 3.

---

## File Structure

```
Cargo.toml                        workspace
.tool-versions                    rust 1.97.1
.gitignore
dev/config.toml                   Mac development config
devices/example.toml              template for a kid's device config
crates/muzak-player/
  Cargo.toml
  build.rs                        compiles ui/app.slint
  ui/
    app.slint                     AppWindow: properties + callbacks, layout of screens
    theme.slint                   colors, sizes
    types.slint                   TileData, TrackData, ScreenKind, LoadState
    fonts/Nunito-Bold.ttf, Nunito-ExtraBold.ttf
    icons/*.svg
    components/                   icon_button, pill_button, tile, track_row, scrubber, rail, mini_player, banner, state_message
    screens/                      grid, detail, now_playing, idle, auth_needed
  src/
    main.rs                       CLI args, window, wiring
    lib.rs                        module list + slint::include_modules!
    config.rs                     Config (TOML)
    model.rs                      Section, Collection, Track, Repeat, LIKED_URI
    app/mod.rs                    Core: handle(Input, now_ms) -> Vec<Effect>
    app/input.rs                  Input, UiAction, PlayerUpdate, LibraryUpdate, Effect, PlayerCommand, LibraryRequest
    app/state.rs                  AppState, Screen, Slot, Playback, Notice, DisplayMode
    app/tests.rs                  core unit tests
    view.rs                       AppState -> View (plain Rust view model)
    library/mod.rs                LibrarySource trait, FetchError
    library/web_api.rs            Spotify Web API client + response parsing
    library/session_tokens.rs     TokenSource backed by the librespot session
    library/cache.rs              DiskCache (JSON files)
    library/service.rs            cache-first library service task
    library/fake.rs               FakeCatalog + FakeSource
    images.rs                     cover download, disk cache, decode
    player/mod.rs                 volume conversions
    player/fake.rs                FakePlayer
    player/librespot.rs           librespot Spirc adapter
    runtime.rs                    tokio runtime: core loop + effect dispatch
    ui_bridge.rs                  View -> Slint properties, callbacks -> UiAction
    platform.rs                   backlight + Bluetooth speaker monitor
crates/muzak-setup/
  Cargo.toml
  src/main.rs                     `auth` and `probe` commands
deploy/
  Dockerfile.build                arm64 Trixie build image
  muzak-player.service            systemd unit
  provision-remote.sh             runs on the Pi as root
scripts/
  build-pi.sh
  provision.sh
  deploy.sh
```

---

### Task 1: Workspace, toolchain, config, placeholder window

**Files:**
- Create: `Cargo.toml`, `.tool-versions`, `.gitignore`, `dev/config.toml`
- Create: `crates/muzak-player/Cargo.toml`, `crates/muzak-player/build.rs`, `crates/muzak-player/ui/app.slint`
- Create: `crates/muzak-player/src/lib.rs`, `crates/muzak-player/src/main.rs`, `crates/muzak-player/src/config.rs`

**Interfaces:**
- Produces:
  - `muzak_player::config::Config` with pub fields:
    - `device_name: String`
    - `audio_backend: String`
    - `audio_device: Option<String>`
    - `state_dir: PathBuf`
    - `dim_after_secs: u64`
    - `off_after_secs: u64`
    - `initial_volume: u8`
    - `bluetooth_speaker: Option<String>`
  - `Config` methods:
    - `fn load(path: &Path) -> anyhow::Result<Config>`
    - `fn parse(text: &str) -> anyhow::Result<Config>`
    - `fn librespot_dir(&self) -> PathBuf`
    - `fn cache_dir(&self) -> PathBuf`
    - `fn images_dir(&self) -> PathBuf`

- [ ] **Step 1: Create workspace files**

`.tool-versions`:
```
rust 1.97.1
```

`.gitignore`:
```
/target
/dev/state
/secrets
```

`Cargo.toml`:
```toml
[workspace]
resolver = "3"
members = ["crates/muzak-player", "crates/muzak-setup"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
librespot = { version = "0.8.0", default-features = false, features = ["rustls-tls-webpki-roots"] }
```

The workspace lists `muzak-setup`, which is created in Task 10. Until then, create a stub so the workspace builds. `crates/muzak-setup/Cargo.toml`:
```toml
[package]
name = "muzak-setup"
version.workspace = true
edition.workspace = true

[dependencies]
```
`crates/muzak-setup/src/main.rs`:
```rust
fn main() {
    println!("muzak-setup: commands arrive in Task 10");
}
```

`dev/config.toml`:
```toml
device_name = "Muzak Dev"
state_dir = "dev/state"
```

- [ ] **Step 2: Create the player crate manifest and build script**

`crates/muzak-player/Cargo.toml`:
```toml
[package]
name = "muzak-player"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow = "1"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
clap = { version = "4", features = ["derive"] }
image = { version = "0.25", default-features = false, features = ["jpeg", "png"] }
librespot = { workspace = true }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
slint = { version = "1.18", default-features = false, features = ["std", "compat-1-2", "renderer-femtovg"] }
thiserror = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "fs", "process"] }
toml = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[target.'cfg(target_os = "macos")'.dependencies]
slint = { version = "1.18", default-features = false, features = ["backend-winit"] }
librespot = { workspace = true, features = ["rodio-backend"] }

[target.'cfg(target_os = "linux")'.dependencies]
slint = { version = "1.18", default-features = false, features = ["backend-linuxkms-noseat"] }
librespot = { workspace = true, features = ["alsa-backend"] }

[build-dependencies]
slint-build = "1.18"

[dev-dependencies]
tempfile = "3"
```

`crates/muzak-player/build.rs`:
```rust
fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");
}
```

`crates/muzak-player/ui/app.slint` (placeholder; Task 9 replaces it):
```slint
export component AppWindow inherits Window {
    width: 800px;
    height: 480px;
    title: "Muzak";
    background: #14121f;
    Text { text: "muzak"; color: white; font-size: 48px; }
}
```

- [ ] **Step 3: Write the failing config tests**

`crates/muzak-player/src/lib.rs`:
```rust
slint::include_modules!();

pub mod config;
```

`crates/muzak-player/src/config.rs` (tests first; the implementation follows in Step 5):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_uses_defaults() {
        let c = Config::parse("device_name = \"Leo\"\nstate_dir = \"/var/lib/muzak\"\n").unwrap();
        assert_eq!(c.device_name, "Leo");
        assert_eq!(c.initial_volume, 50);
        assert_eq!(c.dim_after_secs, 180);
        assert_eq!(c.off_after_secs, 600);
        assert_eq!(c.audio_device, None);
        assert_eq!(c.bluetooth_speaker, None);
        assert_eq!(c.librespot_dir(), PathBuf::from("/var/lib/muzak/librespot"));
        assert_eq!(c.cache_dir(), PathBuf::from("/var/lib/muzak/cache"));
        assert_eq!(c.images_dir(), PathBuf::from("/var/lib/muzak/images"));
    }

    #[test]
    fn rejects_volume_over_100() {
        let err = Config::parse("device_name = \"Leo\"\nstate_dir = \"/x\"\ninitial_volume = 101\n").unwrap_err();
        assert!(err.to_string().contains("initial_volume"), "{err}");
    }

    #[test]
    fn rejects_off_before_dim() {
        let err = Config::parse("device_name = \"Leo\"\nstate_dir = \"/x\"\ndim_after_secs = 600\noff_after_secs = 600\n").unwrap_err();
        assert!(err.to_string().contains("off_after_secs"), "{err}");
    }

    #[test]
    fn rejects_blank_name_and_unknown_keys() {
        assert!(Config::parse("device_name = \" \"\nstate_dir = \"/x\"\n").is_err());
        assert!(Config::parse("device_name = \"Leo\"\nstate_dir = \"/x\"\nvolume = 3\n").is_err());
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p muzak-player config`
Expected: compile error, `cannot find type Config`.

- [ ] **Step 5: Implement `Config`**

Put this above the `#[cfg(test)]` block in `config.rs`:
```rust
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use serde::Deserialize;

/// Device configuration, read from `/etc/muzak/config.toml` on the Pi.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Shown as the Spotify Connect device name.
    pub device_name: String,
    /// librespot audio backend: "alsa" on the Pi, "rodio" on the Mac.
    #[serde(default = "default_audio_backend")]
    pub audio_backend: String,
    /// ALSA device, e.g. "plughw:CARD=Headphones" or "bluealsa:DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp".
    #[serde(default)]
    pub audio_device: Option<String>,
    pub state_dir: PathBuf,
    #[serde(default = "default_dim_after_secs")]
    pub dim_after_secs: u64,
    #[serde(default = "default_off_after_secs")]
    pub off_after_secs: u64,
    #[serde(default = "default_initial_volume")]
    pub initial_volume: u8,
    /// Bluetooth speaker address to watch and reconnect.
    #[serde(default)]
    pub bluetooth_speaker: Option<String>,
}

fn default_audio_backend() -> String {
    if cfg!(target_os = "linux") { "alsa" } else { "rodio" }.to_string()
}
fn default_dim_after_secs() -> u64 {
    180
}
fn default_off_after_secs() -> u64 {
    600
}
fn default_initial_volume() -> u8 {
    50
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing config {}", path.display()))
    }

    pub fn parse(text: &str) -> anyhow::Result<Config> {
        let config: Config = toml::from_str(text)?;
        ensure!(!config.device_name.trim().is_empty(), "device_name must not be blank");
        ensure!(config.initial_volume <= 100, "initial_volume must be 0-100");
        ensure!(
            config.off_after_secs > config.dim_after_secs,
            "off_after_secs must be greater than dim_after_secs"
        );
        Ok(config)
    }

    pub fn librespot_dir(&self) -> PathBuf {
        self.state_dir.join("librespot")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.state_dir.join("cache")
    }

    pub fn images_dir(&self) -> PathBuf {
        self.state_dir.join("images")
    }
}
```

- [ ] **Step 6: Placeholder main**

`crates/muzak-player/src/main.rs`:
```rust
use muzak_player::AppWindow;
use slint::ComponentHandle;

fn main() -> anyhow::Result<()> {
    let window = AppWindow::new()?;
    window.run()?;
    Ok(())
}
```

- [ ] **Step 7: Run tests and the window**

Run: `asdf install && cargo test -p muzak-player config`
Expected: 4 tests pass.

Run: `cargo run -p muzak-player`
Expected: an 800×480 window with "muzak" in it. Close it.

- [ ] **Step 8: Commit**

```bash
git add .tool-versions .gitignore Cargo.toml Cargo.lock dev crates
git commit -m "feat: scaffold workspace, config, placeholder window"
```

---

### Task 2: Domain model and app core (navigation and library state)

**Files:**
- Create: `crates/muzak-player/src/model.rs`
- Create: `crates/muzak-player/src/app/mod.rs`, `app/input.rs`, `app/state.rs`, `app/tests.rs`
- Modify: `crates/muzak-player/src/lib.rs`

**Interfaces:**
- Produces:
  - **`model`:**
    - `Section::{Playlists, Albums, Liked, Recent}`, with `index() -> i32`, `from_index(i32) -> Section`, `title() -> &'static str`, `cache_key() -> &'static str`
    - `CollectionKind::{Playlist, Album, Liked}`
    - `Collection { uri, kind, name, subtitle, image_url: Option<String> }`
    - `Track { uri, name, artists, album, image_url: Option<String>, duration_ms: u32 }`
    - `Repeat::{Off, Context, Track}`, with `next()` and `index() -> i32`
    - `const LIKED_URI: &str = "muzak:liked"`
    - `fn liked_collection() -> Collection`
  - **`app::input`:**
    - `Input::{Ui(UiAction), Player(PlayerUpdate), Library(LibraryUpdate), Speaker { connected }, AuthInvalid, Tick}`
    - `UiAction`, `PlayerUpdate`, `LibraryUpdate`, `FailReason`
    - `Effect::{Player(PlayerCommand), Library(LibraryRequest), Display(DisplayMode)}`
    - `PlayerCommand`
    - `LibraryRequest::{Section(Section), Tracks { collection_uri }}` (`Hash + Eq`)
  - **`app::state`:** `AppState`, `Screen::{Grid(Section), Detail(String), NowPlaying}`, `Slot<T> { data: Option<Arc<T>>, loading, failed }`, `Playback`, `PlayStatus`, `Notice`, `DisplayMode`
  - **`app::Core`:**
    - `Core::new(CoreConfig, now_ms: u64) -> (Core, Vec<Effect>)`
    - `Core::state(&self) -> &AppState`
    - `Core::handle(&mut self, Input, now_ms: u64) -> Vec<Effect>`
    - `CoreConfig { dim_after_ms: u64, off_after_ms: u64, initial_volume: u8 }`

- [ ] **Step 1: Write `model.rs`**

```rust
use serde::{Deserialize, Serialize};

/// Pseudo-collection URI for the kid's Liked Songs. The player maps it to
/// `spotify:user:<username>:collection`, the library to `/me/tracks`.
pub const LIKED_URI: &str = "muzak:liked";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    Playlists,
    Albums,
    Liked,
    Recent,
}

impl Section {
    pub fn index(self) -> i32 {
        match self {
            Section::Playlists => 0,
            Section::Albums => 1,
            Section::Liked => 2,
            Section::Recent => 3,
        }
    }

    pub fn from_index(index: i32) -> Section {
        match index {
            1 => Section::Albums,
            2 => Section::Liked,
            3 => Section::Recent,
            _ => Section::Playlists,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Section::Playlists => "Playlists",
            Section::Albums => "Albums",
            Section::Liked => "Liked Songs",
            Section::Recent => "Recent",
        }
    }

    pub fn cache_key(self) -> &'static str {
        match self {
            Section::Playlists => "section-playlists",
            Section::Albums => "section-albums",
            Section::Liked => "section-liked",
            Section::Recent => "section-recent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectionKind {
    Playlist,
    Album,
    Liked,
}

/// Something you can open and play as a whole: a playlist, an album, or Liked Songs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub uri: String,
    pub kind: CollectionKind,
    pub name: String,
    /// Owner for playlists, artists for albums.
    pub subtitle: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub uri: String,
    pub name: String,
    /// Artist names joined with ", ".
    pub artists: String,
    pub album: String,
    pub image_url: Option<String>,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    Context,
    Track,
}

impl Repeat {
    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::Context,
            Repeat::Context => Repeat::Track,
            Repeat::Track => Repeat::Off,
        }
    }

    pub fn index(self) -> i32 {
        match self {
            Repeat::Off => 0,
            Repeat::Context => 1,
            Repeat::Track => 2,
        }
    }
}

pub fn liked_collection() -> Collection {
    Collection {
        uri: LIKED_URI.to_string(),
        kind: CollectionKind::Liked,
        name: Section::Liked.title().to_string(),
        subtitle: String::new(),
        image_url: None,
    }
}
```

- [ ] **Step 2: Write `app/input.rs`**

```rust
use crate::app::state::DisplayMode;
use crate::model::{Collection, Repeat, Section, Track};

/// Everything the core reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Ui(UiAction),
    Player(PlayerUpdate),
    Library(LibraryUpdate),
    Speaker { connected: bool },
    AuthInvalid,
    /// Sent once a second by the runtime.
    Tick,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    ShowSection(Section),
    OpenCollection(String),
    Back,
    OpenNowPlaying,
    PlayCollection { uri: String, shuffle: bool },
    PlayTrack { collection_uri: String, index: usize },
    TogglePlay,
    Next,
    Previous,
    Seek { position_ms: u32 },
    SetVolume { percent: u8 },
    ToggleShuffle,
    CycleRepeat,
    /// A tap on the dim/off overlay; only wakes the screen.
    Touch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerUpdate {
    Connected,
    Disconnected,
    TrackChanged(Track),
    Loading,
    Playing { position_ms: u32 },
    Paused { position_ms: u32 },
    Stopped,
    Position { position_ms: u32 },
    Volume { percent: u8 },
    Shuffle(bool),
    Repeat(Repeat),
    /// librespot skips unavailable tracks itself; this only drives a notice.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LibraryUpdate {
    Section { section: Section, items: Vec<Collection> },
    Tracks { collection_uri: String, tracks: Vec<Track> },
    SectionFailed { section: Section, reason: FailReason },
    TracksFailed { collection_uri: String, reason: FailReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    Offline,
    Auth,
    Other,
}

/// Work the runtime must carry out after a state change.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Player(PlayerCommand),
    Library(LibraryRequest),
    Display(DisplayMode),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    Load { context_uri: String, start_index: Option<u32>, shuffle: bool },
    Play,
    Pause,
    Next,
    Previous,
    Seek { position_ms: u32 },
    SetVolume { percent: u8 },
    SetShuffle(bool),
    SetRepeat(Repeat),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LibraryRequest {
    Section(Section),
    Tracks { collection_uri: String },
}
```

- [ ] **Step 3: Write `app/state.rs`**

```rust
use std::collections::HashMap;
use std::sync::Arc;

use crate::model::{Collection, Repeat, Section, Track};

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    Grid(Section),
    Detail(String),
    NowPlaying,
}

/// Library data plus its loading status. Data stays visible while a refresh runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot<T> {
    pub data: Option<Arc<T>>,
    pub loading: bool,
    /// True only when a fetch failed and there is no data to show.
    pub failed: bool,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot { data: None, loading: false, failed: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayStatus {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub track: Option<Track>,
    pub context_uri: Option<String>,
    pub status: PlayStatus,
    pub position_ms: u32,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    NoInternet,
    TrackUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Active,
    Dim,
    Off,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub screen: Screen,
    /// The rail item that is highlighted.
    pub section: Section,
    pub back_stack: Vec<Screen>,
    pub sections: HashMap<Section, Slot<Vec<Collection>>>,
    /// Track lists keyed by collection URI.
    pub tracks: HashMap<String, Slot<Vec<Track>>>,
    pub playback: Playback,
    pub notice: Option<Notice>,
    pub display: DisplayMode,
    pub online: bool,
    pub auth_needed: bool,
    pub speaker_connected: bool,
}
```

- [ ] **Step 4: Write the failing navigation and library tests**

`app/tests.rs`:
```rust
use std::sync::Arc;

use super::*;
use crate::model::{Collection, CollectionKind, LIKED_URI, Section, Track};

pub(crate) fn config() -> CoreConfig {
    CoreConfig { dim_after_ms: 60_000, off_after_ms: 120_000, initial_volume: 50 }
}

pub(crate) fn core() -> Core {
    Core::new(config(), 0).0
}

pub(crate) fn playlist(n: u32) -> Collection {
    Collection {
        uri: format!("spotify:playlist:p{n}"),
        kind: CollectionKind::Playlist,
        name: format!("Playlist {n}"),
        subtitle: "Mum".into(),
        image_url: Some(format!("https://i.scdn.co/image/p{n}")),
    }
}

pub(crate) fn track(n: u32) -> Track {
    Track {
        uri: format!("spotify:track:t{n}"),
        name: format!("Song {n}"),
        artists: "Band".into(),
        album: "Album".into(),
        image_url: None,
        duration_ms: 180_000,
    }
}

pub(crate) fn ui(action: UiAction) -> Input {
    Input::Ui(action)
}

pub(crate) fn with_tracks(core: &mut Core, uri: &str, n: u32) {
    core.handle(
        Input::Library(LibraryUpdate::Tracks {
            collection_uri: uri.into(),
            tracks: (0..n).map(track).collect(),
        }),
        0,
    );
}

#[test]
fn new_requests_initial_sections_and_shows_playlists() {
    let (core, effects) = Core::new(config(), 0);
    assert_eq!(core.state().screen, Screen::Grid(Section::Playlists));
    assert_eq!(
        effects,
        vec![
            Effect::Library(LibraryRequest::Section(Section::Playlists)),
            Effect::Library(LibraryRequest::Section(Section::Albums)),
            Effect::Library(LibraryRequest::Section(Section::Recent)),
            Effect::Display(DisplayMode::Active),
        ]
    );
    assert!(core.state().sections[&Section::Playlists].loading);
    assert_eq!(core.state().playback.volume, 50);
}

#[test]
fn show_section_switches_grid_and_clears_back_stack() {
    let mut c = core();
    c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
    let fx = c.handle(ui(UiAction::ShowSection(Section::Albums)), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Albums));
    assert_eq!(c.state().section, Section::Albums);
    assert!(c.state().back_stack.is_empty());
    assert_eq!(fx, vec![Effect::Library(LibraryRequest::Section(Section::Albums))]);
}

#[test]
fn show_liked_opens_liked_detail() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::ShowSection(Section::Liked)), 0);
    assert_eq!(c.state().screen, Screen::Detail(LIKED_URI.into()));
    assert_eq!(c.state().section, Section::Liked);
    assert_eq!(
        fx,
        vec![Effect::Library(LibraryRequest::Tracks { collection_uri: LIKED_URI.into() })]
    );
}

#[test]
fn open_collection_then_back() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
    assert_eq!(c.state().screen, Screen::Detail("spotify:playlist:p1".into()));
    assert!(c.state().tracks["spotify:playlist:p1"].loading);
    assert_eq!(
        fx,
        vec![Effect::Library(LibraryRequest::Tracks {
            collection_uri: "spotify:playlist:p1".into()
        })]
    );
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
}

#[test]
fn back_on_empty_stack_is_noop() {
    let mut c = core();
    let fx = c.handle(ui(UiAction::Back), 0);
    assert!(fx.is_empty());
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
}

#[test]
fn section_update_stores_data_and_marks_online() {
    let mut c = core();
    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Offline }), 0);
    assert!(!c.state().online);
    c.handle(
        Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }),
        0,
    );
    let slot = &c.state().sections[&Section::Playlists];
    assert_eq!(slot.data.as_deref(), Some(&vec![playlist(1)]));
    assert!(!slot.loading && !slot.failed);
    assert!(c.state().online);
}

#[test]
fn failure_keeps_cached_data_and_only_marks_failed_without_data() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }),
        0,
    );
    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Playlists, reason: FailReason::Other }), 0);
    let slot = &c.state().sections[&Section::Playlists];
    assert!(slot.data.is_some());
    assert!(!slot.failed);

    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Other }), 0);
    assert!(c.state().sections[&Section::Albums].failed);
}

#[test]
fn auth_failure_sets_auth_needed() {
    let mut c = core();
    c.handle(
        Input::Library(LibraryUpdate::TracksFailed { collection_uri: "x".into(), reason: FailReason::Auth }),
        0,
    );
    assert!(c.state().auth_needed);
    let mut c = core();
    c.handle(Input::AuthInvalid, 0);
    assert!(c.state().auth_needed);
}

#[test]
fn tracks_update_stores_data() {
    let mut c = core();
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    let slot = &c.state().tracks["spotify:playlist:p1"];
    assert_eq!(slot.data.as_ref().map(|t| t.len()), Some(3));
    assert!(Arc::strong_count(slot.data.as_ref().unwrap()) >= 1);
}
```

- [ ] **Step 5: Run tests to verify they fail**

Add to `lib.rs`:
```rust
pub mod app;
pub mod model;
```
Run: `cargo test -p muzak-player app::`
Expected: compile errors (`Core` not found).

- [ ] **Step 6: Implement `app/mod.rs`**

```rust
pub mod input;
pub mod state;
#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use input::*;
pub use state::*;

use crate::model::{LIKED_URI, Section};

/// How long a notice stays on screen.
const NOTICE_MS: u64 = 4_000;
/// Identical play requests closer together than this are treated as one.
const DOUBLE_TAP_MS: u64 = 1_000;

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub dim_after_ms: u64,
    pub off_after_ms: u64,
    pub initial_volume: u8,
}

/// The app's state machine. Pure: no I/O, time is passed in.
pub struct Core {
    state: AppState,
    cfg: CoreConfig,
    last_activity_ms: u64,
    notice_until_ms: u64,
    last_load: Option<((String, Option<u32>, bool), u64)>,
}

impl Core {
    pub fn new(cfg: CoreConfig, now_ms: u64) -> (Core, Vec<Effect>) {
        let state = AppState {
            screen: Screen::Grid(Section::Playlists),
            section: Section::Playlists,
            back_stack: Vec::new(),
            sections: Default::default(),
            tracks: Default::default(),
            playback: Playback {
                track: None,
                context_uri: None,
                status: PlayStatus::Stopped,
                position_ms: 0,
                volume: cfg.initial_volume.min(100),
                shuffle: false,
                repeat: Default::default(),
            },
            notice: None,
            display: DisplayMode::Active,
            online: true,
            auth_needed: false,
            speaker_connected: true,
        };
        let mut core = Core { state, cfg, last_activity_ms: now_ms, notice_until_ms: 0, last_load: None };
        let mut fx = Vec::new();
        for section in [Section::Playlists, Section::Albums, Section::Recent] {
            core.request_section(section, &mut fx);
        }
        fx.push(Effect::Display(DisplayMode::Active));
        (core, fx)
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn handle(&mut self, input: Input, now_ms: u64) -> Vec<Effect> {
        let mut fx = Vec::new();
        match input {
            Input::Ui(action) => {
                self.wake(now_ms, &mut fx);
                self.on_ui(action, now_ms, &mut fx);
            }
            Input::Player(update) => self.on_player(update, now_ms, &mut fx),
            Input::Library(update) => self.on_library(update),
            Input::Speaker { connected } => self.state.speaker_connected = connected,
            Input::AuthInvalid => self.state.auth_needed = true,
            Input::Tick => self.on_tick(now_ms, &mut fx),
        }
        fx
    }

    fn on_ui(&mut self, action: UiAction, _now_ms: u64, fx: &mut Vec<Effect>) {
        match action {
            UiAction::ShowSection(Section::Liked) => {
                self.state.section = Section::Liked;
                self.state.back_stack.clear();
                self.state.screen = Screen::Detail(LIKED_URI.to_string());
                self.request_tracks(LIKED_URI, fx);
            }
            UiAction::ShowSection(section) => {
                self.state.section = section;
                self.state.back_stack.clear();
                self.state.screen = Screen::Grid(section);
                self.request_section(section, fx);
            }
            UiAction::OpenCollection(uri) => {
                self.navigate(Screen::Detail(uri.clone()));
                self.request_tracks(&uri, fx);
            }
            UiAction::Back => {
                if let Some(previous) = self.state.back_stack.pop() {
                    self.state.screen = previous;
                }
            }
            // Playback actions are implemented in Task 3.
            _ => {}
        }
    }

    fn on_player(&mut self, _update: PlayerUpdate, _now_ms: u64, _fx: &mut Vec<Effect>) {
        // Implemented in Task 3.
    }

    fn on_library(&mut self, update: LibraryUpdate) {
        match update {
            LibraryUpdate::Section { section, items } => {
                let slot = self.state.sections.entry(section).or_default();
                slot.data = Some(Arc::new(items));
                slot.loading = false;
                slot.failed = false;
                self.state.online = true;
            }
            LibraryUpdate::Tracks { collection_uri, tracks } => {
                let slot = self.state.tracks.entry(collection_uri).or_default();
                slot.data = Some(Arc::new(tracks));
                slot.loading = false;
                slot.failed = false;
                self.state.online = true;
            }
            LibraryUpdate::SectionFailed { section, reason } => {
                let slot = self.state.sections.entry(section).or_default();
                slot.loading = false;
                slot.failed = slot.data.is_none();
                self.on_failure(reason);
            }
            LibraryUpdate::TracksFailed { collection_uri, reason } => {
                let slot = self.state.tracks.entry(collection_uri).or_default();
                slot.loading = false;
                slot.failed = slot.data.is_none();
                self.on_failure(reason);
            }
        }
    }

    fn on_failure(&mut self, reason: FailReason) {
        match reason {
            FailReason::Offline => self.state.online = false,
            FailReason::Auth => self.state.auth_needed = true,
            FailReason::Other => {}
        }
    }

    fn on_tick(&mut self, _now_ms: u64, _fx: &mut Vec<Effect>) {
        // Implemented in Task 3.
    }

    fn wake(&mut self, now_ms: u64, fx: &mut Vec<Effect>) {
        self.last_activity_ms = now_ms;
        if self.state.display != DisplayMode::Active {
            self.state.display = DisplayMode::Active;
            fx.push(Effect::Display(DisplayMode::Active));
        }
    }

    fn navigate(&mut self, screen: Screen) {
        if self.state.screen != screen {
            let previous = std::mem::replace(&mut self.state.screen, screen);
            self.state.back_stack.push(previous);
        }
    }

    fn request_section(&mut self, section: Section, fx: &mut Vec<Effect>) {
        self.state.sections.entry(section).or_default().loading = true;
        fx.push(Effect::Library(LibraryRequest::Section(section)));
    }

    fn request_tracks(&mut self, uri: &str, fx: &mut Vec<Effect>) {
        self.state.tracks.entry(uri.to_string()).or_default().loading = true;
        fx.push(Effect::Library(LibraryRequest::Tracks { collection_uri: uri.to_string() }));
    }
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p muzak-player app::`
Expected: 9 tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: add domain model and app core navigation"
```

---

### Task 3: App core: playback, notices, idle display, reconnect

**Files:**
- Modify: `crates/muzak-player/src/app/mod.rs` (`on_ui`, `on_player`, `on_tick`, new `start_playback`, `notify`, `refresh_after_reconnect`)
- Modify: `crates/muzak-player/src/app/tests.rs`

**Interfaces:**
- Consumes: everything from Task 2.
- Produces: the full behavior of `Core::handle`. No new public names.

- [ ] **Step 1: Write the failing tests**

Append to `app/tests.rs`:
```rust
fn load(uri: &str, start: Option<u32>, shuffle: bool) -> Effect {
    Effect::Player(PlayerCommand::Load { context_uri: uri.into(), start_index: start, shuffle })
}

#[test]
fn play_collection_is_optimistic() {
    let mut c = core();
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
    let fx = c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: false }), 0);
    assert_eq!(fx, vec![load("spotify:playlist:p1", Some(0), false)]);
    let pb = &c.state().playback;
    assert_eq!(pb.status, PlayStatus::Loading);
    assert_eq!(pb.track, Some(track(0)));
    assert_eq!(pb.context_uri.as_deref(), Some("spotify:playlist:p1"));
    assert_eq!(c.state().screen, Screen::NowPlaying);
    c.handle(ui(UiAction::Back), 0);
    assert_eq!(c.state().screen, Screen::Detail("spotify:playlist:p1".into()));
}

#[test]
fn play_collection_shuffled_has_no_known_first_track() {
    let mut c = core();
    with_tracks(&mut c, "spotify:playlist:p1", 3);
    let fx = c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: true }), 0);
    assert_eq!(fx, vec![load("spotify:playlist:p1", None, true)]);
    assert_eq!(c.state().playback.track, None);
    assert!(c.state().playback.shuffle);
}

// Review focus 1: double taps.
#[test]
fn double_tap_play_sends_one_load() {
    let mut c = core();
    let play = UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: false };
    let first = c.handle(ui(play.clone()), 10_000);
    let second = c.handle(ui(play.clone()), 10_400);
    assert_eq!(first, vec![load("spotify:playlist:p1", Some(0), false)]);
    assert!(second.is_empty());
    assert_eq!(c.state().back_stack.len(), 1, "no duplicate navigation");
    let third = c.handle(ui(play), 11_500);
    assert_eq!(third, vec![load("spotify:playlist:p1", Some(0), false)]);
}

#[test]
fn play_track_uses_index_and_current_shuffle() {
    let mut c = core();
    with_tracks(&mut c, "spotify:album:a1", 5);
    c.handle(Input::Player(PlayerUpdate::Shuffle(true)), 0);
    let fx = c.handle(ui(UiAction::PlayTrack { collection_uri: "spotify:album:a1".into(), index: 3 }), 0);
    assert_eq!(fx, vec![load("spotify:album:a1", Some(3), true)]);
    assert_eq!(c.state().playback.track, Some(track(3)));
}

#[test]
fn toggle_play_flips_status_optimistically() {
    let mut c = core();
    assert!(c.handle(ui(UiAction::TogglePlay), 0).is_empty(), "nothing loaded");
    c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: false }), 0);
    c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 0 }), 0);
    let fx = c.handle(ui(UiAction::TogglePlay), 0);
    assert_eq!(fx, vec![Effect::Player(PlayerCommand::Pause)]);
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
    let fx = c.handle(ui(UiAction::TogglePlay), 0);
    assert_eq!(fx, vec![Effect::Player(PlayerCommand::Play)]);
    assert_eq!(c.state().playback.status, PlayStatus::Playing);
}

#[test]
fn player_updates_change_playback() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(7))), 0);
    c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 1_000 }), 0);
    c.handle(Input::Player(PlayerUpdate::Position { position_ms: 5_000 }), 0);
    c.handle(Input::Player(PlayerUpdate::Volume { percent: 120 }), 0);
    c.handle(Input::Player(PlayerUpdate::Repeat(crate::model::Repeat::Track)), 0);
    let pb = &c.state().playback;
    assert_eq!(pb.track, Some(track(7)));
    assert_eq!(pb.status, PlayStatus::Playing);
    assert_eq!(pb.position_ms, 5_000);
    assert_eq!(pb.volume, 100);
    assert_eq!(pb.repeat, crate::model::Repeat::Track);
    c.handle(Input::Player(PlayerUpdate::Paused { position_ms: 6_000 }), 0);
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
    c.handle(Input::Player(PlayerUpdate::Stopped), 0);
    assert_eq!(c.state().playback.status, PlayStatus::Stopped);
}

#[test]
fn controls_emit_commands() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
    assert_eq!(c.handle(ui(UiAction::Next), 0), vec![Effect::Player(PlayerCommand::Next)]);
    assert_eq!(c.handle(ui(UiAction::Previous), 0), vec![Effect::Player(PlayerCommand::Previous)]);
    assert_eq!(
        c.handle(ui(UiAction::Seek { position_ms: 999_999 }), 0),
        vec![Effect::Player(PlayerCommand::Seek { position_ms: 180_000 })],
        "seek is clamped to the track length"
    );
    assert_eq!(
        c.handle(ui(UiAction::SetVolume { percent: 150 }), 0),
        vec![Effect::Player(PlayerCommand::SetVolume { percent: 100 })]
    );
    assert_eq!(c.handle(ui(UiAction::ToggleShuffle), 0), vec![Effect::Player(PlayerCommand::SetShuffle(true))]);
    assert_eq!(
        c.handle(ui(UiAction::CycleRepeat), 0),
        vec![Effect::Player(PlayerCommand::SetRepeat(crate::model::Repeat::Context))]
    );
}

#[test]
fn open_now_playing_needs_something_playing() {
    let mut c = core();
    c.handle(ui(UiAction::OpenNowPlaying), 0);
    assert_eq!(c.state().screen, Screen::Grid(Section::Playlists));
    c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
    c.handle(ui(UiAction::OpenNowPlaying), 0);
    assert_eq!(c.state().screen, Screen::NowPlaying);
}

#[test]
fn unavailable_shows_notice_that_expires() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Unavailable), 1_000);
    assert_eq!(c.state().notice, Some(Notice::TrackUnavailable));
    c.handle(Input::Tick, 3_000);
    assert_eq!(c.state().notice, Some(Notice::TrackUnavailable));
    c.handle(Input::Tick, 5_100);
    assert_eq!(c.state().notice, None);
}

#[test]
fn play_while_offline_shows_notice_but_still_loads() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
    let fx = c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: false }), 0);
    assert_eq!(fx, vec![load("spotify:playlist:p1", Some(0), false)]);
    assert_eq!(c.state().notice, Some(Notice::NoInternet));
}

#[test]
fn idle_dims_then_turns_off_and_touch_wakes() {
    let mut c = core();
    assert!(c.handle(Input::Tick, 59_000).is_empty());
    assert_eq!(c.handle(Input::Tick, 60_000), vec![Effect::Display(DisplayMode::Dim)]);
    assert_eq!(c.handle(Input::Tick, 120_000), vec![Effect::Display(DisplayMode::Off)]);
    assert_eq!(c.state().display, DisplayMode::Off);
    let fx = c.handle(ui(UiAction::Touch), 121_000);
    assert_eq!(fx, vec![Effect::Display(DisplayMode::Active)]);
    assert!(c.handle(Input::Tick, 122_000).is_empty());
}

#[test]
fn playing_keeps_screen_awake_and_wakes_it() {
    let mut c = core();
    c.handle(Input::Tick, 60_000);
    assert_eq!(c.state().display, DisplayMode::Dim);
    let fx = c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 0 }), 61_000);
    assert_eq!(fx, vec![Effect::Display(DisplayMode::Active)]);
    assert!(c.handle(Input::Tick, 500_000).is_empty());
    assert_eq!(c.state().display, DisplayMode::Active);
}

// Review focus 5: Wi-Fi down at boot.
#[test]
fn reconnect_rerequests_library_and_open_detail() {
    let mut c = core();
    c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Playlists, reason: FailReason::Offline }), 0);
    c.handle(ui(UiAction::OpenCollection("spotify:album:a1".into())), 0);
    let fx = c.handle(Input::Player(PlayerUpdate::Connected), 0);
    assert!(c.state().online);
    assert_eq!(
        fx,
        vec![
            Effect::Library(LibraryRequest::Section(Section::Playlists)),
            Effect::Library(LibraryRequest::Section(Section::Albums)),
            Effect::Library(LibraryRequest::Section(Section::Recent)),
            Effect::Library(LibraryRequest::Tracks { collection_uri: "spotify:album:a1".into() }),
        ]
    );
}

#[test]
fn disconnect_pauses_and_marks_offline() {
    let mut c = core();
    c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 0 }), 0);
    c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
    assert!(!c.state().online);
    assert_eq!(c.state().playback.status, PlayStatus::Paused);
}

#[test]
fn speaker_status_is_tracked() {
    let mut c = core();
    c.handle(Input::Speaker { connected: false }, 0);
    assert!(!c.state().speaker_connected);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muzak-player app::`
Expected: the new tests fail (no effects emitted, state unchanged).

- [ ] **Step 3: Implement playback, notices, idle and reconnect**

In `app/mod.rs`:

1. Replace the `// Playback actions are implemented in Task 3.` arm `_ => {}` in `on_ui` with:
```rust
            UiAction::OpenNowPlaying => {
                let pb = &self.state.playback;
                if pb.track.is_some() || pb.context_uri.is_some() {
                    self.navigate(Screen::NowPlaying);
                }
            }
            UiAction::PlayCollection { uri, shuffle } => {
                let start = if shuffle { None } else { Some(0) };
                self.start_playback(uri, start, shuffle, now_ms, fx);
            }
            UiAction::PlayTrack { collection_uri, index } => {
                let shuffle = self.state.playback.shuffle;
                self.start_playback(collection_uri, Some(index as u32), shuffle, now_ms, fx);
            }
            UiAction::TogglePlay => {
                let pb = &mut self.state.playback;
                let has_something = pb.track.is_some() || pb.context_uri.is_some();
                match pb.status {
                    PlayStatus::Playing | PlayStatus::Loading => {
                        pb.status = PlayStatus::Paused;
                        fx.push(Effect::Player(PlayerCommand::Pause));
                    }
                    PlayStatus::Paused | PlayStatus::Stopped if has_something => {
                        pb.status = PlayStatus::Playing;
                        fx.push(Effect::Player(PlayerCommand::Play));
                    }
                    _ => {}
                }
            }
            UiAction::Next => {
                self.state.playback.position_ms = 0;
                fx.push(Effect::Player(PlayerCommand::Next));
            }
            UiAction::Previous => {
                self.state.playback.position_ms = 0;
                fx.push(Effect::Player(PlayerCommand::Previous));
            }
            UiAction::Seek { position_ms } => {
                let max = self.state.playback.track.as_ref().map_or(u32::MAX, |t| t.duration_ms);
                let position_ms = position_ms.min(max);
                self.state.playback.position_ms = position_ms;
                fx.push(Effect::Player(PlayerCommand::Seek { position_ms }));
            }
            UiAction::SetVolume { percent } => {
                let percent = percent.min(100);
                self.state.playback.volume = percent;
                fx.push(Effect::Player(PlayerCommand::SetVolume { percent }));
            }
            UiAction::ToggleShuffle => {
                let shuffle = !self.state.playback.shuffle;
                self.state.playback.shuffle = shuffle;
                fx.push(Effect::Player(PlayerCommand::SetShuffle(shuffle)));
            }
            UiAction::CycleRepeat => {
                let repeat = self.state.playback.repeat.next();
                self.state.playback.repeat = repeat;
                fx.push(Effect::Player(PlayerCommand::SetRepeat(repeat)));
            }
            UiAction::Touch => {}
```
Rename the `_now_ms` parameter of `on_ui` to `now_ms`.

2. Replace `on_player` with:
```rust
    fn on_player(&mut self, update: PlayerUpdate, now_ms: u64, fx: &mut Vec<Effect>) {
        match update {
            PlayerUpdate::Connected => {
                self.state.online = true;
                self.refresh_after_reconnect(fx);
            }
            PlayerUpdate::Disconnected => {
                self.state.online = false;
                if self.state.playback.status == PlayStatus::Playing {
                    self.state.playback.status = PlayStatus::Paused;
                }
            }
            PlayerUpdate::TrackChanged(track) => {
                self.state.playback.track = Some(track);
                self.state.playback.position_ms = 0;
            }
            PlayerUpdate::Loading => self.state.playback.status = PlayStatus::Loading,
            PlayerUpdate::Playing { position_ms } => {
                self.state.playback.status = PlayStatus::Playing;
                self.state.playback.position_ms = position_ms;
                self.wake(now_ms, fx);
            }
            PlayerUpdate::Paused { position_ms } => {
                self.state.playback.status = PlayStatus::Paused;
                self.state.playback.position_ms = position_ms;
            }
            PlayerUpdate::Stopped => self.state.playback.status = PlayStatus::Stopped,
            PlayerUpdate::Position { position_ms } => self.state.playback.position_ms = position_ms,
            PlayerUpdate::Volume { percent } => self.state.playback.volume = percent.min(100),
            PlayerUpdate::Shuffle(shuffle) => self.state.playback.shuffle = shuffle,
            PlayerUpdate::Repeat(repeat) => self.state.playback.repeat = repeat,
            PlayerUpdate::Unavailable => self.notify(Notice::TrackUnavailable, now_ms),
        }
    }
```

3. Replace `on_tick` with:
```rust
    fn on_tick(&mut self, now_ms: u64, fx: &mut Vec<Effect>) {
        if self.state.notice.is_some() && now_ms >= self.notice_until_ms {
            self.state.notice = None;
        }
        if self.state.playback.status == PlayStatus::Playing {
            self.last_activity_ms = now_ms;
        }
        let idle = now_ms.saturating_sub(self.last_activity_ms);
        let target = if idle >= self.cfg.off_after_ms {
            DisplayMode::Off
        } else if idle >= self.cfg.dim_after_ms {
            DisplayMode::Dim
        } else {
            DisplayMode::Active
        };
        if target != self.state.display {
            self.state.display = target;
            fx.push(Effect::Display(target));
        }
    }
```

4. Add these methods to `impl Core`:
```rust
    fn start_playback(
        &mut self,
        uri: String,
        start_index: Option<u32>,
        shuffle: bool,
        now_ms: u64,
        fx: &mut Vec<Effect>,
    ) {
        let key = (uri.clone(), start_index, shuffle);
        if let Some((last_key, at)) = &self.last_load {
            if *last_key == key && now_ms.saturating_sub(*at) < DOUBLE_TAP_MS {
                return;
            }
        }
        self.last_load = Some((key, now_ms));
        if !self.state.online {
            self.notify(Notice::NoInternet, now_ms);
        }
        let first = start_index.and_then(|index| {
            self.state
                .tracks
                .get(&uri)
                .and_then(|slot| slot.data.as_ref())
                .and_then(|tracks| tracks.get(index as usize))
                .cloned()
        });
        let pb = &mut self.state.playback;
        pb.context_uri = Some(uri.clone());
        pb.track = first;
        pb.status = PlayStatus::Loading;
        pb.position_ms = 0;
        pb.shuffle = shuffle;
        fx.push(Effect::Player(PlayerCommand::Load { context_uri: uri, start_index, shuffle }));
        self.navigate(Screen::NowPlaying);
    }

    fn notify(&mut self, notice: Notice, now_ms: u64) {
        self.state.notice = Some(notice);
        self.notice_until_ms = now_ms + NOTICE_MS;
    }

    fn refresh_after_reconnect(&mut self, fx: &mut Vec<Effect>) {
        for section in [Section::Playlists, Section::Albums, Section::Recent] {
            self.request_section(section, fx);
        }
        let open_detail = std::iter::once(&self.state.screen)
            .chain(self.state.back_stack.iter().rev())
            .find_map(|screen| match screen {
                Screen::Detail(uri) => Some(uri.clone()),
                _ => None,
            });
        if let Some(uri) = open_detail {
            self.request_tracks(&uri, fx);
        }
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p muzak-player app::`
Expected: all 24 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/muzak-player/src/app
git commit -m "feat: core playback, notices, idle display and reconnect"
```

---

### Task 4: View model

**Files:**
- Create: `crates/muzak-player/src/view.rs`
- Modify: `crates/muzak-player/src/lib.rs` (add `pub mod view;`)

**Interfaces:**
- Consumes: `AppState` and friends (Task 2), `model::*`.
- Produces:
  - `view::build(&AppState) -> View`
  - `view::fmt_ms(u32) -> String`
  - `View { screen: ScreenView, section: Section, grid_title: String, grid: Vec<TileView>, grid_status: LoadStatus, detail: Option<DetailView>, now: NowView, mini_visible: bool, banner: Option<String>, display: DisplayMode, auth_needed: bool }`
  - `ScreenView::{Grid, Detail, NowPlaying}`
  - `LoadStatus::{Loading, Empty, Failed, Ready}`
  - `TileView { uri, title, subtitle, image_url: Option<String> }`
  - `DetailView { header: TileView, status: LoadStatus, tracks: Vec<TrackRowView> }`
  - `TrackRowView { title, artists, duration, current: bool }`
  - `NowView { title, artists, image_url, playing, loading, progress: f32, position: String, duration: String, volume: u8, shuffle: bool, repeat: Repeat }`

- [ ] **Step 1: Write the failing tests**

`view.rs`, starting with the test module only:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{core, playlist, track, ui, with_tracks};
    use crate::app::{Input, LibraryUpdate, FailReason, PlayerUpdate, UiAction};
    use crate::model::{LIKED_URI, Section};

    #[test]
    fn fmt_ms_formats_minutes_and_seconds() {
        assert_eq!(fmt_ms(0), "0:00");
        assert_eq!(fmt_ms(65_400), "1:05");
        assert_eq!(fmt_ms(3_600_000), "60:00");
    }

    // Review focus 2: an empty library.
    #[test]
    fn grid_status_covers_loading_empty_failed_ready() {
        let mut c = core();
        assert_eq!(build(c.state()).grid_status, LoadStatus::Loading);
        c.handle(Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![] }), 0);
        assert_eq!(build(c.state()).grid_status, LoadStatus::Empty);
        c.handle(Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }), 0);
        let v = build(c.state());
        assert_eq!(v.grid_status, LoadStatus::Ready);
        assert_eq!(v.grid[0].title, "Playlist 1");
        assert_eq!(v.grid[0].subtitle, "Mum");
        assert_eq!(v.grid_title, "Playlists");
        c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Other }), 0);
        c.handle(ui(UiAction::ShowSection(Section::Albums)), 0);
        c.handle(Input::Library(LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Other }), 0);
        assert_eq!(build(c.state()).grid_status, LoadStatus::Failed);
    }

    #[test]
    fn detail_shows_collection_and_marks_current_track() {
        let mut c = core();
        c.handle(Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }), 0);
        with_tracks(&mut c, "spotify:playlist:p1", 3);
        c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
        c.handle(Input::Player(PlayerUpdate::TrackChanged(track(1))), 0);
        let v = build(c.state());
        assert_eq!(v.screen, ScreenView::Detail);
        let d = v.detail.unwrap();
        assert_eq!(d.header.title, "Playlist 1");
        assert_eq!(d.status, LoadStatus::Ready);
        assert_eq!(d.tracks.len(), 3);
        assert_eq!(d.tracks[1].duration, "3:00");
        assert!(d.tracks[1].current && !d.tracks[0].current);
    }

    #[test]
    fn liked_detail_has_title_and_rail_highlights_liked() {
        let mut c = core();
        c.handle(ui(UiAction::ShowSection(Section::Liked)), 0);
        let v = build(c.state());
        assert_eq!(v.section, Section::Liked);
        let d = v.detail.unwrap();
        assert_eq!(d.header.uri, LIKED_URI);
        assert_eq!(d.header.title, "Liked Songs");
        assert_eq!(d.status, LoadStatus::Loading);
    }

    #[test]
    fn now_playing_progress_and_loading_fallback() {
        let mut c = core();
        c.handle(Input::Library(LibraryUpdate::Section { section: Section::Playlists, items: vec![playlist(1)] }), 0);
        c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: true }), 0);
        let v = build(c.state());
        assert_eq!(v.screen, ScreenView::NowPlaying);
        assert_eq!(v.now.title, "Playlist 1");
        assert!(v.now.loading);
        assert!(!v.mini_visible, "mini player hides on Now Playing");
        c.handle(Input::Player(PlayerUpdate::TrackChanged(track(2))), 0);
        c.handle(Input::Player(PlayerUpdate::Playing { position_ms: 45_000 }), 0);
        let v = build(c.state());
        assert_eq!(v.now.title, "Song 2");
        assert!((v.now.progress - 0.25).abs() < 0.001);
        assert_eq!(v.now.position, "0:45");
        assert_eq!(v.now.duration, "3:00");
        assert!(v.now.playing);
        c.handle(ui(UiAction::Back), 0);
        assert!(build(c.state()).mini_visible);
    }

    #[test]
    fn detail_survives_while_now_playing_is_on_top() {
        let mut c = core();
        with_tracks(&mut c, "spotify:playlist:p1", 2);
        c.handle(ui(UiAction::OpenCollection("spotify:playlist:p1".into())), 0);
        c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p1".into(), shuffle: false }), 0);
        let v = build(c.state());
        assert_eq!(v.detail.map(|d| d.header.uri), Some("spotify:playlist:p1".to_string()));
    }

    #[test]
    fn banner_prefers_notice_over_speaker_warning() {
        let mut c = core();
        c.handle(Input::Speaker { connected: false }, 0);
        assert_eq!(build(c.state()).banner.as_deref(), Some("Speaker not connected"));
        c.handle(Input::Player(PlayerUpdate::Unavailable), 0);
        assert_eq!(build(c.state()).banner.as_deref(), Some("That song can't play, skipping"));
        c.handle(Input::Player(PlayerUpdate::Disconnected), 0);
        c.handle(ui(UiAction::PlayCollection { uri: "spotify:playlist:p9".into(), shuffle: false }), 0);
        assert_eq!(build(c.state()).banner.as_deref(), Some("No internet right now"));
    }
}
```

`app/tests.rs` helpers are `pub(crate)`, so make the module visible to other test modules. In `app/mod.rs`, change `mod tests;` to `pub(crate) mod tests;` (it is still `#[cfg(test)]`).

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod view;` to `lib.rs`.
Run: `cargo test -p muzak-player view::`
Expected: compile errors (`build`, `fmt_ms` not found).

- [ ] **Step 3: Implement `view.rs`**

Put this above the test module:
```rust
//! Turns `AppState` into plain display data. No Slint types here, so it is unit-testable.

use std::sync::Arc;

use crate::app::{AppState, DisplayMode, Notice, PlayStatus, Screen, Slot};
use crate::model::{Collection, LIKED_URI, Repeat, Section, liked_collection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenView {
    Grid,
    Detail,
    NowPlaying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadStatus {
    Loading,
    Empty,
    Failed,
    Ready,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileView {
    pub uri: String,
    pub title: String,
    pub subtitle: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackRowView {
    pub title: String,
    pub artists: String,
    pub duration: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetailView {
    pub header: TileView,
    pub status: LoadStatus,
    pub tracks: Vec<TrackRowView>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NowView {
    pub title: String,
    pub artists: String,
    pub image_url: Option<String>,
    pub playing: bool,
    pub loading: bool,
    pub progress: f32,
    pub position: String,
    pub duration: String,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub screen: ScreenView,
    pub section: Section,
    pub grid_title: String,
    pub grid: Vec<TileView>,
    pub grid_status: LoadStatus,
    pub detail: Option<DetailView>,
    pub now: NowView,
    pub mini_visible: bool,
    pub banner: Option<String>,
    pub display: DisplayMode,
    pub auth_needed: bool,
}

pub fn fmt_ms(ms: u32) -> String {
    let secs = ms / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

pub fn build(state: &AppState) -> View {
    let screen = match state.screen {
        Screen::Grid(_) => ScreenView::Grid,
        Screen::Detail(_) => ScreenView::Detail,
        Screen::NowPlaying => ScreenView::NowPlaying,
    };
    let grid_section = match state.screen {
        Screen::Grid(section) => section,
        _ if state.section == Section::Liked => Section::Playlists,
        _ => state.section,
    };
    let grid_slot = state.sections.get(&grid_section);
    let grid = grid_slot
        .and_then(|slot| slot.data.as_ref())
        .map(|items| items.iter().map(tile).collect())
        .unwrap_or_default();
    let pb = &state.playback;
    let has_playback = pb.track.is_some() || pb.context_uri.is_some();

    View {
        screen,
        section: state.section,
        grid_title: grid_section.title().to_string(),
        grid,
        grid_status: status(grid_slot),
        detail: detail(state),
        now: now(state),
        mini_visible: has_playback && screen != ScreenView::NowPlaying,
        banner: banner(state),
        display: state.display,
        auth_needed: state.auth_needed,
    }
}

fn tile(c: &Collection) -> TileView {
    TileView {
        uri: c.uri.clone(),
        title: c.name.clone(),
        subtitle: c.subtitle.clone(),
        image_url: c.image_url.clone(),
    }
}

fn status<T>(slot: Option<&Slot<Vec<T>>>) -> LoadStatus {
    match slot {
        Some(Slot { data: Some(data), .. }) if data.is_empty() => LoadStatus::Empty,
        Some(Slot { data: Some(_), .. }) => LoadStatus::Ready,
        Some(Slot { failed: true, .. }) => LoadStatus::Failed,
        _ => LoadStatus::Loading,
    }
}

fn find_collection(state: &AppState, uri: &str) -> Option<Collection> {
    if uri == LIKED_URI {
        return Some(liked_collection());
    }
    state
        .sections
        .values()
        .filter_map(|slot| slot.data.as_ref())
        .flat_map(|items: &Arc<Vec<Collection>>| items.iter())
        .find(|c| c.uri == uri)
        .cloned()
}

fn detail(state: &AppState) -> Option<DetailView> {
    let uri = std::iter::once(&state.screen)
        .chain(state.back_stack.iter().rev())
        .find_map(|screen| match screen {
            Screen::Detail(uri) => Some(uri.clone()),
            _ => None,
        })?;
    let header = find_collection(state, &uri).map(|c| tile(&c)).unwrap_or(TileView {
        uri: uri.clone(),
        title: String::new(),
        subtitle: String::new(),
        image_url: None,
    });
    let slot = state.tracks.get(&uri);
    let current_uri = state.playback.track.as_ref().map(|t| t.uri.as_str());
    let tracks = slot
        .and_then(|s| s.data.as_ref())
        .map(|tracks| {
            tracks
                .iter()
                .map(|t| TrackRowView {
                    title: t.name.clone(),
                    artists: t.artists.clone(),
                    duration: fmt_ms(t.duration_ms),
                    current: Some(t.uri.as_str()) == current_uri,
                })
                .collect()
        })
        .unwrap_or_default();
    Some(DetailView { header, status: status(slot), tracks })
}

fn now(state: &AppState) -> NowView {
    let pb = &state.playback;
    let (title, artists, image_url) = match &pb.track {
        Some(t) => (t.name.clone(), t.artists.clone(), t.image_url.clone()),
        None => match pb.context_uri.as_deref().and_then(|uri| find_collection(state, uri)) {
            Some(c) => (c.name, String::new(), c.image_url),
            None => (String::new(), String::new(), None),
        },
    };
    let duration_ms = pb.track.as_ref().map_or(0, |t| t.duration_ms);
    let progress = if duration_ms > 0 {
        (pb.position_ms as f32 / duration_ms as f32).min(1.0)
    } else {
        0.0
    };
    NowView {
        title,
        artists,
        image_url,
        playing: pb.status == PlayStatus::Playing,
        loading: pb.status == PlayStatus::Loading,
        progress,
        position: fmt_ms(pb.position_ms),
        duration: fmt_ms(duration_ms),
        volume: pb.volume,
        shuffle: pb.shuffle,
        repeat: pb.repeat,
    }
}

fn banner(state: &AppState) -> Option<String> {
    match state.notice {
        Some(Notice::NoInternet) => Some("No internet right now".into()),
        Some(Notice::TrackUnavailable) => Some("That song can't play, skipping".into()),
        None if !state.speaker_connected => Some("Speaker not connected".into()),
        None => None,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p muzak-player`
Expected: all tests pass (config, app, view).

- [ ] **Step 5: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: add view model"
```

---

### Task 5: Spotify Web API client

**Files:**
- Create: `crates/muzak-player/src/library/mod.rs`, `crates/muzak-player/src/library/web_api.rs`
- Modify: `crates/muzak-player/src/lib.rs` (add `pub mod library;`)

**Interfaces:**
- Consumes: `model::*`, `app::FailReason`.
- Produces:
  - **`library` module:**
    - `trait LibrarySource: Send + Sync + 'static`, with:
      - `fn section(&self, Section) -> impl Future<Output = Result<Vec<Collection>, FetchError>> + Send`
      - `fn tracks(&self, collection_uri: &str) -> impl Future<Output = Result<Vec<Track>, FetchError>> + Send`
    - `FetchError::{Offline, Auth, NotFound, Other(String)}`, with `reason() -> FailReason`
  - **`web_api` module:**
    - `trait Http: Send + Sync + 'static`, with `fn get_json(&self, url: &str, token: &str) -> impl Future<Output = Result<serde_json::Value, HttpError>> + Send`
    - `HttpError::{Status(u16), Network(String), Decode(String)}`
    - `trait TokenSource: Send + Sync + 'static`, with `fn token(&self) -> impl Future<Output = Result<String, FetchError>> + Send`
    - `WebApi<H: Http, T: TokenSource>`, with `new(http, tokens)` and `with_base(http, tokens, base)`. It implements `LibrarySource`.
    - `ReqwestHttp::new() -> anyhow::Result<ReqwestHttp>`
    - `const SCOPES: &str`

- [ ] **Step 1: Write `library/mod.rs`**

```rust
pub mod web_api;

use std::future::Future;

use crate::app::FailReason;
use crate::model::{Collection, Section, Track};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FetchError {
    #[error("offline")]
    Offline,
    #[error("not signed in")]
    Auth,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Other(String),
}

impl FetchError {
    pub fn reason(&self) -> FailReason {
        match self {
            FetchError::Offline => FailReason::Offline,
            FetchError::Auth => FailReason::Auth,
            FetchError::NotFound | FetchError::Other(_) => FailReason::Other,
        }
    }
}

/// Where library data comes from: the Spotify Web API, or the fake catalog.
pub trait LibrarySource: Send + Sync + 'static {
    fn section(&self, section: Section) -> impl Future<Output = Result<Vec<Collection>, FetchError>> + Send;
    fn tracks(&self, collection_uri: &str) -> impl Future<Output = Result<Vec<Track>, FetchError>> + Send;
}
```

- [ ] **Step 2: Write the failing tests**

`library/web_api.rs`, test module first:
```rust
#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use serde_json::json;

    use super::*;

    const BASE: &str = "https://api.test/v1";

    #[derive(Default)]
    struct FakeHttp {
        responses: Mutex<HashMap<String, VecDeque<Result<Value, HttpError>>>>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl FakeHttp {
        fn on(self, path: &str, response: Result<Value, HttpError>) -> Self {
            let url = if path.starts_with("http") { path.to_string() } else { format!("{BASE}{path}") };
            self.responses.lock().unwrap().entry(url).or_default().push_back(response);
            self
        }
    }

    impl Http for FakeHttp {
        async fn get_json(&self, url: &str, token: &str) -> Result<Value, HttpError> {
            self.calls.lock().unwrap().push((url.to_string(), token.to_string()));
            self.responses
                .lock()
                .unwrap()
                .get_mut(url)
                .and_then(|queue| queue.pop_front())
                .unwrap_or(Err(HttpError::Status(404)))
        }
    }

    #[derive(Default)]
    struct CountingTokens(AtomicU32);

    impl TokenSource for CountingTokens {
        async fn token(&self) -> Result<String, FetchError> {
            Ok(format!("t{}", self.0.fetch_add(1, Ordering::SeqCst) + 1))
        }
    }

    fn api(http: FakeHttp) -> WebApi<FakeHttp, CountingTokens> {
        WebApi::with_base(http, CountingTokens::default(), BASE)
    }

    fn img(url: &str, width: Option<u32>) -> ImageObj {
        ImageObj { url: url.into(), width }
    }

    #[test]
    fn pick_image_prefers_smallest_at_least_250() {
        let images = [img("big", Some(640)), img("mid", Some(300)), img("small", Some(64))];
        assert_eq!(pick_image(&images).as_deref(), Some("mid"));
        assert_eq!(pick_image(&[img("only", None)]).as_deref(), Some("only"));
        assert_eq!(pick_image(&[img("tiny", Some(64))]).as_deref(), Some("tiny"));
        assert_eq!(pick_image(&[]), None);
    }

    #[tokio::test]
    async fn playlists_follow_pagination_and_tolerate_null_images() {
        let http = FakeHttp::default()
            .on(
                "/me/playlists?limit=50",
                Ok(json!({
                    "items": [{"uri": "spotify:playlist:a", "name": "A", "images": null, "owner": {"display_name": "Mum"}}],
                    "next": "https://api.test/v1/me/playlists?offset=1"
                })),
            )
            .on(
                "https://api.test/v1/me/playlists?offset=1",
                Ok(json!({
                    "items": [null, {"uri": "spotify:playlist:b", "name": "B", "images": [{"url": "u", "width": 300}], "owner": null}],
                    "next": null
                })),
            );
        let result = api(http).section(Section::Playlists).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].subtitle, "Mum");
        assert_eq!(result[0].image_url, None);
        assert_eq!(result[1].image_url.as_deref(), Some("u"));
        assert_eq!(result[1].kind, CollectionKind::Playlist);
    }

    // Review focus 3: odd playlist entries.
    #[tokio::test]
    async fn playlist_tracks_skip_null_local_and_episodes() {
        let http = FakeHttp::default().on(
            "/playlists/p1/items?limit=100",
            Ok(json!({
                "items": [
                    {"track": null},
                    {"track": {"uri": "spotify:local:x", "name": "Local", "is_local": true, "duration_ms": 1, "artists": []}},
                    {"item": {"uri": "spotify:episode:e", "name": "Podcast", "duration_ms": 1}},
                    {"item": {"uri": "spotify:track:t1", "name": "Song", "duration_ms": 61000,
                              "artists": [{"name": "A"}, {"name": "B"}],
                              "album": {"uri": "spotify:album:al", "name": "Al", "images": [{"url": "cover", "width": 300}]}}}
                ],
                "next": null
            })),
        );
        let tracks = api(http).tracks("spotify:playlist:p1").await.unwrap();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].artists, "A, B");
        assert_eq!(tracks[0].album, "Al");
        assert_eq!(tracks[0].image_url.as_deref(), Some("cover"));
    }

    #[tokio::test]
    async fn playlist_tracks_fall_back_to_tracks_endpoint() {
        let http = FakeHttp::default().on(
            "/playlists/p1/tracks?limit=100",
            Ok(json!({"items": [{"track": {"uri": "spotify:track:t1", "name": "Song", "duration_ms": 1, "artists": []}}], "next": null})),
        );
        let tracks = api(http).tracks("spotify:playlist:p1").await.unwrap();
        assert_eq!(tracks.len(), 1);
    }

    #[tokio::test]
    async fn albums_and_album_tracks() {
        let http = FakeHttp::default()
            .on(
                "/me/albums?limit=50",
                Ok(json!({"items": [{"album": {"uri": "spotify:album:a1", "name": "Moana",
                    "images": [{"url": "m", "width": 300}], "artists": [{"name": "Various"}]}}], "next": null})),
            )
            .on(
                "/albums/a1",
                Ok(json!({"uri": "spotify:album:a1", "name": "Moana", "images": [{"url": "m", "width": 300}],
                    "artists": [], "tracks": {"items": [{"uri": "spotify:track:t1", "name": "How Far", "duration_ms": 2, "artists": [{"name": "Auli'i"}]}],
                    "next": "https://api.test/v1/albums/a1/tracks?offset=1"}})),
            )
            .on(
                "https://api.test/v1/albums/a1/tracks?offset=1",
                Ok(json!({"items": [{"uri": "spotify:track:t2", "name": "Shiny", "duration_ms": 3, "artists": []}], "next": null})),
            );
        let api = api(http);
        let albums = api.section(Section::Albums).await.unwrap();
        assert_eq!(albums[0].subtitle, "Various");
        let tracks = api.tracks("spotify:album:a1").await.unwrap();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].album, "Moana");
        assert_eq!(tracks[1].image_url.as_deref(), Some("m"));
    }

    #[tokio::test]
    async fn liked_tracks_use_me_tracks() {
        let http = FakeHttp::default().on(
            "/me/tracks?limit=50",
            Ok(json!({"items": [{"track": {"uri": "spotify:track:t1", "name": "S", "duration_ms": 1, "artists": []}}], "next": null})),
        );
        assert_eq!(api(http).tracks(LIKED_URI).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recent_dedupes_contexts_and_maps_liked() {
        let album = json!({"uri": "spotify:album:a1", "name": "Al", "images": [], "artists": [{"name": "X"}]});
        let t = |ctx: Value| json!({"track": {"uri": "spotify:track:t", "name": "S", "duration_ms": 1, "artists": [], "album": album}, "context": ctx});
        let http = FakeHttp::default()
            .on(
                "/me/playlists?limit=50",
                Ok(json!({"items": [{"uri": "spotify:playlist:p1", "name": "Mine", "images": [], "owner": null}], "next": null})),
            )
            .on(
                "/me/player/recently-played?limit=50",
                Ok(json!({"items": [
                    t(json!({"type": "playlist", "uri": "spotify:playlist:p1"})),
                    t(json!({"type": "playlist", "uri": "spotify:playlist:p1"})),
                    t(json!({"type": "playlist", "uri": "spotify:playlist:unknown"})),
                    t(json!({"type": "collection", "uri": "spotify:user:kid:collection"})),
                    t(json!({"type": "album", "uri": "spotify:album:a1"})),
                    t(Value::Null)
                ], "next": null})),
            );
        let recent = api(http).section(Section::Recent).await.unwrap();
        let uris: Vec<_> = recent.iter().map(|c| c.uri.as_str()).collect();
        assert_eq!(uris, vec!["spotify:playlist:p1", LIKED_URI, "spotify:album:a1"]);
    }

    // Review focus 4: expired access tokens.
    #[tokio::test]
    async fn unauthorized_retries_once_with_fresh_token() {
        let http = FakeHttp::default()
            .on("/me/tracks?limit=50", Err(HttpError::Status(401)))
            .on("/me/tracks?limit=50", Ok(json!({"items": [], "next": null})));
        let api = api(http);
        assert_eq!(api.tracks(LIKED_URI).await.unwrap(), vec![]);
        let calls = api.http.calls.lock().unwrap().clone();
        assert_eq!(calls.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), vec!["t1", "t2"]);
    }

    #[tokio::test]
    async fn repeated_unauthorized_is_auth_error_and_network_is_offline() {
        let http = FakeHttp::default()
            .on("/me/tracks?limit=50", Err(HttpError::Status(401)))
            .on("/me/tracks?limit=50", Err(HttpError::Status(401)))
            .on("/me/albums?limit=50", Err(HttpError::Network("dns".into())));
        let api = api(http);
        assert_eq!(api.tracks(LIKED_URI).await, Err(FetchError::Auth));
        assert_eq!(api.section(Section::Albums).await, Err(FetchError::Offline));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Add `pub mod library;` to `lib.rs`.
Run: `cargo test -p muzak-player web_api`
Expected: compile errors.

- [ ] **Step 4: Implement `web_api.rs`**

Put this above the test module:
```rust
//! Spotify Web API client: only the endpoints the player needs, parsed defensively.

use std::collections::HashSet;
use std::future::Future;
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{FetchError, LibrarySource};
use crate::model::{Collection, CollectionKind, LIKED_URI, Section, Track, liked_collection};

pub const API_BASE: &str = "https://api.spotify.com/v1";
/// Keep in sync with `crates/muzak-setup/src/main.rs`.
pub const SCOPES: &str =
    "playlist-read-private,playlist-read-collaborative,user-library-read,user-read-recently-played";
const MAX_ITEMS: usize = 500;
const RECENT_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum HttpError {
    #[error("HTTP status {0}")]
    Status(u16),
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid response: {0}")]
    Decode(String),
}

pub trait Http: Send + Sync + 'static {
    fn get_json(&self, url: &str, token: &str) -> impl Future<Output = Result<Value, HttpError>> + Send;
}

pub trait TokenSource: Send + Sync + 'static {
    fn token(&self) -> impl Future<Output = Result<String, FetchError>> + Send;
}

pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self { client: reqwest::Client::builder().timeout(Duration::from_secs(15)).build()? })
    }
}

impl Http for ReqwestHttp {
    async fn get_json(&self, url: &str, token: &str) -> Result<Value, HttpError> {
        let response = self
            .client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(HttpError::Status(status.as_u16()));
        }
        response.json::<Value>().await.map_err(|e| HttpError::Decode(e.to_string()))
    }
}

// ---- Response shapes (only the fields we use; everything optional where Spotify is inconsistent) ----

#[derive(Debug, Deserialize)]
struct Page<T> {
    #[serde(default = "Vec::new")]
    items: Vec<Option<T>>,
    next: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ImageObj {
    pub url: String,
    pub width: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OwnerObj {
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistObj {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PlaylistObj {
    uri: String,
    name: String,
    #[serde(default)]
    images: Option<Vec<ImageObj>>,
    owner: Option<OwnerObj>,
}

#[derive(Debug, Deserialize)]
struct AlbumObj {
    uri: String,
    name: String,
    #[serde(default)]
    images: Option<Vec<ImageObj>>,
    #[serde(default)]
    artists: Vec<ArtistObj>,
    tracks: Option<Page<TrackObj>>,
}

#[derive(Debug, Deserialize)]
struct AlbumRef {
    uri: Option<String>,
    name: Option<String>,
    #[serde(default)]
    images: Option<Vec<ImageObj>>,
    #[serde(default)]
    artists: Vec<ArtistObj>,
}

#[derive(Debug, Deserialize)]
struct TrackObj {
    uri: Option<String>,
    name: Option<String>,
    #[serde(default)]
    duration_ms: u32,
    #[serde(default)]
    artists: Vec<ArtistObj>,
    album: Option<AlbumRef>,
    #[serde(default)]
    is_local: bool,
}

#[derive(Debug, Deserialize)]
struct SavedAlbum {
    album: AlbumObj,
}

#[derive(Debug, Deserialize)]
struct SavedTrack {
    track: Option<TrackObj>,
}

/// Spotify has used both `track` and `item` for playlist entries.
#[derive(Debug, Deserialize)]
struct PlaylistItem {
    track: Option<TrackObj>,
    item: Option<TrackObj>,
}

#[derive(Debug, Deserialize)]
struct RecentItem {
    track: TrackObj,
    context: Option<ContextObj>,
}

#[derive(Debug, Deserialize)]
struct ContextObj {
    uri: String,
    #[serde(rename = "type")]
    kind: String,
}

// ---- Conversions ----

fn images(images: &Option<Vec<ImageObj>>) -> &[ImageObj] {
    images.as_deref().unwrap_or(&[])
}

/// Smallest image that is at least 250px wide, else the first one listed.
pub(crate) fn pick_image(images: &[ImageObj]) -> Option<String> {
    images
        .iter()
        .filter(|i| i.width.is_some_and(|w| w >= 250))
        .min_by_key(|i| i.width)
        .or_else(|| images.first())
        .map(|i| i.url.clone())
}

fn join_artists(artists: &[ArtistObj]) -> String {
    artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn playlist_collection(p: PlaylistObj) -> Collection {
    Collection {
        image_url: pick_image(images(&p.images)),
        uri: p.uri,
        kind: CollectionKind::Playlist,
        name: p.name,
        subtitle: p.owner.and_then(|o| o.display_name).unwrap_or_default(),
    }
}

fn album_collection(a: &AlbumObj) -> Collection {
    Collection {
        uri: a.uri.clone(),
        kind: CollectionKind::Album,
        name: a.name.clone(),
        subtitle: join_artists(&a.artists),
        image_url: pick_image(images(&a.images)),
    }
}

/// Only real Spotify tracks; local files and podcast episodes are skipped.
fn to_track(t: TrackObj, album_fallback: Option<(&str, Option<&str>)>) -> Option<Track> {
    let uri = t.uri?;
    if t.is_local || !uri.starts_with("spotify:track:") {
        return None;
    }
    let (album, image_url) = match &t.album {
        Some(a) => (a.name.clone().unwrap_or_default(), pick_image(images(&a.images))),
        None => album_fallback
            .map(|(name, image)| (name.to_string(), image.map(str::to_string)))
            .unwrap_or_default(),
    };
    Some(Track {
        uri,
        name: t.name.unwrap_or_default(),
        artists: join_artists(&t.artists),
        album,
        image_url,
        duration_ms: t.duration_ms,
    })
}

fn recent_collections(items: Vec<RecentItem>, known_playlists: &[Collection]) -> Vec<Collection> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let collection = match &item.context {
            Some(ctx) if ctx.kind == "playlist" => known_playlists.iter().find(|p| p.uri == ctx.uri).cloned(),
            Some(ctx) if ctx.kind == "collection" => Some(liked_collection()),
            _ => item.track.album.as_ref().and_then(|a| {
                Some(Collection {
                    uri: a.uri.clone()?,
                    kind: CollectionKind::Album,
                    name: a.name.clone().unwrap_or_default(),
                    subtitle: join_artists(&a.artists),
                    image_url: pick_image(images(&a.images)),
                })
            }),
        };
        if let Some(c) = collection {
            if seen.insert(c.uri.clone()) {
                out.push(c);
                if out.len() >= RECENT_LIMIT {
                    break;
                }
            }
        }
    }
    out
}

fn decode<D: DeserializeOwned>(value: Value) -> Result<D, FetchError> {
    serde_json::from_value(value).map_err(|e| FetchError::Other(format!("unexpected response: {e}")))
}

// ---- Client ----

pub struct WebApi<H, T> {
    http: H,
    tokens: T,
    base: String,
}

impl<H: Http, T: TokenSource> WebApi<H, T> {
    pub fn new(http: H, tokens: T) -> Self {
        Self::with_base(http, tokens, API_BASE)
    }

    pub fn with_base(http: H, tokens: T, base: &str) -> Self {
        Self { http, tokens, base: base.to_string() }
    }

    async fn get(&self, path_or_url: &str) -> Result<Value, FetchError> {
        let url = if path_or_url.starts_with("http") {
            path_or_url.to_string()
        } else {
            format!("{}{}", self.base, path_or_url)
        };
        let mut retried = false;
        loop {
            let token = self.tokens.token().await?;
            match self.http.get_json(&url, &token).await {
                Ok(value) => return Ok(value),
                Err(HttpError::Status(401)) if !retried => retried = true,
                Err(HttpError::Status(401)) => return Err(FetchError::Auth),
                Err(HttpError::Status(404)) => return Err(FetchError::NotFound),
                Err(HttpError::Status(code)) => return Err(FetchError::Other(format!("HTTP {code} for {url}"))),
                Err(HttpError::Network(e)) => {
                    tracing::warn!("network error for {url}: {e}");
                    return Err(FetchError::Offline);
                }
                Err(HttpError::Decode(e)) => return Err(FetchError::Other(e)),
            }
        }
    }

    async fn pages<I: DeserializeOwned>(&self, first: &str) -> Result<Vec<I>, FetchError> {
        let mut out = Vec::new();
        let mut next = Some(first.to_string());
        while let Some(url) = next.take() {
            let page: Page<I> = decode(self.get(&url).await?)?;
            out.extend(page.items.into_iter().flatten());
            if out.len() < MAX_ITEMS {
                next = page.next;
            }
        }
        out.truncate(MAX_ITEMS);
        Ok(out)
    }

    async fn playlists(&self) -> Result<Vec<Collection>, FetchError> {
        let items = self.pages::<PlaylistObj>("/me/playlists?limit=50").await?;
        Ok(items.into_iter().map(playlist_collection).collect())
    }

    async fn albums(&self) -> Result<Vec<Collection>, FetchError> {
        let items = self.pages::<SavedAlbum>("/me/albums?limit=50").await?;
        Ok(items.iter().map(|s| album_collection(&s.album)).collect())
    }

    async fn recent(&self) -> Result<Vec<Collection>, FetchError> {
        let known = self.playlists().await?;
        // Recently played pages use cursors; the first page (50 plays) is plenty.
        let page: Page<RecentItem> = decode(self.get("/me/player/recently-played?limit=50").await?)?;
        Ok(recent_collections(page.items.into_iter().flatten().collect(), &known))
    }

    async fn liked_tracks(&self) -> Result<Vec<Track>, FetchError> {
        let items = self.pages::<SavedTrack>("/me/tracks?limit=50").await?;
        Ok(items.into_iter().filter_map(|s| s.track.and_then(|t| to_track(t, None))).collect())
    }

    async fn playlist_tracks(&self, id: &str) -> Result<Vec<Track>, FetchError> {
        let items = match self.pages::<PlaylistItem>(&format!("/playlists/{id}/items?limit=100")).await {
            Err(FetchError::NotFound) => self.pages::<PlaylistItem>(&format!("/playlists/{id}/tracks?limit=100")).await?,
            other => other?,
        };
        Ok(items
            .into_iter()
            .filter_map(|i| i.item.or(i.track))
            .filter_map(|t| to_track(t, None))
            .collect())
    }

    async fn album_tracks(&self, id: &str) -> Result<Vec<Track>, FetchError> {
        let album: AlbumObj = decode(self.get(&format!("/albums/{id}")).await?)?;
        let image = pick_image(images(&album.images));
        let mut raw = Vec::new();
        let mut next = None;
        if let Some(page) = album.tracks {
            raw.extend(page.items.into_iter().flatten());
            next = page.next;
        }
        if let Some(url) = next {
            raw.extend(self.pages::<TrackObj>(&url).await?);
        }
        Ok(raw
            .into_iter()
            .filter_map(|t| to_track(t, Some((&album.name, image.as_deref()))))
            .collect())
    }
}

impl<H: Http, T: TokenSource> LibrarySource for WebApi<H, T> {
    async fn section(&self, section: Section) -> Result<Vec<Collection>, FetchError> {
        match section {
            Section::Playlists => self.playlists().await,
            Section::Albums => self.albums().await,
            Section::Recent => self.recent().await,
            Section::Liked => Ok(vec![liked_collection()]),
        }
    }

    async fn tracks(&self, collection_uri: &str) -> Result<Vec<Track>, FetchError> {
        if collection_uri == LIKED_URI {
            return self.liked_tracks().await;
        }
        let parts: Vec<&str> = collection_uri.split(':').collect();
        match parts.as_slice() {
            ["spotify", "playlist", id] => self.playlist_tracks(id).await,
            ["spotify", "album", id] => self.album_tracks(id).await,
            _ => Err(FetchError::Other(format!("unsupported collection {collection_uri}"))),
        }
    }
}
```

The retry test reads `api.http`. That works because the test module is a child of this module, which can read private fields.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p muzak-player web_api`
Expected: 9 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: add Spotify Web API client"
```

---

### Task 6: Disk cache, library service, fake catalog

**Files:**
- Create: `crates/muzak-player/src/library/cache.rs`, `library/service.rs`, `library/fake.rs`
- Modify: `crates/muzak-player/src/library/mod.rs` (add `pub mod cache; pub mod fake; pub mod service;`)

**Interfaces:**
- Consumes: `LibrarySource`, `FetchError` (Task 5); `Input`, `LibraryUpdate`, `LibraryRequest` (Task 2).
- Produces:
  - **`cache`:**
    - `DiskCache::new(dir) -> io::Result<DiskCache>`
    - `.read::<T>(key) -> Option<T>`
    - `.write(key, &T) -> io::Result<()>`
    - `.path_for(key) -> PathBuf`
    - `fn tracks_key(uri: &str) -> String`
  - **`service`:** `spawn_library<S: LibrarySource>(Arc<S>, Arc<DiskCache>, UnboundedSender<Input>) -> UnboundedSender<LibraryRequest>`
  - **`fake`:**
    - `FakeCatalog::sample()`, with pub fields `playlists`, `albums` and `.tracks_for(uri) -> Vec<Track>`
    - `FakeSource::new(Arc<FakeCatalog>)`, which implements `LibrarySource`

- [ ] **Step 1: Write the failing cache and service tests**

`library/cache.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_missing_and_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(dir.path().join("c")).unwrap();
        assert_eq!(cache.read::<Vec<u32>>("k"), None);
        cache.write("k", &vec![1u32, 2]).unwrap();
        assert_eq!(cache.read::<Vec<u32>>("k"), Some(vec![1, 2]));
        std::fs::write(cache.path_for("k"), b"{not json").unwrap();
        assert_eq!(cache.read::<Vec<u32>>("k"), None);
    }

    #[test]
    fn keys_are_sanitized() {
        let cache = DiskCache::new(tempfile::tempdir().unwrap().path()).unwrap();
        let path = cache.path_for(&tracks_key("spotify:playlist:a/b"));
        assert_eq!(path.file_name().unwrap(), "tracks-spotify_playlist_a_b.json");
    }
}
```

`library/service.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::app::FailReason;
    use crate::library::FetchError;
    use crate::model::{Collection, CollectionKind, Section, Track};

    struct StubSource {
        calls: AtomicU32,
        fail: bool,
    }

    fn collection(name: &str) -> Collection {
        Collection { uri: format!("spotify:playlist:{name}"), kind: CollectionKind::Playlist, name: name.into(), subtitle: String::new(), image_url: None }
    }

    impl LibrarySource for StubSource {
        async fn section(&self, _section: Section) -> Result<Vec<Collection>, FetchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;
            if self.fail { Err(FetchError::Offline) } else { Ok(vec![collection("fresh")]) }
        }
        async fn tracks(&self, _uri: &str) -> Result<Vec<Track>, FetchError> {
            Ok(vec![])
        }
    }

    async fn next_update(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Input>) -> LibraryUpdate {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap() {
            Input::Library(update) => update,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn sends_cached_then_fresh_and_writes_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(DiskCache::new(dir.path()).unwrap());
        cache.write(Section::Playlists.cache_key(), &vec![collection("cached")]).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource { calls: AtomicU32::new(0), fail: false });
        let requests = spawn_library(source, cache.clone(), tx);
        requests.send(LibraryRequest::Section(Section::Playlists)).unwrap();
        assert_eq!(next_update(&mut rx).await, LibraryUpdate::Section { section: Section::Playlists, items: vec![collection("cached")] });
        assert_eq!(next_update(&mut rx).await, LibraryUpdate::Section { section: Section::Playlists, items: vec![collection("fresh")] });
        assert_eq!(cache.read::<Vec<Collection>>(Section::Playlists.cache_key()), Some(vec![collection("fresh")]));
    }

    #[tokio::test]
    async fn failure_is_reported_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource { calls: AtomicU32::new(0), fail: true });
        let requests = spawn_library(source, Arc::new(DiskCache::new(dir.path()).unwrap()), tx);
        requests.send(LibraryRequest::Section(Section::Albums)).unwrap();
        assert_eq!(next_update(&mut rx).await, LibraryUpdate::SectionFailed { section: Section::Albums, reason: FailReason::Offline });
    }

    #[tokio::test]
    async fn duplicate_in_flight_requests_fetch_once() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let source = Arc::new(StubSource { calls: AtomicU32::new(0), fail: false });
        let requests = spawn_library(source.clone(), Arc::new(DiskCache::new(dir.path()).unwrap()), tx);
        requests.send(LibraryRequest::Section(Section::Playlists)).unwrap();
        requests.send(LibraryRequest::Section(Section::Playlists)).unwrap();
        next_update(&mut rx).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod cache; pub mod fake; pub mod service;` to `library/mod.rs`, and create `fake.rs` empty for now.
Run: `cargo test -p muzak-player library::`
Expected: compile errors.

- [ ] **Step 3: Implement `cache.rs`**

```rust
//! Tiny JSON-file cache so screens show instantly and work offline.

use std::path::PathBuf;

use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct DiskCache {
    dir: PathBuf,
}

pub fn tracks_key(uri: &str) -> String {
    format!("tracks-{uri}")
}

impl DiskCache {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn path_for(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let bytes = std::fs::read(self.path_for(key)).ok()?;
        match serde_json::from_slice(&bytes) {
            Ok(value) => Some(value),
            Err(e) => {
                tracing::warn!(key, "ignoring corrupt cache entry: {e}");
                None
            }
        }
    }

    pub fn write<T: Serialize>(&self, key: &str, value: &T) -> std::io::Result<()> {
        let path = self.path_for(key);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(value)?)?;
        std::fs::rename(tmp, path)
    }
}
```

- [ ] **Step 4: Implement `service.rs`**

```rust
//! Serves library requests: cached data first, then a fresh fetch.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::{self, UnboundedSender};

use super::LibrarySource;
use super::cache::{DiskCache, tracks_key};
use crate::app::{Input, LibraryRequest, LibraryUpdate};
use crate::model::{Collection, Track};

pub fn spawn_library<S: LibrarySource>(
    source: Arc<S>,
    cache: Arc<DiskCache>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<LibraryRequest> {
    let (tx, mut rx) = mpsc::unbounded_channel::<LibraryRequest>();
    let in_flight = Arc::new(Mutex::new(HashSet::<LibraryRequest>::new()));
    tokio::spawn(async move {
        while let Some(request) = rx.recv().await {
            if !in_flight.lock().unwrap().insert(request.clone()) {
                continue;
            }
            let (source, cache, inputs, in_flight) =
                (source.clone(), cache.clone(), inputs.clone(), in_flight.clone());
            tokio::spawn(async move {
                serve(&*source, &cache, &inputs, request.clone()).await;
                in_flight.lock().unwrap().remove(&request);
            });
        }
    });
    tx
}

async fn serve<S: LibrarySource>(
    source: &S,
    cache: &DiskCache,
    inputs: &UnboundedSender<Input>,
    request: LibraryRequest,
) {
    let send = |update| {
        let _ = inputs.send(Input::Library(update));
    };
    match request {
        LibraryRequest::Section(section) => {
            let key = section.cache_key();
            if let Some(items) = cache.read::<Vec<Collection>>(key) {
                send(LibraryUpdate::Section { section, items });
            }
            match source.section(section).await {
                Ok(items) => {
                    if let Err(e) = cache.write(key, &items) {
                        tracing::warn!("cache write failed for {key}: {e}");
                    }
                    send(LibraryUpdate::Section { section, items });
                }
                Err(e) => {
                    tracing::warn!("loading {section:?} failed: {e}");
                    send(LibraryUpdate::SectionFailed { section, reason: e.reason() });
                }
            }
        }
        LibraryRequest::Tracks { collection_uri } => {
            let key = tracks_key(&collection_uri);
            if let Some(tracks) = cache.read::<Vec<Track>>(&key) {
                send(LibraryUpdate::Tracks { collection_uri: collection_uri.clone(), tracks });
            }
            match source.tracks(&collection_uri).await {
                Ok(tracks) => {
                    if let Err(e) = cache.write(&key, &tracks) {
                        tracing::warn!("cache write failed for {key}: {e}");
                    }
                    send(LibraryUpdate::Tracks { collection_uri, tracks });
                }
                Err(e) => {
                    tracing::warn!("loading {collection_uri} failed: {e}");
                    send(LibraryUpdate::TracksFailed { collection_uri, reason: e.reason() });
                }
            }
        }
    }
}
```

- [ ] **Step 5: Implement `fake.rs`**

```rust
//! In-memory catalog for `--fake` mode: UI work without a Spotify account.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::{FetchError, LibrarySource};
use crate::model::{Collection, CollectionKind, LIKED_URI, Section, Track, liked_collection};

pub struct FakeCatalog {
    pub playlists: Vec<Collection>,
    pub albums: Vec<Collection>,
    tracks: HashMap<String, Vec<Track>>,
}

impl FakeCatalog {
    pub fn sample() -> Self {
        let playlist_names = [
            "Dance Party",
            "Bedtime",
            "Car Songs",
            "Frozen Favourites",
            "Morning Mix",
            "A Very Long Playlist Name That Should Be Cut Off Nicely",
        ];
        let album_names = [
            ("Moana", "Various Artists"),
            ("Encanto", "Various Artists"),
            ("Abbey Road", "The Beatles"),
            ("Rumours", "Fleetwood Mac"),
        ];
        let playlists: Vec<Collection> = playlist_names
            .iter()
            .enumerate()
            .map(|(i, name)| Collection {
                uri: format!("spotify:playlist:fake{i}"),
                kind: CollectionKind::Playlist,
                name: name.to_string(),
                subtitle: "Mum".into(),
                image_url: None,
            })
            .collect();
        let albums: Vec<Collection> = album_names
            .iter()
            .enumerate()
            .map(|(i, (name, artist))| Collection {
                uri: format!("spotify:album:fake{i}"),
                kind: CollectionKind::Album,
                name: name.to_string(),
                subtitle: artist.to_string(),
                image_url: None,
            })
            .collect();
        let mut tracks = HashMap::new();
        for c in playlists.iter().chain(albums.iter()) {
            tracks.insert(c.uri.clone(), fake_tracks(&c.name, 12));
        }
        tracks.insert(LIKED_URI.to_string(), fake_tracks("Liked", 20));
        Self { playlists, albums, tracks }
    }

    pub fn tracks_for(&self, uri: &str) -> Vec<Track> {
        self.tracks.get(uri).cloned().unwrap_or_default()
    }
}

fn fake_tracks(collection: &str, n: u32) -> Vec<Track> {
    (1..=n)
        .map(|i| Track {
            uri: format!("spotify:track:fake-{}-{i}", collection.len()),
            name: format!("{collection} Song {i}"),
            artists: "The Fake Band".into(),
            album: collection.to_string(),
            image_url: None,
            // Short tracks so auto-advance is visible during UI work.
            duration_ms: 20_000 + i * 1_000,
        })
        .collect()
}

pub struct FakeSource {
    catalog: Arc<FakeCatalog>,
}

impl FakeSource {
    pub fn new(catalog: Arc<FakeCatalog>) -> Self {
        Self { catalog }
    }
}

impl LibrarySource for FakeSource {
    async fn section(&self, section: Section) -> Result<Vec<Collection>, FetchError> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(match section {
            Section::Playlists => self.catalog.playlists.clone(),
            Section::Albums => self.catalog.albums.clone(),
            Section::Recent => {
                let mut recent = self.catalog.playlists[..2].to_vec();
                recent.push(self.catalog.albums[0].clone());
                recent.push(liked_collection());
                recent
            }
            Section::Liked => vec![liked_collection()],
        })
    }

    async fn tracks(&self, collection_uri: &str) -> Result<Vec<Track>, FetchError> {
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(self.catalog.tracks_for(collection_uri))
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p muzak-player`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/muzak-player/src/library
git commit -m "feat: add disk cache, cache-first library service, fake catalog"
```

---

### Task 7: Cover image loader

**Files:**
- Create: `crates/muzak-player/src/images.rs`
- Modify: `crates/muzak-player/src/lib.rs` (add `pub mod images;`)

**Interfaces:**
- Produces:
  - `ImageLoader::new(dir: PathBuf, max_px: u32) -> anyhow::Result<ImageLoader>`
  - `ImageLoader::load(&self, url) -> anyhow::Result<image::RgbaImage>`
  - `fn cache_file_name(url: &str) -> String`
  - `fn decode(bytes: &[u8], max_px: u32) -> anyhow::Result<RgbaImage>`
  - `type ImageSink = Arc<dyn Fn(String, Option<RgbaImage>) + Send + Sync>`
  - `fn spawn_image_loader(loader: Arc<ImageLoader>, requests: UnboundedReceiver<String>, sink: ImageSink)`

- [ ] **Step 1: Write the failing tests**

`images.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 100, 50, 255]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn decode_shrinks_large_images_keeping_aspect() {
        let img = decode(&png(640, 320), 300).unwrap();
        assert_eq!(img.dimensions(), (300, 150));
    }

    #[test]
    fn decode_keeps_small_images() {
        assert_eq!(decode(&png(64, 64), 300).unwrap().dimensions(), (64, 64));
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode(b"nope", 300).is_err());
    }

    #[test]
    fn cache_file_name_uses_spotify_image_id() {
        assert_eq!(cache_file_name("https://i.scdn.co/image/ab67616d00001e02ff9ca1"), "ab67616d00001e02ff9ca1");
        let odd = cache_file_name("https://example.com/");
        assert_eq!(odd.len(), 16, "falls back to a hash: {odd}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod images;` to `lib.rs`.
Run: `cargo test -p muzak-player images`
Expected: compile errors.

- [ ] **Step 3: Implement `images.rs`**

```rust
//! Downloads cover art, caches the bytes on disk, decodes off the UI thread.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use image::RgbaImage;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::UnboundedReceiver;

pub type ImageSink = Arc<dyn Fn(String, Option<RgbaImage>) + Send + Sync>;

pub struct ImageLoader {
    client: reqwest::Client,
    dir: PathBuf,
    max_px: u32,
}

pub fn cache_file_name(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or("");
    let safe: String = last.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if safe.is_empty() {
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    } else {
        safe
    }
}

pub fn decode(bytes: &[u8], max_px: u32) -> anyhow::Result<RgbaImage> {
    let img = image::load_from_memory(bytes)?;
    let img = if img.width() > max_px || img.height() > max_px { img.thumbnail(max_px, max_px) } else { img };
    Ok(img.to_rgba8())
}

impl ImageLoader {
    pub fn new(dir: PathBuf, max_px: u32) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
        Ok(Self { client, dir, max_px })
    }

    pub async fn load(&self, url: &str) -> anyhow::Result<RgbaImage> {
        let path = self.dir.join(cache_file_name(url));
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(_) => {
                let bytes = self.client.get(url).send().await?.error_for_status()?.bytes().await?.to_vec();
                let tmp = path.with_extension("tmp");
                tokio::fs::write(&tmp, &bytes).await?;
                tokio::fs::rename(&tmp, &path).await?;
                bytes
            }
        };
        let max_px = self.max_px;
        let decoded = tokio::task::spawn_blocking(move || decode(&bytes, max_px)).await?;
        if decoded.is_err() {
            let _ = tokio::fs::remove_file(&path).await;
        }
        decoded
    }
}

/// Loads requested URLs, at most 3 at a time, and hands results to `sink`.
pub fn spawn_image_loader(loader: Arc<ImageLoader>, mut requests: UnboundedReceiver<String>, sink: ImageSink) {
    let limit = Arc::new(Semaphore::new(3));
    tokio::spawn(async move {
        while let Some(url) = requests.recv().await {
            let (loader, sink, limit) = (loader.clone(), sink.clone(), limit.clone());
            tokio::spawn(async move {
                let Ok(_permit) = limit.acquire_owned().await else { return };
                let result = loader.load(&url).await;
                if let Err(e) = &result {
                    tracing::warn!("cover {url} failed: {e:#}");
                }
                sink(url, result.ok());
            });
        }
    });
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p muzak-player images`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: add cover image loader"
```

---

### Task 8: Fake player and runtime (fake mode)

**Files:**
- Create: `crates/muzak-player/src/player/mod.rs`, `player/fake.rs`
- Create: `crates/muzak-player/src/runtime.rs`
- Modify: `crates/muzak-player/src/lib.rs` (add `pub mod player; pub mod runtime;`)

**Interfaces:**
- Consumes: Core (Tasks 2–3), `spawn_library`, `FakeCatalog`, `FakeSource`, `DiskCache` (Task 6), `ImageLoader`, `spawn_image_loader`, `ImageSink` (Task 7), `Config` (Task 1).
- Produces:
  - **`player`:** `percent_to_volume(u8) -> u16`, `volume_to_percent(u16) -> u8`
  - **`player::fake`:**
    - `FakePlayer::new(Arc<FakeCatalog>)`
    - `.handle(PlayerCommand) -> Vec<PlayerUpdate>`
    - `.tick(elapsed_ms: u32) -> Vec<PlayerUpdate>`
    - `spawn_fake_player(Arc<FakeCatalog>, UnboundedSender<Input>) -> UnboundedSender<PlayerCommand>`
  - **`runtime`:**
    - `type Publish = Box<dyn Fn(AppState) + Send>`
    - `fn start(config: Config, fake: bool, image_requests: UnboundedReceiver<String>, publish: Publish, images: ImageSink) -> anyhow::Result<UnboundedSender<Input>>`

- [ ] **Step 1: Write the failing tests**

`player/mod.rs`:
```rust
pub mod fake;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_conversions_roundtrip() {
        assert_eq!(percent_to_volume(0), 0);
        assert_eq!(percent_to_volume(100), u16::MAX);
        assert_eq!(percent_to_volume(250), u16::MAX);
        for p in 0..=100u8 {
            assert_eq!(volume_to_percent(percent_to_volume(p)), p);
        }
    }
}
```

`player/fake.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_plays_and_runs_to_the_end() {
        let catalog = Arc::new(FakeCatalog::sample());
        let uri = catalog.albums[0].uri.clone();
        let mut p = FakePlayer::new(catalog.clone());
        let updates = p.handle(PlayerCommand::Load { context_uri: uri.clone(), start_index: Some(11), shuffle: false });
        assert!(matches!(updates[1], PlayerUpdate::TrackChanged(ref t) if t.name.ends_with("Song 12")));
        assert_eq!(updates[2], PlayerUpdate::Playing { position_ms: 0 });
        assert_eq!(p.tick(1_000), vec![PlayerUpdate::Position { position_ms: 1_000 }]);
        assert_eq!(p.tick(60_000), vec![PlayerUpdate::Stopped], "last track ended");
        assert!(p.tick(1_000).is_empty());
    }

    #[test]
    fn unknown_context_stops() {
        let mut p = FakePlayer::new(Arc::new(FakeCatalog::sample()));
        assert_eq!(
            p.handle(PlayerCommand::Load { context_uri: "spotify:playlist:nope".into(), start_index: None, shuffle: false }),
            vec![PlayerUpdate::Stopped]
        );
    }
}
```

`runtime.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::app::{PlayStatus, UiAction};
    use crate::model::Section;

    #[test]
    fn fake_runtime_loads_library_and_plays() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::parse(&format!("device_name = \"Test\"\nstate_dir = \"{}\"\n", dir.path().display())).unwrap();
        let (state_tx, state_rx) = std::sync::mpsc::channel();
        let (_image_tx, image_rx) = tokio::sync::mpsc::unbounded_channel();
        let inputs = start(config, true, image_rx, Box::new(move |s| { let _ = state_tx.send(s); }), Arc::new(|_, _| {})).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let wait_for = |pred: &dyn Fn(&AppState) -> bool| loop {
            let state = state_rx.recv_timeout(Duration::from_secs(5)).expect("state published");
            if pred(&state) {
                break;
            }
            assert!(Instant::now() < deadline, "timed out");
        };
        wait_for(&|s| s.sections.get(&Section::Playlists).and_then(|x| x.data.as_ref()).is_some_and(|d| !d.is_empty()));
        inputs.send(Input::Ui(UiAction::PlayCollection { uri: "spotify:playlist:fake0".into(), shuffle: false })).unwrap();
        wait_for(&|s| s.playback.status == PlayStatus::Playing && s.playback.track.is_some());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod player; pub mod runtime;` to `lib.rs`.
Run: `cargo test -p muzak-player player:: runtime::`
Expected: compile errors.

- [ ] **Step 3: Implement volume conversions**

At the top of `player/mod.rs`:
```rust
/// librespot volume is 0..=65535; the app uses percent.
pub fn percent_to_volume(percent: u8) -> u16 {
    ((percent.min(100) as u32 * u16::MAX as u32 + 50) / 100) as u16
}

pub fn volume_to_percent(volume: u16) -> u8 {
    ((volume as u32 * 100 + u16::MAX as u32 / 2) / u16::MAX as u32) as u8
}
```

- [ ] **Step 4: Implement `player/fake.rs`**

Put this above the tests:
```rust
//! Pretend player for `--fake` mode. Shuffle just reverses the queue.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedSender};

use crate::app::{Input, PlayerCommand, PlayerUpdate};
use crate::library::fake::FakeCatalog;
use crate::model::Track;

pub struct FakePlayer {
    catalog: Arc<FakeCatalog>,
    queue: Vec<Track>,
    index: usize,
    playing: bool,
    position_ms: u32,
}

impl FakePlayer {
    pub fn new(catalog: Arc<FakeCatalog>) -> Self {
        Self { catalog, queue: Vec::new(), index: 0, playing: false, position_ms: 0 }
    }

    pub fn handle(&mut self, command: PlayerCommand) -> Vec<PlayerUpdate> {
        match command {
            PlayerCommand::Load { context_uri, start_index, shuffle } => {
                self.queue = self.catalog.tracks_for(&context_uri);
                if shuffle {
                    self.queue.reverse();
                }
                if self.queue.is_empty() {
                    self.playing = false;
                    return vec![PlayerUpdate::Stopped];
                }
                self.index = (start_index.unwrap_or(0) as usize).min(self.queue.len() - 1);
                self.start_current()
            }
            PlayerCommand::Play if !self.queue.is_empty() => {
                self.playing = true;
                vec![PlayerUpdate::Playing { position_ms: self.position_ms }]
            }
            PlayerCommand::Play => vec![],
            PlayerCommand::Pause => {
                self.playing = false;
                vec![PlayerUpdate::Paused { position_ms: self.position_ms }]
            }
            PlayerCommand::Next => self.skip(1),
            PlayerCommand::Previous if self.position_ms > 3_000 => {
                self.position_ms = 0;
                vec![PlayerUpdate::Position { position_ms: 0 }]
            }
            PlayerCommand::Previous => self.skip(-1),
            PlayerCommand::Seek { position_ms } => {
                self.position_ms = position_ms;
                vec![PlayerUpdate::Position { position_ms }]
            }
            PlayerCommand::SetVolume { percent } => vec![PlayerUpdate::Volume { percent }],
            PlayerCommand::SetShuffle(shuffle) => vec![PlayerUpdate::Shuffle(shuffle)],
            PlayerCommand::SetRepeat(repeat) => vec![PlayerUpdate::Repeat(repeat)],
        }
    }

    pub fn tick(&mut self, elapsed_ms: u32) -> Vec<PlayerUpdate> {
        if !self.playing {
            return vec![];
        }
        self.position_ms += elapsed_ms;
        if self.position_ms >= self.queue[self.index].duration_ms {
            self.skip(1)
        } else {
            vec![PlayerUpdate::Position { position_ms: self.position_ms }]
        }
    }

    fn skip(&mut self, delta: i64) -> Vec<PlayerUpdate> {
        if self.queue.is_empty() {
            return vec![];
        }
        let next = self.index as i64 + delta;
        if next < 0 || next >= self.queue.len() as i64 {
            self.playing = false;
            self.position_ms = 0;
            return vec![PlayerUpdate::Stopped];
        }
        self.index = next as usize;
        self.start_current()
    }

    fn start_current(&mut self) -> Vec<PlayerUpdate> {
        self.playing = true;
        self.position_ms = 0;
        vec![
            PlayerUpdate::Loading,
            PlayerUpdate::TrackChanged(self.queue[self.index].clone()),
            PlayerUpdate::Playing { position_ms: 0 },
        ]
    }
}

pub fn spawn_fake_player(catalog: Arc<FakeCatalog>, inputs: UnboundedSender<Input>) -> UnboundedSender<PlayerCommand> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut player = FakePlayer::new(catalog);
        let send = |updates: Vec<PlayerUpdate>| {
            for update in updates {
                let _ = inputs.send(Input::Player(update));
            }
        };
        send(vec![PlayerUpdate::Connected]);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                command = rx.recv() => match command {
                    Some(command) => send(player.handle(command)),
                    None => return,
                },
                _ = tick.tick() => send(player.tick(1_000)),
            }
        }
    });
    tx
}
```

- [ ] **Step 5: Implement `runtime.rs`**

Put this above the tests:
```rust
//! Owns the tokio runtime: feeds inputs to the core, carries out effects, publishes state.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::app::{AppState, Core, CoreConfig, Effect, Input, LibraryRequest, PlayerCommand};
use crate::config::Config;
use crate::images::{ImageLoader, ImageSink, spawn_image_loader};
use crate::library::cache::DiskCache;
use crate::library::fake::{FakeCatalog, FakeSource};
use crate::library::service::spawn_library;
use crate::player::fake::spawn_fake_player;

pub type Publish = Box<dyn Fn(AppState) + Send>;

/// Starts the background runtime on its own thread and returns the input channel.
pub fn start(
    config: Config,
    fake: bool,
    image_requests: UnboundedReceiver<String>,
    publish: Publish,
    images: ImageSink,
) -> anyhow::Result<UnboundedSender<Input>> {
    let (inputs_tx, inputs_rx) = mpsc::unbounded_channel();
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    let inputs = inputs_tx.clone();
    std::thread::Builder::new().name("muzak-runtime".into()).spawn(move || {
        runtime.block_on(async move {
            if let Err(e) = run(config, fake, inputs, inputs_rx, image_requests, publish, images).await {
                tracing::error!("runtime stopped: {e:#}");
            }
        });
    })?;
    Ok(inputs_tx)
}

async fn run(
    config: Config,
    fake: bool,
    inputs: UnboundedSender<Input>,
    mut inputs_rx: UnboundedReceiver<Input>,
    image_requests: UnboundedReceiver<String>,
    publish: Publish,
    images: ImageSink,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(&config.state_dir)?;
    let cache = Arc::new(DiskCache::new(config.cache_dir())?);
    let (player, library) = if fake {
        let catalog = Arc::new(FakeCatalog::sample());
        let library = spawn_library(Arc::new(FakeSource::new(catalog.clone())), cache, inputs.clone());
        (spawn_fake_player(catalog, inputs.clone()), library)
    } else {
        anyhow::bail!("real Spotify playback is wired up in Task 11; run with --fake");
    };
    spawn_image_loader(Arc::new(ImageLoader::new(config.images_dir(), 300)?), image_requests, images);

    let started = Instant::now();
    let now_ms = || started.elapsed().as_millis() as u64;
    let core_config = CoreConfig {
        dim_after_ms: config.dim_after_secs * 1000,
        off_after_ms: config.off_after_secs * 1000,
        initial_volume: config.initial_volume,
    };
    let (mut core, effects) = Core::new(core_config, now_ms());
    dispatch(effects, &player, &library);
    publish(core.state().clone());

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        let input = tokio::select! {
            Some(input) = inputs_rx.recv() => input,
            _ = tick.tick() => Input::Tick,
        };
        let effects = core.handle(input, now_ms());
        dispatch(effects, &player, &library);
        publish(core.state().clone());
    }
}

fn dispatch(effects: Vec<Effect>, player: &UnboundedSender<PlayerCommand>, library: &UnboundedSender<LibraryRequest>) {
    for effect in effects {
        match effect {
            Effect::Player(command) => {
                let _ = player.send(command);
            }
            Effect::Library(request) => {
                let _ = library.send(request);
            }
            // Backlight control arrives in Task 12; the UI overlay already dims the screen.
            Effect::Display(mode) => tracing::debug!("display {mode:?}"),
        }
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p muzak-player`
Expected: all tests pass, including `fake_runtime_loads_library_and_plays`.

- [ ] **Step 7: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: add fake player and runtime"
```

---

### Task 9: Slint UI and bridge (`--fake` is fully usable)

**Files:**
- Create: `crates/muzak-player/ui/theme.slint`, `ui/types.slint`
- Create: `ui/components/{icon_button,pill_button,tile,track_row,scrubber,rail,mini_player,banner,state_message}.slint`
- Create: `ui/screens/{grid,detail,now_playing,idle,auth_needed}.slint`
- Create: `ui/icons/*.svg`, `ui/fonts/Nunito-Bold.ttf`, `ui/fonts/Nunito-ExtraBold.ttf`
- Replace: `crates/muzak-player/ui/app.slint`, `crates/muzak-player/src/main.rs`
- Create: `crates/muzak-player/src/ui_bridge.rs`
- Modify: `crates/muzak-player/src/lib.rs` (add `pub mod ui_bridge;`)

**Interfaces:**
- Consumes:
  - `view::build` and the view types (Task 4)
  - `runtime::start`, `Publish` (Task 8)
  - `ImageSink` (Task 7)
  - `Input`, `UiAction` (Task 2)
- Produces:
  - **Slint-generated types:**
    - `AppWindow`
    - `TileData { uri, title, subtitle, image, has_image }`
    - `TrackData { title, artists, duration, current }`
    - `ScreenKind::{Grid, Detail, NowPlaying}`
    - `LoadState::{Loading, Empty, Failed, Ready}`
  - **`ui_bridge`:**
    - `install(&AppWindow, UnboundedSender<String>)`
    - `publish(AppState)`
    - `deliver_image(String, Option<RgbaImage>)`
    - `wire_callbacks(&AppWindow, UnboundedSender<Input>)`

This task is UI glue. It is verified by running `--fake` on the Mac against the checklist in Step 9. The logic it shows is already covered by the `view` tests.

- [ ] **Step 1: Fonts and icons**

```bash
cd crates/muzak-player/ui
mkdir -p fonts icons components screens
curl -fsSL -o fonts/Nunito-Bold.ttf https://cdn.jsdelivr.net/fontsource/fonts/nunito@latest/latin-700-normal.ttf
curl -fsSL -o fonts/Nunito-ExtraBold.ttf https://cdn.jsdelivr.net/fontsource/fonts/nunito@latest/latin-800-normal.ttf
file fonts/*.ttf   # expect: TrueType Font data
```
Nunito is under the SIL Open Font License. Add `crates/muzak-player/ui/fonts/LICENSE.txt` with the OFL text from https://openfontlicense.org/documents/OFL.txt.

Create each icon as `ui/icons/<name>.svg` with this wrapper, substituting the path data below (Material Icons, Apache 2.0):
```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#ffffff"><path d="PATH"/></svg>
```
| file | PATH |
|---|---|
| play.svg | `M8 5v14l11-7z` |
| pause.svg | `M6 5h4v14H6zM14 5h4v14h-4z` |
| next.svg | `M6 6l8.5 6L6 18zM16 6h2v12h-2z` |
| previous.svg | `M18 6l-8.5 6L18 18zM6 6h2v12H6z` |
| shuffle.svg | `M10.59 9.17L5.41 4 4 5.41l5.17 5.17 1.42-1.41zM14.5 4l2.04 2.04L4 18.59 5.41 20 17.96 7.46 20 9.5V4h-5.5zm.33 9.41l-1.41 1.41 3.13 3.13L14.5 20H20v-5.5l-2.04 2.04-3.13-3.13z` |
| repeat.svg | `M7 7h10v3l4-4-4-4v3H5v6h2V7zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2v4z` |
| playlists.svg | `M15 6H3v2h12V6zm0 4H3v2h12v-2zM3 16h8v-2H3v2zM17 6v8.18c-.31-.11-.65-.18-1-.18-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3V8h3V6h-5z` |
| album.svg | `M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 14.5c-2.49 0-4.5-2.01-4.5-4.5S9.51 7.5 12 7.5s4.5 2.01 4.5 4.5-2.01 4.5-4.5 4.5zm0-5.5c-.55 0-1 .45-1 1s.45 1 1 1 1-.45 1-1-.45-1-1-1z` |
| heart.svg | `M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z` |
| history.svg | `M13 3a9 9 0 0 0-9 9H1l3.89 3.89.07.14L9 12H6c0-3.87 3.13-7 7-7s7 3.13 7 7-3.13 7-7 7c-1.93 0-3.68-.79-4.94-2.06l-1.42 1.42A8.954 8.954 0 0 0 13 21a9 9 0 0 0 0-18zm-1 5v5l4.28 2.54.72-1.21-3.5-2.08V8H12z` |
| back.svg | `M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z` |
| volume.svg | `M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02z` |
| note.svg | `M12 3v10.55c-.59-.34-1.27-.55-2-.55-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4V7h4V3h-6z` |

- [ ] **Step 2: Theme and shared types**

`ui/theme.slint`:
```slint
export global Theme {
    out property <color> bg: #14121f;
    out property <color> surface: #221f33;
    out property <color> surface-hi: #2f2b45;
    out property <color> text: #f4f2ff;
    out property <color> text-dim: #a9a3c7;
    out property <color> accent: #ffb703;
    out property <color> accent-text: #1b1300;
    out property <length> rail-width: 96px;
    out property <length> mini-height: 80px;
    out property <length> touch: 64px;
    out property <length> radius: 14px;
    out property <length> font-small: 15px;
    out property <length> font-body: 18px;
    out property <length> font-title: 26px;
}
```

`ui/types.slint`:
```slint
export struct TileData {
    uri: string,
    title: string,
    subtitle: string,
    image: image,
    has-image: bool,
}

export struct TrackData {
    title: string,
    artists: string,
    duration: string,
    current: bool,
}

export enum ScreenKind { grid, detail, now-playing }

export enum LoadState { loading, empty, failed, ready }
```

- [ ] **Step 3: Components**

`ui/components/icon_button.slint`:
```slint
import { Theme } from "../theme.slint";

export component IconButton inherits Rectangle {
    in property <image> icon;
    in property <length> size: Theme.touch;
    in property <bool> primary: false;
    in property <bool> active: false;
    callback clicked();
    width: root.size;
    height: root.size;
    border-radius: root.size / 2;
    background: root.primary
        ? (touch.pressed ? Theme.accent.darker(0.15) : Theme.accent)
        : (touch.pressed ? Theme.surface-hi : transparent);
    Image {
        source: root.icon;
        width: root.size * 0.5;
        height: root.size * 0.5;
        colorize: root.primary ? Theme.accent-text : (root.active ? Theme.accent : Theme.text);
    }
    touch := TouchArea {
        clicked => { root.clicked(); }
    }
}
```

`ui/components/pill_button.slint`:
```slint
import { Theme } from "../theme.slint";

export component PillButton inherits Rectangle {
    in property <string> label;
    in property <image> icon;
    in property <bool> primary: false;
    callback clicked();
    height: Theme.touch;
    min-width: 120px;
    border-radius: root.height / 2;
    background: root.primary
        ? (touch.pressed ? Theme.accent.darker(0.15) : Theme.accent)
        : (touch.pressed ? Theme.surface-hi : Theme.surface);
    HorizontalLayout {
        padding-left: 18px;
        padding-right: 22px;
        spacing: 8px;
        alignment: center;
        Image {
            source: root.icon;
            width: 26px;
            height: 26px;
            colorize: root.primary ? Theme.accent-text : Theme.text;
        }
        Text {
            text: root.label;
            color: root.primary ? Theme.accent-text : Theme.text;
            font-size: Theme.font-body;
            font-weight: 800;
            vertical-alignment: center;
        }
    }
    touch := TouchArea {
        clicked => { root.clicked(); }
    }
}
```

`ui/components/tile.slint`:
```slint
import { Theme } from "../theme.slint";
import { TileData } from "../types.slint";

export component Tile inherits Rectangle {
    in property <TileData> data;
    in property <length> art-size;
    callback clicked();
    border-radius: Theme.radius;
    background: touch.pressed ? Theme.surface-hi : transparent;
    VerticalLayout {
        padding: 6px;
        spacing: 6px;
        Rectangle {
            width: root.art-size;
            height: root.art-size;
            border-radius: Theme.radius - 4px;
            clip: true;
            background: Theme.surface;
            Image {
                source: @image-url("../icons/note.svg");
                width: 40%;
                height: 40%;
                colorize: Theme.text-dim;
                visible: !root.data.has-image;
            }
            Image {
                width: 100%;
                height: 100%;
                source: root.data.image;
                image-fit: cover;
                opacity: root.data.has-image ? 1 : 0;
                animate opacity { duration: 200ms; }
            }
        }
        Text {
            text: root.data.title;
            color: Theme.text;
            font-size: Theme.font-body;
            font-weight: 800;
            overflow: elide;
        }
        Text {
            text: root.data.subtitle;
            color: Theme.text-dim;
            font-size: Theme.font-small;
            overflow: elide;
        }
    }
    touch := TouchArea {
        clicked => { root.clicked(); }
    }
}
```

`ui/components/track_row.slint`:
```slint
import { Theme } from "../theme.slint";
import { TrackData } from "../types.slint";

export component TrackRow inherits Rectangle {
    in property <TrackData> data;
    callback clicked();
    height: Theme.touch;
    border-radius: 10px;
    background: touch.pressed ? Theme.surface-hi : transparent;
    HorizontalLayout {
        padding-left: 12px;
        padding-right: 12px;
        spacing: 12px;
        VerticalLayout {
            alignment: center;
            Text {
                text: root.data.title;
                color: root.data.current ? Theme.accent : Theme.text;
                font-size: Theme.font-body;
                font-weight: 800;
                overflow: elide;
            }
            Text {
                text: root.data.artists;
                color: Theme.text-dim;
                font-size: Theme.font-small;
                overflow: elide;
            }
        }
        Text {
            text: root.data.duration;
            color: Theme.text-dim;
            font-size: Theme.font-small;
            vertical-alignment: center;
            horizontal-stretch: 0;
        }
    }
    touch := TouchArea {
        clicked => { root.clicked(); }
    }
}
```

`ui/components/scrubber.slint`:
```slint
import { Theme } from "../theme.slint";

// A draggable 0..1 bar. Reports the value once, on release.
export component Scrubber inherits Rectangle {
    in property <float> value;
    callback changed(float);
    property <float> drag-value;
    property <float> shown: touch.pressed ? root.drag-value : Math.clamp(root.value, 0, 1);
    min-width: 120px;
    height: Theme.touch;
    Rectangle {
        y: (root.height - 8px) / 2;
        width: root.width;
        height: 8px;
        border-radius: 4px;
        background: Theme.surface-hi;
    }
    Rectangle {
        x: 0;
        y: (root.height - 8px) / 2;
        width: root.width * root.shown;
        height: 8px;
        border-radius: 4px;
        background: Theme.accent;
    }
    Rectangle {
        x: root.width * root.shown - 13px;
        y: (root.height - 26px) / 2;
        width: 26px;
        height: 26px;
        border-radius: 13px;
        background: Theme.text;
    }
    touch := TouchArea {
        pointer-event(event) => {
            if (event.kind == PointerEventKind.down) {
                root.drag-value = Math.clamp(self.mouse-x / self.width, 0, 1);
            }
            if (event.kind == PointerEventKind.up) {
                root.changed(root.drag-value);
            }
        }
        moved => {
            root.drag-value = Math.clamp(self.mouse-x / self.width, 0, 1);
        }
    }
}
```

`ui/components/rail.slint`:
```slint
import { Theme } from "../theme.slint";

component RailItem inherits Rectangle {
    in property <image> icon;
    in property <string> label;
    in property <bool> active;
    callback clicked();
    height: 92px;
    background: root.active ? Theme.surface-hi : transparent;
    Rectangle {
        x: 0;
        width: 5px;
        height: root.height * 0.6;
        border-radius: 3px;
        background: root.active ? Theme.accent : transparent;
    }
    VerticalLayout {
        alignment: center;
        spacing: 4px;
        Image {
            source: root.icon;
            height: 34px;
            image-fit: contain;
            colorize: root.active ? Theme.accent : Theme.text-dim;
        }
        Text {
            text: root.label;
            horizontal-alignment: center;
            font-size: 13px;
            color: root.active ? Theme.text : Theme.text-dim;
        }
    }
    TouchArea {
        clicked => { root.clicked(); }
    }
}

export component Rail inherits Rectangle {
    in property <int> selected;
    callback select(int);
    width: Theme.rail-width;
    background: Theme.surface;
    VerticalLayout {
        padding-top: 12px;
        alignment: start;
        RailItem { icon: @image-url("../icons/playlists.svg"); label: "Playlists"; active: root.selected == 0; clicked => { root.select(0); } }
        RailItem { icon: @image-url("../icons/album.svg"); label: "Albums"; active: root.selected == 1; clicked => { root.select(1); } }
        RailItem { icon: @image-url("../icons/heart.svg"); label: "Liked"; active: root.selected == 2; clicked => { root.select(2); } }
        RailItem { icon: @image-url("../icons/history.svg"); label: "Recent"; active: root.selected == 3; clicked => { root.select(3); } }
    }
}
```

`ui/components/mini_player.slint`:
```slint
import { Theme } from "../theme.slint";
import { IconButton } from "icon_button.slint";

export component MiniPlayer inherits Rectangle {
    in property <string> title;
    in property <string> artists;
    in property <image> art;
    in property <bool> has-art;
    in property <bool> playing;
    callback open();
    callback toggle-play();
    height: Theme.mini-height;
    background: Theme.surface;
    TouchArea {
        clicked => { root.open(); }
    }
    HorizontalLayout {
        padding: 8px;
        padding-right: 16px;
        spacing: 12px;
        Rectangle {
            width: 64px;
            height: 64px;
            border-radius: 8px;
            clip: true;
            background: Theme.surface-hi;
            Image {
                width: 100%;
                height: 100%;
                source: root.art;
                image-fit: cover;
                visible: root.has-art;
            }
        }
        VerticalLayout {
            alignment: center;
            Text { text: root.title; color: Theme.text; font-size: Theme.font-body; font-weight: 800; overflow: elide; }
            Text { text: root.artists; color: Theme.text-dim; font-size: Theme.font-small; overflow: elide; }
        }
        IconButton {
            size: Theme.touch;
            primary: true;
            icon: root.playing ? @image-url("../icons/pause.svg") : @image-url("../icons/play.svg");
            clicked => { root.toggle-play(); }
        }
    }
}
```

`ui/components/banner.slint`:
```slint
import { Theme } from "../theme.slint";

export component Banner inherits Rectangle {
    in property <string> text;
    height: 48px;
    width: label.preferred-width + 40px;
    border-radius: 24px;
    background: Theme.accent;
    drop-shadow-blur: 12px;
    drop-shadow-color: #00000080;
    label := Text {
        text: root.text;
        color: Theme.accent-text;
        font-size: Theme.font-body;
        font-weight: 800;
    }
}
```

`ui/components/state_message.slint`:
```slint
import { Theme } from "../theme.slint";
import { LoadState } from "../types.slint";

export component StateMessage inherits Rectangle {
    in property <LoadState> state;
    in property <string> empty-text: "Nothing here yet";
    Text {
        text: root.state == LoadState.loading ? "Loading…"
            : root.state == LoadState.failed ? "Can't load this right now"
            : root.empty-text;
        color: Theme.text-dim;
        font-size: Theme.font-body;
    }
}
```

- [ ] **Step 4: Screens**

`ui/screens/grid.slint`:
```slint
import { Theme } from "../theme.slint";
import { TileData, LoadState } from "../types.slint";
import { Tile } from "../components/tile.slint";
import { StateMessage } from "../components/state_message.slint";

export component GridScreen inherits Rectangle {
    in property <string> title;
    in property <[TileData]> tiles;
    in property <LoadState> state;
    callback open(string);
    property <int> columns: 4;
    property <length> pad: 16px;
    property <length> header: 56px;
    property <length> tile-w: (root.width - 2 * root.pad) / root.columns;
    property <length> art: root.tile-w - 12px;
    property <length> tile-h: root.art + 72px;
    background: Theme.bg;
    Text {
        x: root.pad;
        y: 12px;
        text: root.title;
        color: Theme.text;
        font-size: Theme.font-title;
        font-weight: 800;
    }
    Flickable {
        x: 0;
        y: root.header;
        width: root.width;
        height: root.height - root.header;
        viewport-height: Math.ceil(root.tiles.length / root.columns) * root.tile-h + root.pad;
        for tile[i] in root.tiles: Tile {
            x: root.pad + Math.mod(i, root.columns) * root.tile-w;
            y: Math.floor(i / root.columns) * root.tile-h;
            width: root.tile-w;
            height: root.tile-h;
            art-size: root.art;
            data: tile;
            clicked => { root.open(tile.uri); }
        }
    }
    if root.state != LoadState.ready: StateMessage {
        x: 0;
        y: root.header;
        width: root.width;
        height: root.height - root.header;
        state: root.state;
    }
}
```

`ui/screens/detail.slint`:
```slint
import { ListView } from "std-widgets.slint";
import { Theme } from "../theme.slint";
import { TileData, TrackData, LoadState } from "../types.slint";
import { IconButton } from "../components/icon_button.slint";
import { PillButton } from "../components/pill_button.slint";
import { TrackRow } from "../components/track_row.slint";
import { StateMessage } from "../components/state_message.slint";

export component DetailScreen inherits Rectangle {
    in property <TileData> header;
    in property <[TrackData]> tracks;
    in property <LoadState> state;
    callback back();
    callback play(bool);
    callback play-track(int);
    background: Theme.bg;
    HorizontalLayout {
        padding: 16px;
        spacing: 16px;
        VerticalLayout {
            width: 230px;
            spacing: 10px;
            alignment: start;
            IconButton {
                icon: @image-url("../icons/back.svg");
                clicked => { root.back(); }
            }
            Rectangle {
                width: 180px;
                height: 180px;
                border-radius: Theme.radius;
                clip: true;
                background: Theme.surface;
                Image {
                    source: @image-url("../icons/note.svg");
                    width: 40%;
                    height: 40%;
                    colorize: Theme.text-dim;
                    visible: !root.header.has-image;
                }
                Image {
                    width: 100%;
                    height: 100%;
                    source: root.header.image;
                    image-fit: cover;
                    opacity: root.header.has-image ? 1 : 0;
                    animate opacity { duration: 200ms; }
                }
            }
            Text {
                text: root.header.title;
                color: Theme.text;
                font-size: 22px;
                font-weight: 800;
                wrap: word-wrap;
                overflow: elide;
                max-height: 60px;
            }
            HorizontalLayout {
                spacing: 10px;
                alignment: start;
                PillButton {
                    label: "Play";
                    icon: @image-url("../icons/play.svg");
                    primary: true;
                    clicked => { root.play(false); }
                }
                IconButton {
                    icon: @image-url("../icons/shuffle.svg");
                    clicked => { root.play(true); }
                }
            }
        }
        Rectangle {
            ListView {
                for track[i] in root.tracks: TrackRow {
                    data: track;
                    clicked => { root.play-track(i); }
                }
            }
            if root.state != LoadState.ready: StateMessage {
                width: parent.width;
                height: parent.height;
                state: root.state;
                empty-text: "No songs here yet";
            }
        }
    }
}
```

`ui/screens/now_playing.slint`:
```slint
import { Theme } from "../theme.slint";
import { IconButton } from "../components/icon_button.slint";
import { Scrubber } from "../components/scrubber.slint";

export component NowPlayingScreen inherits Rectangle {
    in property <string> title;
    in property <string> artists;
    in property <image> art;
    in property <bool> has-art;
    in property <bool> playing;
    in property <bool> loading;
    in property <float> progress;
    in property <string> position-text;
    in property <string> duration-text;
    in property <int> volume;
    in property <bool> shuffle;
    in property <int> repeat;
    callback back();
    callback toggle-play();
    callback next();
    callback previous();
    callback seek(float);
    callback set-volume(int);
    callback toggle-shuffle();
    callback cycle-repeat();
    background: Theme.bg;
    // Absorbs taps so nothing underneath reacts.
    TouchArea {}
    HorizontalLayout {
        padding: 20px;
        spacing: 28px;
        VerticalLayout {
            spacing: 12px;
            alignment: start;
            IconButton {
                icon: @image-url("../icons/back.svg");
                clicked => { root.back(); }
            }
            Rectangle {
                width: 320px;
                height: 320px;
                border-radius: Theme.radius;
                clip: true;
                background: Theme.surface;
                Image {
                    source: @image-url("../icons/note.svg");
                    width: 35%;
                    height: 35%;
                    colorize: Theme.text-dim;
                    visible: !root.has-art;
                }
                Image {
                    width: 100%;
                    height: 100%;
                    source: root.art;
                    image-fit: cover;
                    opacity: root.has-art ? 1 : 0;
                    animate opacity { duration: 200ms; }
                }
            }
        }
        VerticalLayout {
            spacing: 14px;
            alignment: center;
            Text {
                text: root.title;
                color: Theme.text;
                font-size: 30px;
                font-weight: 800;
                wrap: word-wrap;
                overflow: elide;
                max-height: 80px;
            }
            Text {
                text: root.loading ? "Loading…" : root.artists;
                color: Theme.text-dim;
                font-size: Theme.font-body;
                overflow: elide;
            }
            Scrubber {
                value: root.progress;
                changed(v) => { root.seek(v); }
            }
            HorizontalLayout {
                Text { text: root.position-text; color: Theme.text-dim; font-size: Theme.font-small; }
                Rectangle {}
                Text { text: root.duration-text; color: Theme.text-dim; font-size: Theme.font-small; }
            }
            HorizontalLayout {
                alignment: space-between;
                IconButton {
                    icon: @image-url("../icons/shuffle.svg");
                    active: root.shuffle;
                    clicked => { root.toggle-shuffle(); }
                }
                IconButton {
                    icon: @image-url("../icons/previous.svg");
                    size: 72px;
                    clicked => { root.previous(); }
                }
                IconButton {
                    icon: root.playing ? @image-url("../icons/pause.svg") : @image-url("../icons/play.svg");
                    size: 88px;
                    primary: true;
                    clicked => { root.toggle-play(); }
                }
                IconButton {
                    icon: @image-url("../icons/next.svg");
                    size: 72px;
                    clicked => { root.next(); }
                }
                Rectangle {
                    width: Theme.touch;
                    height: Theme.touch;
                    IconButton {
                        icon: @image-url("../icons/repeat.svg");
                        active: root.repeat != 0;
                        clicked => { root.cycle-repeat(); }
                    }
                    Text {
                        visible: root.repeat == 2;
                        text: "1";
                        x: parent.width - 16px;
                        y: 6px;
                        font-size: 13px;
                        font-weight: 800;
                        color: Theme.accent;
                    }
                }
            }
            HorizontalLayout {
                spacing: 12px;
                Image {
                    source: @image-url("../icons/volume.svg");
                    width: 28px;
                    height: 28px;
                    colorize: Theme.text-dim;
                }
                Scrubber {
                    value: root.volume / 100;
                    changed(v) => { root.set-volume(Math.round(v * 100)); }
                }
            }
        }
    }
}
```

`ui/screens/idle.slint`:
```slint
import { Theme } from "../theme.slint";

// mode 1 = dim (clock), mode 2 = off (black). Any tap wakes.
export component IdleOverlay inherits Rectangle {
    in property <int> mode;
    in property <string> clock;
    callback touched();
    background: root.mode == 2 ? #000000 : #000000c0;
    animate background { duration: 600ms; }
    Text {
        visible: root.mode == 1;
        text: root.clock;
        color: Theme.text;
        font-size: 96px;
        font-weight: 800;
    }
    TouchArea {
        clicked => { root.touched(); }
    }
}
```

`ui/screens/auth_needed.slint`:
```slint
import { Theme } from "../theme.slint";

export component AuthNeededScreen inherits Rectangle {
    background: Theme.bg;
    TouchArea {}
    VerticalLayout {
        alignment: center;
        spacing: 12px;
        Text {
            text: "Ask a grown-up for help";
            font-size: 34px;
            font-weight: 800;
            color: Theme.text;
            horizontal-alignment: center;
        }
        Text {
            text: "This player needs to sign in to Spotify again.";
            font-size: Theme.font-body;
            color: Theme.text-dim;
            horizontal-alignment: center;
        }
    }
}
```

- [ ] **Step 5: Root window**

Replace `ui/app.slint`:
```slint
import "fonts/Nunito-Bold.ttf";
import "fonts/Nunito-ExtraBold.ttf";
import { Theme } from "theme.slint";
import { TileData, TrackData, ScreenKind, LoadState } from "types.slint";
import { Rail } from "components/rail.slint";
import { MiniPlayer } from "components/mini_player.slint";
import { Banner } from "components/banner.slint";
import { GridScreen } from "screens/grid.slint";
import { DetailScreen } from "screens/detail.slint";
import { NowPlayingScreen } from "screens/now_playing.slint";
import { IdleOverlay } from "screens/idle.slint";
import { AuthNeededScreen } from "screens/auth_needed.slint";

export { TileData, TrackData, ScreenKind, LoadState }

export component AppWindow inherits Window {
    width: 800px;
    height: 480px;
    title: "Muzak";
    background: Theme.bg;
    default-font-family: "Nunito";

    in property <ScreenKind> screen;
    in property <int> section;
    in property <string> grid-title;
    in property <[TileData]> tiles;
    in property <LoadState> grid-state;
    in property <TileData> detail;
    in property <[TrackData]> detail-tracks;
    in property <LoadState> detail-state;
    in property <bool> mini-visible;
    in property <string> track-title;
    in property <string> track-artists;
    in property <image> track-art;
    in property <bool> track-has-art;
    in property <bool> playing;
    in property <bool> loading;
    in property <float> progress;
    in property <string> position-text;
    in property <string> duration-text;
    in property <int> volume;
    in property <bool> shuffle;
    in property <int> repeat;
    in property <string> banner;
    in property <int> display-mode;
    in property <string> clock;
    in property <bool> auth-needed;

    callback show-section(int);
    callback open-collection(string);
    callback back();
    callback open-now-playing();
    callback play-collection(string, bool);
    callback play-track(string, int);
    callback toggle-play();
    callback next();
    callback previous();
    callback seek(float);
    callback set-volume(int);
    callback toggle-shuffle();
    callback cycle-repeat();
    callback touched();

    property <length> content-height: root.mini-visible ? root.height - Theme.mini-height : root.height;

    Rail {
        x: 0;
        y: 0;
        height: root.height;
        selected: root.section;
        select(i) => { root.show-section(i); }
    }
    GridScreen {
        x: Theme.rail-width;
        y: 0;
        width: root.width - Theme.rail-width;
        height: root.content-height;
        visible: root.screen == ScreenKind.grid;
        title: root.grid-title;
        tiles: root.tiles;
        state: root.grid-state;
        open(uri) => { root.open-collection(uri); }
    }
    DetailScreen {
        x: Theme.rail-width;
        y: 0;
        width: root.width - Theme.rail-width;
        height: root.content-height;
        visible: root.screen == ScreenKind.detail;
        header: root.detail;
        tracks: root.detail-tracks;
        state: root.detail-state;
        back => { root.back(); }
        play(shuffle) => { root.play-collection(root.detail.uri, shuffle); }
        play-track(i) => { root.play-track(root.detail.uri, i); }
    }
    if root.mini-visible: MiniPlayer {
        x: Theme.rail-width;
        y: root.height - Theme.mini-height;
        width: root.width - Theme.rail-width;
        title: root.track-title;
        artists: root.track-artists;
        art: root.track-art;
        has-art: root.track-has-art;
        playing: root.playing;
        open => { root.open-now-playing(); }
        toggle-play => { root.toggle-play(); }
    }
    NowPlayingScreen {
        x: 0;
        y: 0;
        width: root.width;
        height: root.height;
        visible: root.screen == ScreenKind.now-playing;
        title: root.track-title;
        artists: root.track-artists;
        art: root.track-art;
        has-art: root.track-has-art;
        playing: root.playing;
        loading: root.loading;
        progress: root.progress;
        position-text: root.position-text;
        duration-text: root.duration-text;
        volume: root.volume;
        shuffle: root.shuffle;
        repeat: root.repeat;
        back => { root.back(); }
        toggle-play => { root.toggle-play(); }
        next => { root.next(); }
        previous => { root.previous(); }
        seek(v) => { root.seek(v); }
        set-volume(v) => { root.set-volume(v); }
        toggle-shuffle => { root.toggle-shuffle(); }
        cycle-repeat => { root.cycle-repeat(); }
    }
    if root.banner != "": Banner {
        x: (root.width - self.width) / 2;
        y: 12px;
        text: root.banner;
    }
    if root.auth-needed: AuthNeededScreen {
        x: 0;
        y: 0;
        width: root.width;
        height: root.height;
    }
    if root.display-mode != 0: IdleOverlay {
        x: 0;
        y: 0;
        width: root.width;
        height: root.height;
        mode: root.display-mode;
        clock: root.clock;
        touched => { root.touched(); }
    }
}
```

Run: `cargo build -p muzak-player`
Expected: builds. If the Slint compiler reports an error, fix the `.slint` file it names. The messages give the file, line and the property involved.

- [ ] **Step 6: Bridge**

`src/ui_bridge.rs`:
```rust
//! Glue between the runtime (any thread) and the Slint window (UI thread).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use image::RgbaImage;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use tokio::sync::mpsc::UnboundedSender;

use crate::app::{AppState, DisplayMode, Input, UiAction};
use crate::model::Section;
use crate::view::{self, LoadStatus, ScreenView, TileView, TrackRowView};
use crate::{AppWindow, LoadState, ScreenKind, TileData, TrackData};

const IMAGE_CACHE_LIMIT: usize = 80;

struct Images {
    ready: HashMap<String, Image>,
    order: VecDeque<String>,
    requested: HashSet<String>,
    requests: UnboundedSender<String>,
}

impl Images {
    fn get(&mut self, url: &Option<String>) -> Option<Image> {
        let url = url.as_ref()?;
        if let Some(image) = self.ready.get(url) {
            return Some(image.clone());
        }
        if self.requested.insert(url.clone()) {
            let _ = self.requests.send(url.clone());
        }
        None
    }

    fn insert(&mut self, url: String, image: Image) {
        self.ready.insert(url.clone(), image);
        self.order.push_back(url);
        while self.order.len() > IMAGE_CACHE_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.ready.remove(&old);
                self.requested.remove(&old);
            }
        }
    }
}

struct Bridge {
    window: slint::Weak<AppWindow>,
    images: Images,
    tiles: Rc<VecModel<TileData>>,
    tracks: Rc<VecModel<TrackData>>,
    last: Option<AppState>,
}

thread_local! {
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

fn with_bridge<R>(f: impl FnOnce(&mut Bridge) -> R) -> Option<R> {
    BRIDGE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

/// Call once on the UI thread before `window.run()`.
pub fn install(window: &AppWindow, image_requests: UnboundedSender<String>) {
    let tiles = Rc::new(VecModel::<TileData>::default());
    let tracks = Rc::new(VecModel::<TrackData>::default());
    window.set_tiles(ModelRc::from(tiles.clone()));
    window.set_detail_tracks(ModelRc::from(tracks.clone()));
    let bridge = Bridge {
        window: window.as_weak(),
        images: Images { ready: HashMap::new(), order: VecDeque::new(), requested: HashSet::new(), requests: image_requests },
        tiles,
        tracks,
        last: None,
    };
    BRIDGE.with(|cell| *cell.borrow_mut() = Some(bridge));
}

/// Called from the runtime thread with each new state.
pub fn publish(state: AppState) {
    let _ = slint::invoke_from_event_loop(move || {
        with_bridge(|b| {
            b.last = Some(state);
            b.render();
        });
    });
}

/// Called from the image loader with a decoded cover.
pub fn deliver_image(url: String, image: Option<RgbaImage>) {
    let Some(image) = image else { return };
    let (width, height) = image.dimensions();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(image.as_raw(), width, height);
    let _ = slint::invoke_from_event_loop(move || {
        with_bridge(|b| {
            b.images.insert(url, Image::from_rgba8(buffer));
            b.render();
        });
    });
}

pub fn wire_callbacks(window: &AppWindow, inputs: UnboundedSender<Input>) {
    let send = move |action: UiAction| {
        let _ = inputs.send(Input::Ui(action));
    };
    let s = send.clone();
    window.on_show_section(move |i| s(UiAction::ShowSection(Section::from_index(i))));
    let s = send.clone();
    window.on_open_collection(move |uri| s(UiAction::OpenCollection(uri.to_string())));
    let s = send.clone();
    window.on_back(move || s(UiAction::Back));
    let s = send.clone();
    window.on_open_now_playing(move || s(UiAction::OpenNowPlaying));
    let s = send.clone();
    window.on_play_collection(move |uri, shuffle| s(UiAction::PlayCollection { uri: uri.to_string(), shuffle }));
    let s = send.clone();
    window.on_play_track(move |uri, index| {
        s(UiAction::PlayTrack { collection_uri: uri.to_string(), index: index.max(0) as usize })
    });
    let s = send.clone();
    window.on_toggle_play(move || s(UiAction::TogglePlay));
    let s = send.clone();
    window.on_next(move || s(UiAction::Next));
    let s = send.clone();
    window.on_previous(move || s(UiAction::Previous));
    let s = send.clone();
    window.on_seek(move |fraction| {
        let duration = with_bridge(|b| {
            b.last.as_ref().and_then(|st| st.playback.track.as_ref()).map(|t| t.duration_ms)
        })
        .flatten()
        .unwrap_or(0);
        s(UiAction::Seek { position_ms: (fraction.clamp(0.0, 1.0) * duration as f32) as u32 });
    });
    let s = send.clone();
    window.on_set_volume(move |percent| s(UiAction::SetVolume { percent: percent.clamp(0, 100) as u8 }));
    let s = send.clone();
    window.on_toggle_shuffle(move || s(UiAction::ToggleShuffle));
    let s = send.clone();
    window.on_cycle_repeat(move || s(UiAction::CycleRepeat));
    let s = send;
    window.on_touched(move || s(UiAction::Touch));
}

impl Bridge {
    fn render(&mut self) {
        let Some(state) = self.last.as_ref() else { return };
        let Some(w) = self.window.upgrade() else { return };
        let v = view::build(state);

        w.set_screen(match v.screen {
            ScreenView::Grid => ScreenKind::Grid,
            ScreenView::Detail => ScreenKind::Detail,
            ScreenView::NowPlaying => ScreenKind::NowPlaying,
        });
        w.set_section(v.section.index());
        w.set_grid_title(v.grid_title.as_str().into());
        let tiles: Vec<TileData> = v.grid.iter().map(|t| tile_data(&mut self.images, t)).collect();
        sync(&self.tiles, tiles);
        w.set_grid_state(load_state(v.grid_status));

        if let Some(detail) = &v.detail {
            w.set_detail(tile_data(&mut self.images, &detail.header));
            sync(&self.tracks, detail.tracks.iter().map(track_data).collect());
            w.set_detail_state(load_state(detail.status));
        }

        let art = self.images.get(&v.now.image_url);
        w.set_mini_visible(v.mini_visible);
        w.set_track_title(v.now.title.as_str().into());
        w.set_track_artists(v.now.artists.as_str().into());
        w.set_track_has_art(art.is_some());
        w.set_track_art(art.unwrap_or_default());
        w.set_playing(v.now.playing);
        w.set_loading(v.now.loading);
        w.set_progress(v.now.progress);
        w.set_position_text(v.now.position.as_str().into());
        w.set_duration_text(v.now.duration.as_str().into());
        w.set_volume(v.now.volume as i32);
        w.set_shuffle(v.now.shuffle);
        w.set_repeat(v.now.repeat.index());
        w.set_banner(v.banner.unwrap_or_default().into());
        w.set_display_mode(match v.display {
            DisplayMode::Active => 0,
            DisplayMode::Dim => 1,
            DisplayMode::Off => 2,
        });
        w.set_clock(chrono::Local::now().format("%-I:%M").to_string().into());
        w.set_auth_needed(v.auth_needed);
    }
}

fn tile_data(images: &mut Images, t: &TileView) -> TileData {
    let image = images.get(&t.image_url);
    TileData {
        uri: t.uri.as_str().into(),
        title: t.title.as_str().into(),
        subtitle: t.subtitle.as_str().into(),
        has_image: image.is_some(),
        image: image.unwrap_or_default(),
    }
}

fn track_data(t: &TrackRowView) -> TrackData {
    TrackData {
        title: t.title.as_str().into(),
        artists: t.artists.as_str().into(),
        duration: t.duration.as_str().into(),
        current: t.current,
    }
}

fn load_state(status: LoadStatus) -> LoadState {
    match status {
        LoadStatus::Loading => LoadState::Loading,
        LoadStatus::Empty => LoadState::Empty,
        LoadStatus::Failed => LoadState::Failed,
        LoadStatus::Ready => LoadState::Ready,
    }
}

/// Updates only changed rows so scroll positions and images don't reset.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() != rows.len() {
        model.set_vec(rows);
        return;
    }
    for (i, row) in rows.into_iter().enumerate() {
        if model.row_data(i).as_ref() != Some(&row) {
            model.set_row_data(i, row);
        }
    }
}
```

Add `pub mod ui_bridge;` to `lib.rs`.

- [ ] **Step 7: Real `main.rs`**

Replace `src/main.rs`:
```rust
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use muzak_player::config::Config;
use muzak_player::{AppWindow, runtime, ui_bridge};
use slint::ComponentHandle;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(about = "Muzak touchscreen Spotify player")]
struct Args {
    /// Path to the device config.
    #[arg(long, default_value = "dev/config.toml")]
    config: PathBuf,
    /// Use the built-in fake library and player instead of Spotify.
    #[arg(long)]
    fake: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,librespot=warn")))
        .init();
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let window = AppWindow::new()?;
    let (image_tx, image_rx) = tokio::sync::mpsc::unbounded_channel();
    ui_bridge::install(&window, image_tx);
    let inputs = runtime::start(
        config,
        args.fake,
        image_rx,
        Box::new(ui_bridge::publish),
        Arc::new(ui_bridge::deliver_image),
    )?;
    ui_bridge::wire_callbacks(&window, inputs);
    window.run()?;
    Ok(())
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test -p muzak-player`
Expected: all tests pass.

- [ ] **Step 9: Manual check in fake mode**

Run: `cargo run -p muzak-player -- --fake`. Set `dim_after_secs = 10` and `off_after_secs = 20` in `dev/config.toml` for the idle check, then remove them afterwards.

Expected:
- [ ] "Loading…" appears briefly, then six playlist tiles. The long name is cut off with "…".
- [ ] Rail Albums, Liked and Recent each switch the content. Liked opens a track list directly.
- [ ] Tapping a tile opens its track list. Play opens Now Playing on "… Song 1". Shuffle starts on the last song.
- [ ] The progress bar moves every second. Dragging it seeks. Next and previous work. Volume drag works.
- [ ] Back returns to the track list, and the mini player shows at the bottom. Tapping the mini player reopens Now Playing.
- [ ] Shuffle and repeat toggles highlight. Repeat shows a "1" on the third tap.
- [ ] After 10s without touch, with playback paused, the clock overlay appears. After 20s the screen goes black. A tap wakes it.
- [ ] A screenshot (`screencapture -l$(osascript -e 'tell app "System Events" to id of window 1 of process "muzak-player"') /tmp/muzak.png`) shows no clipped controls at 800×480.

- [ ] **Step 10: Commit**

```bash
git add crates/muzak-player
git commit -m "feat: add Slint touchscreen UI and bridge"
```

---

### Task 10: `muzak-setup` sign-in and Web API probe

**Files:**
- Replace: `crates/muzak-setup/Cargo.toml`, `crates/muzak-setup/src/main.rs`

**Interfaces:**
- Produces:
  - **CLI:**
    - `muzak-setup auth --state-dir <dir>` writes `<dir>/librespot/credentials.json`
    - `muzak-setup probe --state-dir <dir>` exits 0 if every endpoint works with librespot tokens
  - **Credentials format:** librespot's own `credentials.json`. `player::librespot` reads it in Task 11 through `Cache::credentials()`.

- [ ] **Step 1: Manifest**

```toml
[package]
name = "muzak-setup"
version.workspace = true
edition.workspace = true

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
librespot = { workspace = true }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

- [ ] **Step 2: Implement `main.rs`**

```rust
//! Mac-side setup: sign a kid's Spotify account in and check Web API access.

use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow, bail};
use clap::{Parser, Subcommand};
use librespot::core::authentication::Credentials;
use librespot::core::cache::Cache;
use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::oauth::OAuthClientBuilder;

const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
/// Keep in sync with `crates/muzak-player/src/library/web_api.rs`.
const SCOPES: &str =
    "playlist-read-private,playlist-read-collaborative,user-library-read,user-read-recently-played";
const API: &str = "https://api.spotify.com/v1";

#[derive(Parser)]
#[command(about = "Muzak setup tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in to Spotify in the browser and save device credentials.
    Auth {
        #[arg(long)]
        state_dir: PathBuf,
    },
    /// Check the saved credentials can read every Web API endpoint the player uses.
    Probe {
        #[arg(long)]
        state_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Auth { state_dir } => auth(&state_dir).await,
        Command::Probe { state_dir } => probe(&state_dir).await,
    }
}

fn cache(state_dir: &Path) -> anyhow::Result<(PathBuf, Cache)> {
    let dir = state_dir.join("librespot");
    std::fs::create_dir_all(&dir)?;
    let cache = Cache::new(Some(&dir), Some(&dir), None, None)?;
    Ok((dir, cache))
}

async fn auth(state_dir: &Path) -> anyhow::Result<()> {
    let session_config = SessionConfig::default();
    let client_id = session_config.client_id.clone();
    println!("Opening the Spotify sign-in page. Sign in with the kid's account.");
    let token = tokio::task::spawn_blocking(move || {
        OAuthClientBuilder::new(&client_id, REDIRECT_URI, vec!["streaming"])
            .open_in_browser()
            .build()?
            .get_access_token()
    })
    .await?
    .map_err(|e| anyhow!("Spotify sign-in failed: {e}"))?;

    let (dir, cache) = cache(state_dir)?;
    let session = Session::new(session_config, Some(cache));
    session.connect(Credentials::with_access_token(token.access_token), true).await?;
    println!("Signed in as {}. Saved {}", session.username(), dir.join("credentials.json").display());
    Ok(())
}

async fn probe(state_dir: &Path) -> anyhow::Result<()> {
    let (_, cache) = cache(state_dir)?;
    let credentials = cache.credentials().context("no saved credentials; run `muzak-setup auth` first")?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;
    println!("Connected as {}", session.username());
    let token = session.token_provider().get_token(SCOPES).await?;
    let client = reqwest::Client::new();

    let get = |path: String| {
        let request = client.get(format!("{API}{path}")).bearer_auth(&token.access_token);
        async move {
            let response = request.send().await?;
            let status = response.status();
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            anyhow::Ok((status, body))
        }
    };

    let mut failures = 0;
    let mut first_playlist = None;
    for (label, path) in [
        ("playlists", "/me/playlists?limit=5"),
        ("saved albums", "/me/albums?limit=5"),
        ("liked songs", "/me/tracks?limit=5"),
        ("recently played", "/me/player/recently-played?limit=5"),
    ] {
        let (status, body) = get(path.to_string()).await?;
        let count = body["items"].as_array().map_or(0, |a| a.len());
        println!("{label:<16} HTTP {status}  items: {count}");
        if !status.is_success() {
            failures += 1;
        }
        if label == "playlists" {
            first_playlist = body["items"][0]["id"].as_str().map(str::to_string);
        }
    }
    if let Some(id) = first_playlist {
        let mut any_ok = false;
        for path in [format!("/playlists/{id}/items?limit=5"), format!("/playlists/{id}/tracks?limit=5")] {
            let (status, _) = get(path.clone()).await?;
            println!("{:<16} HTTP {status}  {path}", "playlist items");
            any_ok |= status.is_success();
        }
        if !any_ok {
            failures += 1;
        }
    }
    if failures > 0 {
        bail!("{failures} endpoint group(s) failed; apply Contingency A in the phase 1 plan");
    }
    println!("All endpoints work with librespot tokens.");
    Ok(())
}
```

- [ ] **Step 3: Build**

Run: `cargo build -p muzak-setup`
Expected: builds.

- [ ] **Step 4: Sign in and probe (needs the parent at the keyboard)**

Run:
```bash
cargo run -p muzak-setup -- auth --state-dir secrets/dev
cargo run -p muzak-setup -- probe --state-dir secrets/dev
```
Expected: the browser opens, the parent signs in with a kid's account, and `secrets/dev/librespot/credentials.json` exists. Probe prints HTTP 200 lines and "All endpoints work with librespot tokens."

Record the result in the commit message. If probe fails, carry out **Contingency A** (end of this plan) before Task 11.

- [ ] **Step 5: Commit**

```bash
git add crates/muzak-setup Cargo.lock
git commit -m "feat: add muzak-setup auth and probe commands"
```

---

### Task 11: librespot player and real-mode wiring

**Files:**
- Create: `crates/muzak-player/src/player/librespot.rs`
- Create: `crates/muzak-player/src/library/session_tokens.rs`
- Modify: `crates/muzak-player/src/player/mod.rs` (add `pub mod librespot;`)
- Modify: `crates/muzak-player/src/library/mod.rs` (add `pub mod session_tokens;`)
- Modify: `crates/muzak-player/src/runtime.rs` (real branch)

**Interfaces:**
- Consumes:
  - `PlayerCommand`, `PlayerUpdate`, `Input` (Task 2)
  - `percent_to_volume`, `volume_to_percent` (Task 8)
  - `TokenSource`, `WebApi`, `ReqwestHttp`, `SCOPES` (Task 5)
  - `Config` (Task 1)
- Produces:
  - **`player::librespot`:**
    - `PlayerSettings { device_name, backend, device: Option<String>, credentials_dir: PathBuf, initial_volume: u8 }`, with `PlayerSettings::from_config(&Config)`
    - `spawn(PlayerSettings, watch::Sender<Option<Session>>, UnboundedSender<Input>) -> UnboundedSender<PlayerCommand>`
    - `map_event(PlayerEvent) -> Option<PlayerUpdate>`
  - **`library::session_tokens`:** `SessionTokens::new(watch::Receiver<Option<Session>>)`, which implements `TokenSource`

- [ ] **Step 1: Write the failing event-mapping tests**

`player/librespot.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use librespot::core::SpotifyUri;

    use super::*;
    use crate::model::Repeat;

    fn uri() -> SpotifyUri {
        SpotifyUri::from_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").unwrap()
    }

    #[test]
    fn maps_playback_events() {
        assert_eq!(
            map_event(PlayerEvent::Playing { play_request_id: 1, track_id: uri(), position_ms: 42 }),
            Some(PlayerUpdate::Playing { position_ms: 42 })
        );
        assert_eq!(
            map_event(PlayerEvent::Paused { play_request_id: 1, track_id: uri(), position_ms: 7 }),
            Some(PlayerUpdate::Paused { position_ms: 7 })
        );
        assert_eq!(
            map_event(PlayerEvent::PositionChanged { play_request_id: 1, track_id: uri(), position_ms: 9 }),
            Some(PlayerUpdate::Position { position_ms: 9 })
        );
        assert_eq!(
            map_event(PlayerEvent::Unavailable { play_request_id: 1, track_id: uri() }),
            Some(PlayerUpdate::Unavailable)
        );
        assert_eq!(map_event(PlayerEvent::Preloading { track_id: uri() }), None);
    }

    #[test]
    fn maps_settings_events() {
        assert_eq!(map_event(PlayerEvent::VolumeChanged { volume: u16::MAX }), Some(PlayerUpdate::Volume { percent: 100 }));
        assert_eq!(map_event(PlayerEvent::ShuffleChanged { shuffle: true }), Some(PlayerUpdate::Shuffle(true)));
        assert_eq!(
            map_event(PlayerEvent::RepeatChanged { context: true, track: false }),
            Some(PlayerUpdate::Repeat(Repeat::Context))
        );
        assert_eq!(
            map_event(PlayerEvent::RepeatChanged { context: true, track: true }),
            Some(PlayerUpdate::Repeat(Repeat::Track))
        );
    }

    #[test]
    fn liked_uri_maps_to_user_collection() {
        assert_eq!(context_uri_for(LIKED_URI, "kid1"), "spotify:user:kid1:collection");
        assert_eq!(context_uri_for("spotify:album:x", "kid1"), "spotify:album:x");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod librespot;` to `player/mod.rs`.
Run: `cargo test -p muzak-player player::librespot`
Expected: compile errors.

- [ ] **Step 3: Implement `player/librespot.rs`**

Put this above the tests:
```rust
//! Plays Spotify through an embedded librespot Connect device (Spirc).

use std::path::PathBuf;
use std::time::Duration;

use librespot::connect::{ConnectConfig, LoadContextOptions, LoadRequest, LoadRequestOptions, Options, PlayingTrack, Spirc};
use librespot::core::cache::Cache;
use librespot::core::config::SessionConfig;
use librespot::core::error::ErrorKind;
use librespot::core::session::Session;
use librespot::core::Error as SpotifyError;
use librespot::metadata::audio::{AudioItem, UniqueFields};
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, Bitrate, PlayerConfig};
use librespot::playback::mixer::{self, MixerConfig};
use librespot::playback::player::{Player, PlayerEvent};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::watch;

use super::{percent_to_volume, volume_to_percent};
use crate::app::{Input, PlayerCommand, PlayerUpdate};
use crate::config::Config;
use crate::model::{LIKED_URI, Repeat, Track};

#[derive(Debug, Clone)]
pub struct PlayerSettings {
    pub device_name: String,
    pub backend: String,
    pub device: Option<String>,
    pub credentials_dir: PathBuf,
    pub initial_volume: u8,
}

impl PlayerSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            device_name: config.device_name.clone(),
            backend: config.audio_backend.clone(),
            device: config.audio_device.clone(),
            credentials_dir: config.librespot_dir(),
            initial_volume: config.initial_volume,
        }
    }
}

enum Exit {
    Disconnected,
    CommandsClosed,
}

enum Failure {
    Auth,
    Other(String),
}

impl From<SpotifyError> for Failure {
    fn from(e: SpotifyError) -> Self {
        match e.kind {
            ErrorKind::Unauthenticated | ErrorKind::PermissionDenied => Failure::Auth,
            _ => Failure::Other(e.to_string()),
        }
    }
}

pub fn spawn(
    settings: PlayerSettings,
    session_tx: watch::Sender<Option<Session>>,
    inputs: UnboundedSender<Input>,
) -> UnboundedSender<PlayerCommand> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run(settings, session_tx, inputs, rx));
    tx
}

async fn run(
    settings: PlayerSettings,
    session_tx: watch::Sender<Option<Session>>,
    inputs: UnboundedSender<Input>,
    mut commands: UnboundedReceiver<PlayerCommand>,
) {
    let mut backoff = Duration::from_secs(2);
    loop {
        // Commands queued while disconnected are stale.
        while commands.try_recv().is_ok() {}
        match connect_and_serve(&settings, &session_tx, &inputs, &mut commands).await {
            Ok(Exit::CommandsClosed) => return,
            Ok(Exit::Disconnected) => {
                tracing::warn!("Spotify connection ended; reconnecting");
                backoff = Duration::from_secs(2);
            }
            Err(Failure::Auth) => {
                tracing::error!("Spotify credentials rejected; rerun muzak-setup auth");
                let _ = inputs.send(Input::AuthInvalid);
                return;
            }
            Err(Failure::Other(e)) => tracing::warn!("Spotify connection failed: {e}"),
        }
        session_tx.send_replace(None);
        let _ = inputs.send(Input::Player(PlayerUpdate::Disconnected));
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

async fn connect_and_serve(
    settings: &PlayerSettings,
    session_tx: &watch::Sender<Option<Session>>,
    inputs: &UnboundedSender<Input>,
    commands: &mut UnboundedReceiver<PlayerCommand>,
) -> Result<Exit, Failure> {
    let dir = &settings.credentials_dir;
    let cache = Cache::new(Some(dir), Some(dir), None, None)?;
    let credentials = cache.credentials().ok_or(Failure::Auth)?;
    let session = Session::new(SessionConfig::default(), Some(cache));

    let mixer_builder = mixer::find(None).ok_or_else(|| Failure::Other("no mixer".into()))?;
    let mixer = mixer_builder(MixerConfig::default())?;
    let sink_builder = audio_backend::find(Some(settings.backend.clone()))
        .ok_or_else(|| Failure::Other(format!("unknown audio backend {}", settings.backend)))?;
    let device = settings.device.clone();
    let player_config = PlayerConfig {
        bitrate: Bitrate::Bitrate160,
        position_update_interval: Some(Duration::from_secs(1)),
        ..Default::default()
    };
    let player = Player::new(player_config, session.clone(), mixer.get_soft_volume(), move || {
        sink_builder(device, AudioFormat::default())
    });
    let mut events = player.get_player_event_channel();
    let connect_config = ConnectConfig {
        name: settings.device_name.clone(),
        initial_volume: percent_to_volume(settings.initial_volume),
        ..Default::default()
    };

    let (spirc, spirc_task) = Spirc::new(connect_config, session.clone(), credentials, player, mixer).await?;
    session_tx.send_replace(Some(session.clone()));
    let _ = inputs.send(Input::Player(PlayerUpdate::Connected));
    tokio::pin!(spirc_task);

    loop {
        tokio::select! {
            _ = &mut spirc_task => return Ok(Exit::Disconnected),
            command = commands.recv() => match command {
                Some(command) => {
                    if let Err(e) = apply(&spirc, &session, command) {
                        tracing::warn!("player command failed: {e}");
                    }
                }
                None => {
                    let _ = spirc.shutdown();
                    return Ok(Exit::CommandsClosed);
                }
            },
            event = events.recv() => match event {
                Some(event) => {
                    if let Some(update) = map_event(event) {
                        let _ = inputs.send(Input::Player(update));
                    }
                }
                None => return Ok(Exit::Disconnected),
            },
        }
    }
}

pub(crate) fn context_uri_for(context_uri: &str, username: &str) -> String {
    if context_uri == LIKED_URI {
        format!("spotify:user:{username}:collection")
    } else {
        context_uri.to_string()
    }
}

fn apply(spirc: &Spirc, session: &Session, command: PlayerCommand) -> Result<(), SpotifyError> {
    match command {
        PlayerCommand::Load { context_uri, start_index, shuffle } => {
            let uri = context_uri_for(&context_uri, &session.username());
            spirc.activate()?;
            spirc.load(LoadRequest::from_context_uri(
                uri,
                LoadRequestOptions {
                    start_playing: true,
                    seek_to: 0,
                    context_options: Some(LoadContextOptions::Options(Options {
                        shuffle,
                        repeat: false,
                        repeat_track: false,
                    })),
                    playing_track: start_index.map(PlayingTrack::Index),
                },
            ))
        }
        PlayerCommand::Play => spirc.play(),
        PlayerCommand::Pause => spirc.pause(),
        PlayerCommand::Next => spirc.next(),
        PlayerCommand::Previous => spirc.prev(),
        PlayerCommand::Seek { position_ms } => spirc.set_position_ms(position_ms),
        PlayerCommand::SetVolume { percent } => spirc.set_volume(percent_to_volume(percent)),
        PlayerCommand::SetShuffle(shuffle) => spirc.shuffle(shuffle),
        PlayerCommand::SetRepeat(repeat) => {
            spirc.repeat(repeat != Repeat::Off)?;
            spirc.repeat_track(repeat == Repeat::Track)
        }
    }
}

pub fn map_event(event: PlayerEvent) -> Option<PlayerUpdate> {
    Some(match event {
        PlayerEvent::TrackChanged { audio_item } => PlayerUpdate::TrackChanged(track_from_item(&audio_item)),
        PlayerEvent::Loading { .. } => PlayerUpdate::Loading,
        PlayerEvent::Playing { position_ms, .. } => PlayerUpdate::Playing { position_ms },
        PlayerEvent::Paused { position_ms, .. } => PlayerUpdate::Paused { position_ms },
        PlayerEvent::Stopped { .. } => PlayerUpdate::Stopped,
        PlayerEvent::PositionChanged { position_ms, .. }
        | PlayerEvent::Seeked { position_ms, .. }
        | PlayerEvent::PositionCorrection { position_ms, .. } => PlayerUpdate::Position { position_ms },
        PlayerEvent::VolumeChanged { volume } => PlayerUpdate::Volume { percent: volume_to_percent(volume) },
        PlayerEvent::ShuffleChanged { shuffle } => PlayerUpdate::Shuffle(shuffle),
        PlayerEvent::RepeatChanged { context, track } => PlayerUpdate::Repeat(if track {
            Repeat::Track
        } else if context {
            Repeat::Context
        } else {
            Repeat::Off
        }),
        PlayerEvent::Unavailable { .. } => PlayerUpdate::Unavailable,
        PlayerEvent::SessionConnected { .. } => PlayerUpdate::Connected,
        PlayerEvent::SessionDisconnected { .. } => PlayerUpdate::Disconnected,
        _ => return None,
    })
}

fn track_from_item(item: &AudioItem) -> Track {
    let (artists, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => {
            (artists.iter().map(|a| a.name.clone()).collect::<Vec<_>>().join(", "), album.clone())
        }
        UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new()),
        UniqueFields::Local { artists, album, .. } => {
            (artists.clone().unwrap_or_default(), album.clone().unwrap_or_default())
        }
    };
    let image_url = item
        .covers
        .iter()
        .filter(|c| c.width >= 250)
        .min_by_key(|c| c.width)
        .or_else(|| item.covers.iter().max_by_key(|c| c.width))
        .map(|c| c.url.clone());
    Track {
        uri: item.uri.clone(),
        name: item.name.clone(),
        artists,
        album,
        image_url,
        duration_ms: item.duration_ms,
    }
}
```

- [ ] **Step 4: Implement `library/session_tokens.rs`**

```rust
//! Web API access tokens minted by the librespot session.

use std::time::Duration;

use librespot::core::session::Session;
use tokio::sync::watch;

use super::FetchError;
use super::web_api::{SCOPES, TokenSource};

pub struct SessionTokens {
    session: watch::Receiver<Option<Session>>,
}

impl SessionTokens {
    pub fn new(session: watch::Receiver<Option<Session>>) -> Self {
        Self { session }
    }
}

impl TokenSource for SessionTokens {
    async fn token(&self) -> Result<String, FetchError> {
        let mut rx = self.session.clone();
        let session = {
            let waited = tokio::time::timeout(Duration::from_secs(20), rx.wait_for(|s| s.is_some())).await;
            match waited {
                Ok(Ok(guard)) => guard.clone(),
                _ => None,
            }
        };
        let Some(session) = session else { return Err(FetchError::Offline) };
        session.token_provider().get_token(SCOPES).await.map(|t| t.access_token).map_err(|e| {
            tracing::warn!("token request failed: {e}");
            FetchError::Offline
        })
    }
}
```
Add `pub mod session_tokens;` to `library/mod.rs`.

- [ ] **Step 5: Wire the real branch in `runtime.rs`**

Replace the `else { anyhow::bail!(...) }` branch in `run` with:
```rust
    } else {
        let (session_tx, session_rx) = tokio::sync::watch::channel(None);
        let player = crate::player::librespot::spawn(
            crate::player::librespot::PlayerSettings::from_config(&config),
            session_tx,
            inputs.clone(),
        );
        let source = Arc::new(crate::library::web_api::WebApi::new(
            crate::library::web_api::ReqwestHttp::new()?,
            crate::library::session_tokens::SessionTokens::new(session_rx),
        ));
        (player, spawn_library(source, cache, inputs.clone()))
    };
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p muzak-player`
Expected: all tests pass.

- [ ] **Step 7: Manual check with the real account on the Mac**

```bash
mkdir -p dev/state/librespot
cp secrets/dev/librespot/credentials.json dev/state/librespot/
cargo run -p muzak-player
```
Expected:
- [ ] The kid's real playlists and albums appear with cover art. Recent and Liked fill in.
- [ ] Play on a playlist plays audio through the Mac speakers within about a second. The title and artist match.
- [ ] Pause, next, previous, seek, volume, shuffle and repeat all take effect. The Spotify app on a phone shows the "Muzak Dev" device playing.
- [ ] Turning Wi-Fi off: cached grids still show and Play shows "No internet right now". Turning Wi-Fi back on: within about a minute the library refreshes and playback works again.
- [ ] Moving `credentials.json` away and restarting shows "Ask a grown-up for help".

- [ ] **Step 8: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: play Spotify through embedded librespot"
```

---

### Task 12: Platform: backlight and Bluetooth speaker monitor

**Files:**
- Create: `crates/muzak-player/src/platform.rs`
- Modify: `crates/muzak-player/src/lib.rs` (add `pub mod platform;`)
- Modify: `crates/muzak-player/src/runtime.rs` (start `Platform`, route `Effect::Display`)

**Interfaces:**
- Consumes: `Config`, `Input`, `DisplayMode`.
- Produces:
  - `Platform::start(&Config, UnboundedSender<Input>) -> Platform`
  - `Platform::set_display(&self, DisplayMode)`
  - `brightness_for(DisplayMode, max: u32) -> u32`
  - `parse_connected(&str) -> bool`

- [ ] **Step 1: Write the failing tests**

`platform.rs` test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_levels() {
        assert_eq!(brightness_for(DisplayMode::Active, 255), 255);
        assert_eq!(brightness_for(DisplayMode::Dim, 255), 38);
        assert_eq!(brightness_for(DisplayMode::Dim, 5), 1);
        assert_eq!(brightness_for(DisplayMode::Off, 255), 0);
    }

    #[test]
    fn parses_bluetoothctl_info() {
        let out = "Device AA:BB:CC:DD:EE:FF (public)\n\tName: Speaker\n\tPaired: yes\n\tConnected: yes\n";
        assert!(parse_connected(out));
        assert!(!parse_connected(&out.replace("Connected: yes", "Connected: no")));
        assert!(!parse_connected(""));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `pub mod platform;` to `lib.rs`.
Run: `cargo test -p muzak-player platform`
Expected: compile errors.

- [ ] **Step 3: Implement `platform.rs`**

Put this above the tests:
```rust
//! Hardware bits: DSI backlight and a Bluetooth speaker. Both are no-ops on the Mac.

use std::path::PathBuf;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;

use crate::app::{DisplayMode, Input};
use crate::config::Config;

pub struct Platform {
    backlight: Option<Backlight>,
}

impl Platform {
    /// Must be called inside the tokio runtime.
    pub fn start(config: &Config, inputs: UnboundedSender<Input>) -> Self {
        if let Some(address) = config.bluetooth_speaker.clone() {
            tokio::spawn(monitor_speaker(address, inputs));
        }
        let backlight = Backlight::detect();
        match &backlight {
            Some(b) => tracing::info!("backlight at {}", b.brightness.display()),
            None => tracing::info!("no backlight control; the UI overlay handles dimming"),
        }
        Self { backlight }
    }

    pub fn set_display(&self, mode: DisplayMode) {
        if let Some(backlight) = &self.backlight {
            backlight.set(mode);
        }
    }
}

pub fn brightness_for(mode: DisplayMode, max: u32) -> u32 {
    match mode {
        DisplayMode::Active => max,
        DisplayMode::Dim => (max * 15 / 100).max(1),
        DisplayMode::Off => 0,
    }
}

struct Backlight {
    brightness: PathBuf,
    max: u32,
}

impl Backlight {
    fn detect() -> Option<Self> {
        let dir = std::fs::read_dir("/sys/class/backlight").ok()?.flatten().next()?.path();
        let max = std::fs::read_to_string(dir.join("max_brightness")).ok()?.trim().parse().ok()?;
        Some(Self { brightness: dir.join("brightness"), max })
    }

    fn set(&self, mode: DisplayMode) {
        let value = brightness_for(mode, self.max).to_string();
        if let Err(e) = std::fs::write(&self.brightness, value) {
            tracing::warn!("setting backlight failed: {e}");
        }
    }
}

pub fn parse_connected(bluetoothctl_info: &str) -> bool {
    bluetoothctl_info.lines().any(|line| line.trim() == "Connected: yes")
}

async fn bluetoothctl(args: &[&str]) -> Option<String> {
    let run = Command::new("bluetoothctl").args(args).output();
    match tokio::time::timeout(Duration::from_secs(10), run).await {
        Ok(Ok(output)) => Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        _ => None,
    }
}

async fn monitor_speaker(address: String, inputs: UnboundedSender<Input>) {
    let mut last = None;
    loop {
        let mut connected = bluetoothctl(&["info", &address]).await.is_some_and(|out| parse_connected(&out));
        if !connected {
            let _ = bluetoothctl(&["connect", &address]).await;
            connected = bluetoothctl(&["info", &address]).await.is_some_and(|out| parse_connected(&out));
        }
        if last != Some(connected) {
            tracing::info!("speaker {address} connected: {connected}");
            let _ = inputs.send(Input::Speaker { connected });
            last = Some(connected);
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
```

- [ ] **Step 4: Route display effects in `runtime.rs`**

In `run`, after `spawn_image_loader(...)`:
```rust
    let platform = crate::platform::Platform::start(&config, inputs.clone());
```
Change both `dispatch(effects, &player, &library)` calls to `dispatch(effects, &player, &library, &platform)`. Then update `dispatch`:
```rust
fn dispatch(
    effects: Vec<Effect>,
    player: &UnboundedSender<PlayerCommand>,
    library: &UnboundedSender<LibraryRequest>,
    platform: &crate::platform::Platform,
) {
    for effect in effects {
        match effect {
            Effect::Player(command) => {
                let _ = player.send(command);
            }
            Effect::Library(request) => {
                let _ = library.send(request);
            }
            Effect::Display(mode) => platform.set_display(mode),
        }
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p muzak-player`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/muzak-player/src
git commit -m "feat: add backlight control and Bluetooth speaker monitor"
```

---

### Task 13: Pi build, provisioning and deploy scripts

**Files:**
- Create: `deploy/Dockerfile.build`, `deploy/muzak-player.service`, `deploy/provision-remote.sh`
- Create: `scripts/build-pi.sh`, `scripts/provision.sh`, `scripts/deploy.sh`
- Create: `devices/example.toml`

**Interfaces:**
- Consumes: `muzak-player --config <path>` (Task 9), the config keys (Task 1), and `secrets/<kid>/librespot/credentials.json` (Task 10).
- Produces:
  - `target/pi/release/muzak-player`, an aarch64 Linux binary
  - `scripts/provision.sh <host>`
  - `scripts/deploy.sh <host> <config.toml> [secrets-dir]`

- [ ] **Step 1: Build image**

`deploy/Dockerfile.build`:
```dockerfile
# Raspberry Pi OS is based on Debian Trixie; building in Trixie keeps glibc compatible.
FROM rust:1.97-trixie
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config cmake clang \
        libasound2-dev libudev-dev libinput-dev libxkbcommon-dev \
        libgbm-dev libdrm-dev libegl-dev libgles-dev libfontconfig-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
```

`scripts/build-pi.sh`:
```bash
#!/usr/bin/env bash
# Builds the Pi binary inside an arm64 container (native speed on Apple Silicon).
set -euo pipefail
cd "$(dirname "$0")/.."
docker build --platform linux/arm64 -t muzak-build -f deploy/Dockerfile.build deploy
docker run --rm --platform linux/arm64 \
    -v "$PWD":/src \
    -v muzak-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/src/target/pi \
    muzak-build cargo build --release -p muzak-player
docker run --rm --platform linux/arm64 -v "$PWD":/src muzak-build \
    sh -c 'file target/pi/release/muzak-player 2>/dev/null || true; target/pi/release/muzak-player --help'
echo "Built target/pi/release/muzak-player"
```

- [ ] **Step 2: Build and verify**

Run: `chmod +x scripts/*.sh && scripts/build-pi.sh`
Expected: the build succeeds and the binary prints its `--help` text inside the container. That proves it links against the Trixie libraries. If linking fails on a missing `-l<name>`, add the matching `lib<name>-dev` package to the Dockerfile.

- [ ] **Step 3: Service, provisioning, config template**

`deploy/muzak-player.service`:
```ini
[Unit]
Description=Muzak player
After=network-online.target sound.target bluetooth.target
Wants=network-online.target

[Service]
User=muzak
Group=muzak
SupplementaryGroups=video input audio render bluetooth
Environment=SLINT_BACKEND=linuxkms-femtovg
Environment=RUST_LOG=info,librespot=warn
ExecStart=/usr/local/bin/muzak-player --config /etc/muzak/config.toml
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
```

`deploy/provision-remote.sh`:
```bash
#!/usr/bin/env bash
# Runs on the Pi as root. Safe to run more than once.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

apt-get update
apt-get install -y --no-install-recommends \
    libasound2t64 libinput10 libudev1 libxkbcommon0 libgbm1 libegl1 libgles2 libdrm2 libfontconfig1 \
    bluez bluez-alsa-utils

id muzak >/dev/null 2>&1 || useradd --system --create-home --groups video,input,audio,render,bluetooth muzak
install -d -o muzak -g muzak /var/lib/muzak /var/lib/muzak/librespot
install -d /etc/muzak

CONFIG=/boot/firmware/config.txt
grep -q '^dtoverlay=vc4-kms-dsi-7inch' "$CONFIG" || echo 'dtoverlay=vc4-kms-dsi-7inch' >> "$CONFIG"
grep -q '^dtparam=audio=on' "$CONFIG" || echo 'dtparam=audio=on' >> "$CONFIG"
CMDLINE=/boot/firmware/cmdline.txt
grep -q 'vt.global_cursor_default=0' "$CMDLINE" || sed -i '1 s/$/ vt.global_cursor_default=0 consoleblank=0/' "$CMDLINE"

cat > /etc/udev/rules.d/90-muzak-backlight.rules <<'EOF'
SUBSYSTEM=="backlight", ACTION=="add", RUN+="/bin/chgrp video /sys%p/brightness", RUN+="/bin/chmod g+w /sys%p/brightness"
EOF

mkdir -p /etc/systemd/journald.conf.d
printf '[Journal]\nSystemMaxUse=50M\n' > /etc/systemd/journald.conf.d/muzak.conf

install -m 644 "$HERE/muzak-player.service" /etc/systemd/system/muzak-player.service
systemctl daemon-reload
systemctl enable bluealsa.service muzak-player.service
systemctl disable getty@tty1.service || true

echo "Provisioned. Reboot to apply display settings: sudo reboot"
```

`scripts/provision.sh`:
```bash
#!/usr/bin/env bash
# Usage: scripts/provision.sh <ssh-host>
set -euo pipefail
cd "$(dirname "$0")/.."
HOST=${1:?usage: scripts/provision.sh <ssh-host>}
ssh "$HOST" 'rm -rf /tmp/muzak-provision && mkdir -p /tmp/muzak-provision'
scp deploy/provision-remote.sh deploy/muzak-player.service "$HOST":/tmp/muzak-provision/
ssh "$HOST" 'sudo bash /tmp/muzak-provision/provision-remote.sh'
```

`scripts/deploy.sh`:
```bash
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
```

`devices/example.toml`:
```toml
# Copy to devices/<kid>.toml and edit.
device_name = "Muzak"
state_dir = "/var/lib/muzak"
audio_backend = "alsa"
# Headphone jack:
audio_device = "plughw:CARD=Headphones"
# Bluetooth speaker instead (pair it first, see the Task 14 checklist):
# audio_device = "bluealsa:DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp"
# bluetooth_speaker = "AA:BB:CC:DD:EE:FF"
initial_volume = 60
dim_after_secs = 180
off_after_secs = 600
```

Check the template parses:

Add this test to `config.rs` tests:
```rust
    #[test]
    fn example_device_config_parses() {
        let text = include_str!("../../../devices/example.toml");
        let c = Config::parse(text).unwrap();
        assert_eq!(c.audio_device.as_deref(), Some("plughw:CARD=Headphones"));
    }
```
Run: `cargo test -p muzak-player config`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
chmod +x scripts/*.sh deploy/provision-remote.sh
git add deploy scripts devices crates/muzak-player/src/config.rs
git commit -m "feat: add Pi build, provisioning and deploy scripts"
```

---

### Task 14: Hardware bring-up and release checklist

**Files:**
- Create: `docs/hardware-checklist.md` (the checklist below, kept as the per-release record)

This task runs on real hardware with the parent present. Each step says what to run and what to expect. Fix any failure in the task that owns the code, re-run that task's tests, and redeploy.

- [ ] **Step 1: Flash and boot**

1. In Raspberry Pi Imager, choose Raspberry Pi OS Lite (64-bit). Set:
   - hostname: `muzak-<kid>`
   - a user
   - Wi-Fi
   - SSH with your public key
2. With the board unpowered, attach the DSI ribbon. Then power on.
3. Run `ssh muzak-<kid>.local uname -m`. Expected: `aarch64`.

- [ ] **Step 2: Provision and check the display**

Run:
```bash
scripts/provision.sh muzak-<kid>.local
ssh muzak-<kid>.local sudo reboot
```
After the reboot, run `ssh muzak-<kid>.local 'ls /dev/dri; cat /sys/class/drm/*DSI*/status; ls /sys/class/backlight'`.

Expected:
- `card0` (or `card1`) appears.
- The DSI status line reads `connected`.
- A backlight entry is listed. If it isn't, dimming falls back to the black overlay, which is acceptable.

- [ ] **Step 3: Sign in, build, deploy**

```bash
cargo run -p muzak-setup -- auth --state-dir secrets/<kid>
cargo run -p muzak-setup -- probe --state-dir secrets/<kid>
cp devices/example.toml devices/<kid>.toml   # set device_name, e.g. "Leo's Muzak"
scripts/build-pi.sh
scripts/deploy.sh muzak-<kid>.local devices/<kid>.toml secrets/<kid>
```
Expected: the service shows `active (running)` and the touchscreen shows the playlists grid.

- [ ] **Step 4: Release checklist**

Record pass or fail for each item in `docs/hardware-checklist.md`:
- [ ] A cold boot (unplug, replug) reaches the playlists grid with no console text visible. Note the boot time.
- [ ] Touch: taps land where touched. Swipe-scrolling the grid and the track list is smooth.
- [ ] Audio on the headphone jack plays a playlist. Pause, skip and volume respond within 0.5s.
- [ ] Memory: `systemctl status muzak-player | grep Memory` is under 250MB after 10 minutes of browsing and playing. Also note `free -m`.
- [ ] Idle: with music paused, the clock overlay appears after 3 minutes and the screen is off after 10. A tap wakes it.
- [ ] Wi-Fi drop: `sudo ip link set wlan0 down` for 30s, then `up`. Cached grids stay. Playback resumes after Play within about a minute.
- [ ] Crash recovery: `sudo pkill -9 muzak-player` brings the UI back within about 3 seconds.
- [ ] Logs: `journalctl -u muzak-player -n 50` shows no repeating errors.

- [ ] **Step 5: Bluetooth speaker (if used)**

```bash
ssh -t muzak-<kid>.local bluetoothctl
# inside bluetoothctl: power on; agent on; scan on; (wait for the speaker) pair <MAC>; trust <MAC>; connect <MAC>; quit
```
In `devices/<kid>.toml`, set:
- `audio_device = "bluealsa:DEV=<MAC>,PROFILE=a2dp"`
- `bluetooth_speaker = "<MAC>"`

Then redeploy.

Expected:
- [ ] Audio plays on the speaker.
- [ ] Turning the speaker off shows "Speaker not connected" within about 10s.
- [ ] Turning it back on reconnects and the banner clears.
- [ ] If music stutters while browsing, note it. That is Wi-Fi and Bluetooth sharing one radio, the known Pi 3 limit from the spec.

- [ ] **Step 6: Commit the record**

```bash
git add docs/hardware-checklist.md devices/<kid>.toml
git commit -m "docs: record hardware checklist for <kid>'s device"
```

---

## Contingency A: Spotify rejects librespot tokens for the Web API

Use this only if `muzak-setup probe` fails in Task 10. It switches Web API calls to the parent's own developer app. Playback is unchanged.

1. The parent creates an app at https://developer.spotify.com/dashboard:
   - Redirect URI: `http://127.0.0.1:8898/login`
   - Check the "Web API" box.
   - Add each kid's Spotify account email under "User Management".
2. In `muzak-setup`, add an `AuthWeb { state_dir: PathBuf, client_id: String }` command:
```rust
async fn auth_web(state_dir: &Path, client_id: &str) -> anyhow::Result<()> {
    let id = client_id.to_string();
    let token = tokio::task::spawn_blocking(move || {
        OAuthClientBuilder::new(&id, REDIRECT_URI, SCOPES.split(',').collect())
            .open_in_browser()
            .build()?
            .get_access_token()
    })
    .await?
    .map_err(|e| anyhow!("Spotify sign-in failed: {e}"))?;
    let json = serde_json::json!({ "client_id": client_id, "refresh_token": token.refresh_token });
    std::fs::create_dir_all(state_dir)?;
    std::fs::write(state_dir.join("web-auth.json"), serde_json::to_vec_pretty(&json)?)?;
    println!("Saved {}", state_dir.join("web-auth.json").display());
    Ok(())
}
```
3. In `muzak-player`, add `library/refresh_tokens.rs`:
```rust
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use librespot::oauth::OAuthClientBuilder;
use serde::Deserialize;

use super::FetchError;
use super::web_api::{SCOPES, TokenSource};

#[derive(Deserialize)]
struct WebAuth {
    client_id: String,
    refresh_token: String,
}

pub struct RefreshTokens {
    auth: Mutex<WebAuth>,
    cached: Mutex<Option<(String, Instant)>>,
}

impl RefreshTokens {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let auth: WebAuth = serde_json::from_slice(&std::fs::read(path)?)?;
        Ok(Self { auth: Mutex::new(auth), cached: Mutex::new(None) })
    }
}

impl TokenSource for RefreshTokens {
    async fn token(&self) -> Result<String, FetchError> {
        if let Some((token, expires)) = self.cached.lock().unwrap().clone() {
            if Instant::now() + Duration::from_secs(60) < expires {
                return Ok(token);
            }
        }
        let (client_id, refresh) = {
            let auth = self.auth.lock().unwrap();
            (auth.client_id.clone(), auth.refresh_token.clone())
        };
        let fresh = tokio::task::spawn_blocking(move || {
            OAuthClientBuilder::new(&client_id, "http://127.0.0.1:8898/login", SCOPES.split(',').collect())
                .build()?
                .refresh_token(&refresh)
        })
        .await
        .map_err(|e| FetchError::Other(e.to_string()))?
        .map_err(|e| {
            tracing::warn!("token refresh failed: {e}");
            FetchError::Auth
        })?;
        self.auth.lock().unwrap().refresh_token = fresh.refresh_token.clone();
        *self.cached.lock().unwrap() = Some((fresh.access_token.clone(), fresh.expires_at));
        Ok(fresh.access_token)
    }
}
```
   Spotify may rotate the refresh token. The new token is kept in memory only; the file keeps the original, which stays valid until revoked.
4. In `runtime.rs`, in the real branch, use `RefreshTokens::load(&config.state_dir.join("web-auth.json"))` when that file exists, and `SessionTokens` otherwise. This needs two `spawn_library` call sites, one per token type.
5. `scripts/deploy.sh`: also copy `$SECRETS/web-auth.json` to `/var/lib/muzak/web-auth.json` when it exists.
6. Re-run Task 11 Step 7.

---

## Self-Review Notes

**Spec coverage**

| Spec section | Task |
|---|---|
| Architecture: components and messages | 2, 3, 8 |
| Screens: grid, detail, now playing, idle, mini player, rail | 4, 9 |
| Feel: optimistic updates, fade-in covers, ≥64px targets | 3, 9 |
| Setup and auth | 10, 13 |
| Config | 1, 13 |
| Cache | 6, 7 |
| Data flow | 3, 6, 11 |
| Failures table | 3, 4, 11, 12, 13 (systemd restart) |
| Mac development and `--fake` | 8, 9 |
| Build, provision, deploy | 13 |
| Hardware checklist and memory budget | 14 |

Voice, playlist creation and the calendar are phases 2–4 and are deliberately out of scope here.

**Known approximations**
- "Account playing elsewhere" shows as Paused without the other device's name. The spec was updated to match.
- Fake shuffle reverses the queue.

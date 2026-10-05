//! Plays Spotify through an embedded librespot Connect device (Spirc).

use std::path::PathBuf;
use std::time::Duration;

use librespot::connect::{
    ConnectConfig, LoadContextOptions, LoadRequest, LoadRequestOptions, Options, PlayingTrack,
    Spirc,
};
use librespot::core::Error as SpotifyError;
use librespot::core::cache::Cache;
use librespot::core::config::SessionConfig;
use librespot::core::error::ErrorKind;
use librespot::core::session::Session;
use librespot::metadata::audio::{AudioItem, UniqueFields};
use librespot::playback::audio_backend;
use librespot::playback::config::{AudioFormat, Bitrate, PlayerConfig};
use librespot::playback::mixer::{self, MixerConfig};
use librespot::playback::player::{Player, PlayerEvent};
use sha1::{Digest, Sha1};
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
    /// Spotify stream quality in kbps.
    pub bitrate: u16,
}

impl PlayerSettings {
    pub fn from_config(config: &Config) -> Self {
        Self {
            device_name: config.device_name.clone(),
            backend: config.audio_backend.clone(),
            device: config.audio_device.clone(),
            credentials_dir: config.librespot_dir(),
            initial_volume: config.initial_volume,
            bitrate: config.bitrate,
        }
    }
}

enum Exit {
    Disconnected,
    CommandsClosed,
}

#[derive(Debug, PartialEq)]
enum Failure {
    /// No credentials.json on disk.
    NoCredentials,
    /// Spotify rejected the stored credentials (or the account cannot stream).
    BadCredentials,
    /// Anything else (network, captive portal, HTTP 403, ...): retry.
    Other(String),
}

/// Only a definite login rejection is fatal. HTTP 401/403 from other services and
/// io errors also map to Unauthenticated/PermissionDenied, so the error kind alone is not used.
/// librespot-core does not export its `AuthenticationError`, so the login failure is matched by
/// the message librespot-core 0.8.0 gives each `ErrorCode` (`connection::login_error_message`).
/// These strings come from that function: re-check them whenever librespot is upgraded.
fn classify(e: &SpotifyError) -> Failure {
    const FATAL: [&str; 3] = [
        "Login failed with reason: Bad credentials",
        "Login failed with reason: Could not validate credentials",
        "Login failed with reason: Premium account required",
    ];
    let message = e.to_string();
    if e.kind == ErrorKind::PermissionDenied && FATAL.iter().any(|m| message.contains(m)) {
        Failure::BadCredentials
    } else {
        Failure::Other(message)
    }
}

impl From<SpotifyError> for Failure {
    fn from(e: SpotifyError) -> Self {
        classify(&e)
    }
}

const STABLE_AFTER: Duration = Duration::from_secs(30);
const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// How long to wait before reconnecting. A connection that stayed up resets the backoff.
fn sleep_for(current: Duration, uptime: Duration) -> Duration {
    if uptime >= STABLE_AFTER {
        MIN_BACKOFF
    } else {
        current
    }
}

fn next_backoff(slept: Duration) -> Duration {
    (slept * 2).min(MAX_BACKOFF)
}

/// What this adapter knows about the Connect device, kept across reconnects.
#[derive(Debug, Default)]
struct Tracking {
    /// True while this device is the active Connect device. Spirc ignores most commands otherwise.
    active: bool,
    last_context: Option<(String, bool)>,
    track_uri: Option<String>,
    position_ms: u32,
}

impl Tracking {
    fn observe(&mut self, event: &PlayerEvent) {
        match event {
            PlayerEvent::SessionConnected { .. } => self.active = true,
            PlayerEvent::SessionDisconnected { .. } => self.active = false,
            PlayerEvent::TrackChanged { audio_item } => {
                self.track_uri = Some(audio_item.uri.clone());
                self.position_ms = 0;
            }
            PlayerEvent::Playing { position_ms, .. }
            | PlayerEvent::Paused { position_ms, .. }
            | PlayerEvent::PositionChanged { position_ms, .. }
            | PlayerEvent::Seeked { position_ms, .. }
            | PlayerEvent::PositionCorrection { position_ms, .. } => {
                self.position_ms = *position_ms
            }
            _ => {}
        }
    }

    fn note_command(&mut self, command: &PlayerCommand) {
        if let PlayerCommand::Load {
            context_uri,
            shuffle,
            ..
        } = command
        {
            self.last_context = Some((context_uri.clone(), *shuffle));
            self.track_uri = None;
            self.position_ms = 0;
            // A Load activates this device; don't wait for SessionConnected to route what follows.
            self.active = true;
        }
    }
}

/// A Connect device id that stays the same across restarts, so phones see one device.
/// Same scheme as the librespot binary: lowercase hex SHA-1 of the device name.
fn device_id(name: &str) -> String {
    Sha1::digest(name.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Commands queued while disconnected are stale and dropped, but a dropped Load is still
/// remembered so a later Play can resume it. The core resends a pending Load on Connected.
fn drain_stale(commands: &mut UnboundedReceiver<PlayerCommand>, tracking: &mut Tracking) {
    while let Ok(command) = commands.try_recv() {
        tracking.note_command(&command);
    }
    tracking.active = false;
}

#[derive(Debug, PartialEq)]
enum Route {
    /// Hand the command to Spirc.
    Forward,
    /// Re-activate this device and reload the last context at the remembered track and position.
    Resume {
        context_uri: String,
        shuffle: bool,
        track_uri: Option<String>,
        position_ms: u32,
    },
    /// Answer from the adapter without touching Spirc.
    Report(PlayerUpdate),
    Skip,
}

fn route(state: &Tracking, command: &PlayerCommand) -> Route {
    if state.active || matches!(command, PlayerCommand::Load { .. }) {
        return Route::Forward;
    }
    match command {
        PlayerCommand::Play => match &state.last_context {
            Some((uri, shuffle)) => Route::Resume {
                context_uri: uri.clone(),
                shuffle: *shuffle,
                track_uri: state.track_uri.clone(),
                position_ms: state.position_ms,
            },
            None => Route::Report(PlayerUpdate::Stopped),
        },
        PlayerCommand::Pause => Route::Report(PlayerUpdate::Paused {
            position_ms: state.position_ms,
        }),
        _ => Route::Skip,
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
    let mut backoff = MIN_BACKOFF;
    let mut tracking = Tracking::default();
    loop {
        let started = std::time::Instant::now();
        match connect_and_serve(
            &settings,
            &session_tx,
            &inputs,
            &mut commands,
            &mut tracking,
        )
        .await
        {
            Ok(Exit::CommandsClosed) => return,
            Ok(Exit::Disconnected) => tracing::warn!("Spotify connection ended; reconnecting"),
            Err(Failure::NoCredentials) => {
                tracing::error!("no Spotify credentials found; run ziggy-setup auth");
                session_tx.send_replace(None);
                let _ = inputs.send(Input::AuthInvalid);
                return;
            }
            Err(Failure::BadCredentials) => {
                tracing::error!("Spotify credentials rejected; rerun ziggy-setup auth");
                session_tx.send_replace(None);
                let _ = inputs.send(Input::AuthInvalid);
                return;
            }
            Err(Failure::Other(e)) => tracing::warn!("Spotify connection failed: {e}"),
        }
        session_tx.send_replace(None);
        let _ = inputs.send(Input::Player(PlayerUpdate::Disconnected));
        let sleep = sleep_for(backoff, started.elapsed());
        tokio::time::sleep(sleep).await;
        backoff = next_backoff(sleep);
    }
}

async fn connect_and_serve(
    settings: &PlayerSettings,
    session_tx: &watch::Sender<Option<Session>>,
    inputs: &UnboundedSender<Input>,
    commands: &mut UnboundedReceiver<PlayerCommand>,
    tracking: &mut Tracking,
) -> Result<Exit, Failure> {
    let dir = &settings.credentials_dir;
    let cache = Cache::new(Some(dir), Some(dir), None, None)?;
    let credentials = cache.credentials().ok_or(Failure::NoCredentials)?;
    let session_config = SessionConfig {
        device_id: device_id(&settings.device_name),
        ..Default::default()
    };
    let session = Session::new(session_config, Some(cache));

    let mixer_builder = mixer::find(None).ok_or_else(|| Failure::Other("no mixer".into()))?;
    let mixer = mixer_builder(MixerConfig::default())?;
    let sink_builder = audio_backend::find(Some(settings.backend.clone()))
        .ok_or_else(|| Failure::Other(format!("unknown audio backend {}", settings.backend)))?;
    let device = settings.device.clone();
    let player_config = PlayerConfig {
        bitrate: match settings.bitrate {
            96 => Bitrate::Bitrate96,
            160 => Bitrate::Bitrate160,
            _ => Bitrate::Bitrate320,
        },
        position_update_interval: Some(Duration::from_secs(1)),
        ..Default::default()
    };
    let player = Player::new(
        player_config,
        session.clone(),
        mixer.get_soft_volume(),
        move || sink_builder(device, AudioFormat::default()),
    );
    let mut events = player.get_player_event_channel();
    let connect_config = ConnectConfig {
        name: settings.device_name.clone(),
        initial_volume: percent_to_volume(settings.initial_volume),
        ..Default::default()
    };

    let (spirc, spirc_task) =
        Spirc::new(connect_config, session.clone(), credentials, player, mixer).await?;
    // Drain right before Connected so a Load queued during setup isn't sent twice.
    drain_stale(commands, tracking);
    session_tx.send_replace(Some(session.clone()));
    let _ = inputs.send(Input::Player(PlayerUpdate::Connected));
    tokio::pin!(spirc_task);

    loop {
        tokio::select! {
            _ = &mut spirc_task => return Ok(Exit::Disconnected),
            command = commands.recv() => match command {
                Some(command) => {
                    tracking.note_command(&command);
                    match route(tracking, &command) {
                        Route::Forward => {
                            if let Err(e) = apply(&spirc, &session, command) {
                                tracing::warn!("player command failed: {e}");
                            }
                        }
                        Route::Resume { context_uri, shuffle, track_uri, position_ms } => {
                            tracking.active = true;
                            if let Err(e) = resume(&spirc, &session, &context_uri, shuffle, track_uri, position_ms) {
                                tracing::warn!("resume failed: {e}");
                            }
                        }
                        Route::Report(update) => {
                            let _ = inputs.send(Input::Player(update));
                        }
                        Route::Skip => tracing::debug!("skipping {command:?} while inactive"),
                    }
                }
                None => {
                    let _ = spirc.shutdown();
                    let _ = tokio::time::timeout(Duration::from_secs(2), &mut spirc_task).await;
                    return Ok(Exit::CommandsClosed);
                }
            },
            event = events.recv() => match event {
                Some(event) => {
                    tracking.observe(&event);
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

fn load_request(
    context_uri: String,
    shuffle: bool,
    playing_track: Option<PlayingTrack>,
    seek_to: u32,
) -> LoadRequest {
    LoadRequest::from_context_uri(
        context_uri,
        LoadRequestOptions {
            start_playing: true,
            seek_to,
            context_options: Some(LoadContextOptions::Options(Options {
                shuffle,
                repeat: false,
                repeat_track: false,
            })),
            playing_track,
        },
    )
}

fn resume(
    spirc: &Spirc,
    session: &Session,
    context_uri: &str,
    shuffle: bool,
    track_uri: Option<String>,
    position_ms: u32,
) -> Result<(), SpotifyError> {
    let uri = context_uri_for(context_uri, &session.username());
    spirc.activate()?;
    spirc.load(load_request(
        uri,
        shuffle,
        track_uri.map(PlayingTrack::Uri),
        position_ms,
    ))
}

fn apply(spirc: &Spirc, session: &Session, command: PlayerCommand) -> Result<(), SpotifyError> {
    match command {
        PlayerCommand::Load {
            context_uri,
            start_index,
            start_uri,
            shuffle,
        } => {
            let uri = context_uri_for(&context_uri, &session.username());
            spirc.activate()?;
            let playing_track = start_uri
                .map(PlayingTrack::Uri)
                .or(start_index.map(PlayingTrack::Index));
            spirc.load(load_request(uri, shuffle, playing_track, 0))
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
        PlayerEvent::TrackChanged { audio_item } => {
            PlayerUpdate::TrackChanged(track_from_item(&audio_item))
        }
        PlayerEvent::Loading { .. } => PlayerUpdate::Loading,
        PlayerEvent::Playing { position_ms, .. } => PlayerUpdate::Playing { position_ms },
        PlayerEvent::Paused { position_ms, .. } => PlayerUpdate::Paused { position_ms },
        PlayerEvent::Stopped { .. } => PlayerUpdate::Stopped,
        PlayerEvent::PositionChanged { position_ms, .. }
        | PlayerEvent::Seeked { position_ms, .. }
        | PlayerEvent::PositionCorrection { position_ms, .. } => {
            PlayerUpdate::Position { position_ms }
        }
        PlayerEvent::VolumeChanged { volume } => PlayerUpdate::Volume {
            percent: volume_to_percent(volume),
        },
        PlayerEvent::ShuffleChanged { shuffle } => PlayerUpdate::Shuffle(shuffle),
        PlayerEvent::RepeatChanged { context, track } => PlayerUpdate::Repeat(if track {
            Repeat::Track
        } else if context {
            Repeat::Context
        } else {
            Repeat::Off
        }),
        PlayerEvent::Unavailable { .. } => PlayerUpdate::Unavailable,
        _ => return None,
    })
}

fn track_from_item(item: &AudioItem) -> Track {
    let (artists, album, artist_uri) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            artists
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            album.clone(),
            artists.first().and_then(|a| a.id.to_uri().ok()),
        ),
        UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new(), None),
        UniqueFields::Local { artists, album, .. } => (
            artists.clone().unwrap_or_default(),
            album.clone().unwrap_or_default(),
            None,
        ),
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
        album_uri: None,
        artist_uri,
    }
}

#[cfg(test)]
mod tests {
    use librespot::core::SpotifyUri;
    use librespot::playback::player::PlayerEvent;

    use super::*;
    use crate::app::PlayerUpdate;
    use crate::model::{LIKED_URI, Repeat};

    fn uri() -> SpotifyUri {
        SpotifyUri::from_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC").unwrap()
    }

    #[test]
    fn maps_playback_events() {
        assert_eq!(
            map_event(PlayerEvent::Playing {
                play_request_id: 1,
                track_id: uri(),
                position_ms: 42
            }),
            Some(PlayerUpdate::Playing { position_ms: 42 })
        );
        assert_eq!(
            map_event(PlayerEvent::Paused {
                play_request_id: 1,
                track_id: uri(),
                position_ms: 7
            }),
            Some(PlayerUpdate::Paused { position_ms: 7 })
        );
        assert_eq!(
            map_event(PlayerEvent::PositionChanged {
                play_request_id: 1,
                track_id: uri(),
                position_ms: 9
            }),
            Some(PlayerUpdate::Position { position_ms: 9 })
        );
        assert_eq!(
            map_event(PlayerEvent::Unavailable {
                play_request_id: 1,
                track_id: uri()
            }),
            Some(PlayerUpdate::Unavailable)
        );
        assert_eq!(map_event(PlayerEvent::Preloading { track_id: uri() }), None);
        // Connect activation is not network state.
        assert_eq!(
            map_event(PlayerEvent::SessionConnected {
                connection_id: "c".into(),
                user_name: "u".into()
            }),
            None
        );
        assert_eq!(
            map_event(PlayerEvent::SessionDisconnected {
                connection_id: "c".into(),
                user_name: "u".into()
            }),
            None
        );
    }

    #[test]
    fn maps_settings_events() {
        assert_eq!(
            map_event(PlayerEvent::VolumeChanged { volume: u16::MAX }),
            Some(PlayerUpdate::Volume { percent: 100 })
        );
        assert_eq!(
            map_event(PlayerEvent::ShuffleChanged { shuffle: true }),
            Some(PlayerUpdate::Shuffle(true))
        );
        assert_eq!(
            map_event(PlayerEvent::RepeatChanged {
                context: true,
                track: false
            }),
            Some(PlayerUpdate::Repeat(Repeat::Context))
        );
        assert_eq!(
            map_event(PlayerEvent::RepeatChanged {
                context: true,
                track: true
            }),
            Some(PlayerUpdate::Repeat(Repeat::Track))
        );
    }

    #[test]
    fn liked_uri_maps_to_user_collection() {
        assert_eq!(
            context_uri_for(LIKED_URI, "kid1"),
            "spotify:user:kid1:collection"
        );
        assert_eq!(
            context_uri_for("spotify:album:x", "kid1"),
            "spotify:album:x"
        );
    }

    fn login(reason: &str) -> SpotifyError {
        // Same shape librespot-core builds for AuthenticationError::LoginFailed.
        SpotifyError::permission_denied(format!("Login failed with reason: {reason}"))
    }

    #[test]
    fn only_definite_login_rejections_are_fatal() {
        assert_eq!(classify(&login("Bad credentials")), Failure::BadCredentials);
        assert_eq!(
            classify(&login("Could not validate credentials")),
            Failure::BadCredentials
        );
        assert_eq!(
            classify(&login("Premium account required")),
            Failure::BadCredentials
        );
        assert!(matches!(
            classify(&login("Try another access point")),
            Failure::Other(_)
        ));
        assert!(matches!(
            classify(&login("Travel restriction")),
            Failure::Other(_)
        ));
        assert!(matches!(
            classify(&SpotifyError::unauthenticated("http 401")),
            Failure::Other(_)
        ));
        assert!(matches!(
            classify(&SpotifyError::permission_denied("http 403")),
            Failure::Other(_)
        ));
        assert!(matches!(
            classify(&SpotifyError::unavailable("down")),
            Failure::Other(_)
        ));
    }

    #[test]
    fn backoff_resets_only_after_a_stable_connection() {
        let s = Duration::from_secs;
        assert_eq!(sleep_for(s(8), s(1)), s(8));
        assert_eq!(sleep_for(s(8), s(29)), s(8));
        assert_eq!(sleep_for(s(8), s(30)), s(2));
        assert_eq!(next_backoff(s(2)), s(4));
        assert_eq!(next_backoff(s(40)), s(60));
        assert_eq!(next_backoff(s(60)), s(60));
    }

    fn inactive_with_context() -> Tracking {
        Tracking {
            active: false,
            last_context: Some(("spotify:album:a".into(), true)),
            track_uri: Some("spotify:track:t".into()),
            position_ms: 1234,
        }
    }

    #[test]
    fn inactive_commands_are_routed_by_the_adapter() {
        let t = inactive_with_context();
        assert_eq!(
            route(&t, &PlayerCommand::Play),
            Route::Resume {
                context_uri: "spotify:album:a".into(),
                shuffle: true,
                track_uri: Some("spotify:track:t".into()),
                position_ms: 1234,
            }
        );
        assert_eq!(
            route(&t, &PlayerCommand::Pause),
            Route::Report(PlayerUpdate::Paused { position_ms: 1234 })
        );
        assert_eq!(route(&t, &PlayerCommand::Next), Route::Skip);
        assert_eq!(
            route(&t, &PlayerCommand::SetVolume { percent: 5 }),
            Route::Skip
        );
        let load = PlayerCommand::Load {
            context_uri: "x".into(),
            start_index: None,
            start_uri: None,
            shuffle: false,
        };
        assert_eq!(route(&t, &load), Route::Forward);
        assert_eq!(
            route(&Tracking::default(), &PlayerCommand::Play),
            Route::Report(PlayerUpdate::Stopped)
        );
    }

    #[test]
    fn active_commands_go_to_spirc() {
        let t = Tracking {
            active: true,
            ..inactive_with_context()
        };
        for c in [
            PlayerCommand::Play,
            PlayerCommand::Pause,
            PlayerCommand::Next,
        ] {
            assert_eq!(route(&t, &c), Route::Forward);
        }
    }

    #[test]
    fn device_id_is_stable_per_name() {
        // Same scheme as the librespot binary: lowercase hex SHA-1 of the name.
        assert_eq!(
            device_id("Test"),
            "640ab2bae07bedc4c163f679a746f7ab7fb5d1fa"
        );
        assert_eq!(device_id("Kid Room"), device_id("Kid Room"));
        assert_ne!(device_id("Kid Room"), device_id("Kid Room 2"));
    }

    #[test]
    fn load_then_immediate_pause_goes_to_spirc() {
        let mut t = Tracking::default();
        let load = PlayerCommand::Load {
            context_uri: "spotify:album:a".into(),
            start_index: None,
            start_uri: None,
            shuffle: false,
        };
        t.note_command(&load);
        assert_eq!(route(&t, &load), Route::Forward);
        t.note_command(&PlayerCommand::Pause);
        assert_eq!(route(&t, &PlayerCommand::Pause), Route::Forward);
    }

    #[test]
    fn stale_commands_are_dropped_but_a_load_is_remembered() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        tx.send(PlayerCommand::Load {
            context_uri: "spotify:album:b".into(),
            start_index: Some(1),
            start_uri: None,
            shuffle: true,
        })
        .unwrap();
        tx.send(PlayerCommand::Pause).unwrap();
        let mut t = Tracking {
            active: true,
            ..Tracking::default()
        };
        drain_stale(&mut rx, &mut t);
        assert!(rx.try_recv().is_err());
        assert!(!t.active);
        assert_eq!(t.last_context, Some(("spotify:album:b".into(), true)));
        assert_eq!(
            route(&t, &PlayerCommand::Play),
            Route::Resume {
                context_uri: "spotify:album:b".into(),
                shuffle: true,
                track_uri: None,
                position_ms: 0,
            }
        );
    }

    #[test]
    fn tracking_follows_events_and_loads() {
        let mut t = Tracking::default();
        t.observe(&PlayerEvent::SessionConnected {
            connection_id: "c".into(),
            user_name: "u".into(),
        });
        assert!(t.active);
        t.observe(&PlayerEvent::Playing {
            play_request_id: 1,
            track_id: uri(),
            position_ms: 500,
        });
        assert_eq!(t.position_ms, 500);
        t.note_command(&PlayerCommand::Load {
            context_uri: "spotify:album:b".into(),
            start_index: Some(2),
            start_uri: None,
            shuffle: true,
        });
        assert_eq!(t.last_context, Some(("spotify:album:b".into(), true)));
        assert_eq!(t.position_ms, 0);
        t.observe(&PlayerEvent::SessionDisconnected {
            connection_id: "c".into(),
            user_name: "u".into(),
        });
        assert!(!t.active);
    }
}

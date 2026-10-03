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
        PlayerCommand::Load {
            context_uri,
            start_index,
            shuffle,
        } => {
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
        PlayerEvent::SessionConnected { .. } => PlayerUpdate::Connected,
        PlayerEvent::SessionDisconnected { .. } => PlayerUpdate::Disconnected,
        _ => return None,
    })
}

fn track_from_item(item: &AudioItem) -> Track {
    let (artists, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            artists
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            album.clone(),
        ),
        UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new()),
        UniqueFields::Local { artists, album, .. } => (
            artists.clone().unwrap_or_default(),
            album.clone().unwrap_or_default(),
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
}

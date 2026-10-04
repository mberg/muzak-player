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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let inputs = inputs_tx.clone();
    std::thread::Builder::new()
        .name("muzak-runtime".into())
        .spawn(move || {
            runtime.block_on(async move {
                if let Err(e) = run(
                    config,
                    fake,
                    inputs,
                    inputs_rx,
                    image_requests,
                    publish,
                    images,
                )
                .await
                {
                    // Without the runtime the UI would show "Loading…" forever. Exit so systemd
                    // restarts the player and the journal shows why.
                    tracing::error!("runtime stopped: {e:#}");
                    std::process::exit(1);
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
    // Choices made on the Settings screen override the config file.
    let saved = crate::settings::Settings::load(&config.state_dir);
    let mut config = config;
    saved.apply(&mut config);
    let speaker = match &saved.output {
        Some(crate::settings::Output::Bluetooth(speaker)) => Some(speaker.clone()),
        Some(crate::settings::Output::Jack) => None,
        None => config
            .bluetooth_speaker
            .clone()
            .map(|address| crate::settings::Speaker {
                name: address.clone(),
                address,
            }),
    };
    let bluetooth = crate::bluetooth::spawn(
        if fake {
            crate::bluetooth::Backend::Fake
        } else {
            crate::bluetooth::Backend::Real {
                watch: config.bluetooth_speaker.clone(),
            }
        },
        inputs.clone(),
    );
    let cache = Arc::new(DiskCache::new(config.cache_dir())?);
    let (player, library) = if fake {
        let catalog = Arc::new(FakeCatalog::sample());
        let library = spawn_library(
            Arc::new(FakeSource::new(catalog.clone())),
            cache,
            inputs.clone(),
        );
        (spawn_fake_player(catalog, inputs.clone()), library)
    } else {
        let (session_tx, session_rx) = tokio::sync::watch::channel(None);
        let player = crate::player::librespot::spawn(
            crate::player::librespot::PlayerSettings::from_config(&config),
            session_tx,
            inputs.clone(),
        );
        let http = crate::library::web_api::ReqwestHttp::new()?;
        // Endpoints Spotify told us to leave alone, remembered across restarts.
        let block_file = config.cache_dir().join("rate-limits.json");
        let web_auth = config
            .state_dir
            .join(crate::library::refresh_tokens::FILE_NAME);
        let library = if web_auth.is_file() {
            // Contingency A: library requests use the parent's own developer app.
            tracing::info!("using developer-app tokens from {}", web_auth.display());
            let tokens = crate::library::refresh_tokens::RefreshTokens::load(&web_auth)?;
            let source = Arc::new(
                crate::library::web_api::WebApi::new(http, tokens)
                    .with_block_file(block_file.clone()),
            );
            spawn_library(source, cache, inputs.clone())
        } else {
            let tokens = crate::library::session_tokens::SessionTokens::new(session_rx);
            let source = Arc::new(
                crate::library::web_api::WebApi::new(http, tokens)
                    .with_block_file(block_file.clone()),
            );
            spawn_library(source, cache, inputs.clone())
        };
        (player, library)
    };
    spawn_image_loader(
        Arc::new(ImageLoader::new(config.images_dir(), 300)?),
        image_requests,
        images,
    );

    let platform = crate::platform::Platform::start();

    let started = Instant::now();
    let now_ms = || started.elapsed().as_millis() as u64;
    let core_config = CoreConfig {
        dim_after_ms: config.dim_after_secs * 1000,
        off_after_ms: config.off_after_secs * 1000,
        initial_volume: config.initial_volume,
        device_name: config.device_name.clone(),
        speaker,
        saved,
        bluetooth: bluetooth.is_some(),
    };
    let outputs = Outputs {
        player,
        library,
        bluetooth,
        state_dir: config.state_dir.clone(),
    };
    let (mut core, effects) = Core::new(core_config, now_ms());
    dispatch(effects, &outputs, &platform);
    publish(core.state().clone());

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    // Only runs while the search screen is open, so a search goes out soon after typing stops.
    let mut search_tick = tokio::time::interval(Duration::from_millis(150));
    search_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let input = tokio::select! {
            Some(input) = inputs_rx.recv() => input,
            _ = tick.tick() => Input::Tick,
            _ = search_tick.tick(), if core.wants_search_tick() => Input::SearchTick,
        };
        let quiet = input == Input::SearchTick;
        let effects = core.handle(input, now_ms());
        // A search tick that sent nothing changed nothing; skip re-rendering.
        if quiet && effects.is_empty() {
            continue;
        }
        dispatch(effects, &outputs, &platform);
        publish(core.state().clone());
    }
}

/// Where effects go.
struct Outputs {
    player: UnboundedSender<PlayerCommand>,
    library: UnboundedSender<LibraryRequest>,
    bluetooth: Option<UnboundedSender<crate::app::BtCommand>>,
    state_dir: std::path::PathBuf,
}

fn dispatch(effects: Vec<Effect>, outputs: &Outputs, platform: &crate::platform::Platform) {
    for effect in effects {
        match effect {
            Effect::Player(command) => {
                let _ = outputs.player.send(command);
            }
            Effect::Library(request) => {
                let _ = outputs.library.send(request);
            }
            Effect::Display(mode) => platform.set_display(mode),
            Effect::Bluetooth(command) => {
                if let Some(bluetooth) = &outputs.bluetooth {
                    let _ = bluetooth.send(command);
                }
            }
            Effect::SaveSettings(settings) => {
                if let Err(e) = settings.save(&outputs.state_dir) {
                    tracing::error!("saving settings failed: {e:#}");
                }
            }
            Effect::ApplySettings(settings) => {
                let state_dir = outputs.state_dir.clone();
                tokio::spawn(async move {
                    if let Err(e) = settings.save(&state_dir) {
                        tracing::error!("saving settings failed: {e:#}");
                        return;
                    }
                    tracing::info!("settings saved; restarting to apply them");
                    // Long enough for the screen to say it's restarting.
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    crate::platform::restart();
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::app::{PlayStatus, UiAction};
    use crate::model::Section;

    #[test]
    fn fake_runtime_loads_library_and_plays() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::parse(&format!(
            "device_name = \"Test\"\nstate_dir = \"{}\"\n",
            dir.path().display()
        ))
        .unwrap();
        let (state_tx, state_rx) = std::sync::mpsc::channel();
        let (_image_tx, image_rx) = tokio::sync::mpsc::unbounded_channel();
        let inputs = start(
            config,
            true,
            image_rx,
            Box::new(move |s| {
                let _ = state_tx.send(s);
            }),
            Arc::new(|_, _| {}),
        )
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let wait_for = |pred: &dyn Fn(&AppState) -> bool| loop {
            let state = state_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("state published");
            if pred(&state) {
                break;
            }
            assert!(Instant::now() < deadline, "timed out");
        };
        wait_for(&|s| {
            s.sections
                .get(&Section::Playlists)
                .and_then(|x| x.data.as_ref())
                .is_some_and(|d| !d.is_empty())
        });
        inputs
            .send(Input::Ui(UiAction::PlayCollection {
                uri: "spotify:playlist:fake0".into(),
                shuffle: false,
            }))
            .unwrap();
        wait_for(&|s| s.playback.status == PlayStatus::Playing && s.playback.track.is_some());

        // Search: typing, a pause, then catalog results arrive without another input.
        inputs
            .send(Input::Ui(UiAction::ShowSection(Section::Search)))
            .unwrap();
        inputs
            .send(Input::Ui(UiAction::KeyPressed("dance".into())))
            .unwrap();
        wait_for(&|s| {
            s.search
                .results
                .data
                .as_ref()
                .is_some_and(|r| !r.playlists.is_empty())
        });

        // Editing: add the playing song to "Bedtime" (fake1) and see Spotify's answer come back.
        wait_for(&|s| s.account.is_some());
        let song = "spotify:track:fake-spotify-playlist-fake0-1";
        inputs
            .send(Input::Ui(UiAction::OpenCollection(
                "spotify:playlist:fake1".into(),
            )))
            .unwrap();
        inputs
            .send(Input::Ui(UiAction::OpenPicker(song.into())))
            .unwrap();
        inputs
            .send(Input::Ui(UiAction::PickPlaylist(
                "spotify:playlist:fake1".into(),
            )))
            .unwrap();
        wait_for(&|s| {
            s.tracks
                .get("spotify:playlist:fake1")
                .and_then(|slot| slot.data.as_ref())
                .is_some_and(|tracks| tracks.len() == 13 && tracks[12].uri == song)
                && !s.tracks["spotify:playlist:fake1"].loading
        });
    }
}

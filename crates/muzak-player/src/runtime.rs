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
        let source = Arc::new(crate::library::web_api::WebApi::new(
            crate::library::web_api::ReqwestHttp::new()?,
            crate::library::session_tokens::SessionTokens::new(session_rx),
        ));
        (player, spawn_library(source, cache, inputs.clone()))
    };
    spawn_image_loader(
        Arc::new(ImageLoader::new(config.images_dir(), 300)?),
        image_requests,
        images,
    );

    let platform = crate::platform::Platform::start(&config, inputs.clone());

    let started = Instant::now();
    let now_ms = || started.elapsed().as_millis() as u64;
    let core_config = CoreConfig {
        dim_after_ms: config.dim_after_secs * 1000,
        off_after_ms: config.off_after_secs * 1000,
        initial_volume: config.initial_volume,
    };
    let (mut core, effects) = Core::new(core_config, now_ms());
    dispatch(effects, &player, &library, &platform);
    publish(core.state().clone());

    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        let input = tokio::select! {
            Some(input) = inputs_rx.recv() => input,
            _ = tick.tick() => Input::Tick,
        };
        let effects = core.handle(input, now_ms());
        dispatch(effects, &player, &library, &platform);
        publish(core.state().clone());
    }
}

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

        let deadline = Instant::now() + Duration::from_secs(5);
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
    }
}

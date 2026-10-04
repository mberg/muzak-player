//! Runs voice control: the microphone and wake word on a thread, Gemini on the runtime.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::UnboundedSender;

use super::gemini::Gemini;
use super::listen::{Heard, Listener};
use super::wake::SherpaWake;
use crate::app::{Input, VoiceContext, VoiceUpdate};

pub struct VoiceSettings {
    /// A sherpa-onnx keyword model folder.
    pub model_dir: PathBuf,
    pub phrase: String,
    pub threshold: f32,
    pub microphone: Option<String>,
    pub gemini_key: Option<String>,
    pub gemini_model: Option<String>,
}

/// The latest library and playback summary, sent with each request.
pub type SharedContext = Arc<Mutex<VoiceContext>>;

pub fn spawn_voice(
    settings: VoiceSettings,
    inputs: UnboundedSender<Input>,
    context: SharedContext,
    runtime: tokio::runtime::Handle,
) {
    let gemini = settings
        .gemini_key
        .clone()
        .map(|key| Arc::new(Gemini::new(key, settings.gemini_model.clone())));
    std::thread::Builder::new()
        .name("voice".into())
        .spawn(move || {
            let wake =
                match SherpaWake::load(&settings.model_dir, &settings.phrase, settings.threshold) {
                    Ok(wake) => wake,
                    Err(e) => {
                        tracing::error!("voice is off: {e:#}");
                        return;
                    }
                };
            tracing::info!("voice: listening for {:?}", settings.phrase);
            let mut listener = Listener::new(wake);
            loop {
                let (tx, rx) = std::sync::mpsc::channel();
                // The stream records while it's alive.
                let _stream = match super::mic::open(settings.microphone.as_deref(), tx) {
                    Ok(stream) => stream,
                    Err(e) => {
                        tracing::warn!("voice: {e:#}; trying again soon");
                        std::thread::sleep(Duration::from_secs(10));
                        continue;
                    }
                };
                // Ends when the microphone goes away.
                while let Ok(chunk) = rx.recv_timeout(Duration::from_secs(5)) {
                    for heard in listener.push(&chunk) {
                        let send = |update| {
                            let _ = inputs.send(Input::Voice(update));
                        };
                        match heard {
                            Heard::Woke => send(VoiceUpdate::Woke),
                            Heard::Nothing => send(VoiceUpdate::NotUnderstood),
                            Heard::Request(audio) => {
                                send(VoiceUpdate::Thinking);
                                let Some(gemini) = gemini.clone() else {
                                    send(VoiceUpdate::Failed(
                                        "Voice needs a Gemini key in the config".into(),
                                    ));
                                    continue;
                                };
                                let context = context.lock().unwrap().clone();
                                let inputs = inputs.clone();
                                runtime.spawn(async move {
                                    let update = match gemini.ask(&context, &audio).await {
                                        Ok(Some(command)) => VoiceUpdate::Command(command),
                                        Ok(None) => VoiceUpdate::NotUnderstood,
                                        Err(e) => VoiceUpdate::Failed(e.to_string()),
                                    };
                                    tracing::info!("voice: {update:?}");
                                    let _ = inputs.send(Input::Voice(update));
                                });
                            }
                        }
                    }
                }
                tracing::warn!("voice: the microphone stopped; reopening");
            }
        })
        .expect("voice thread");
}

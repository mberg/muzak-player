//! The wake word, spotted on the device with a sherpa-onnx keyword model.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use sherpa_onnx::{
    KeywordSpotter, KeywordSpotterConfig, OnlineModelConfig, OnlineStream,
    OnlineTransducerModelConfig,
};

use super::bpe::Bpe;
use super::listen::{RATE, WakeWord};

/// The model hears "muzak" much like "music", so this also wakes on "hey music" now and then.
/// The phrase can be changed in the config.
pub const DEFAULT_PHRASE: &str = "hey muzak";
pub const DEFAULT_THRESHOLD: f32 = 0.25;

pub struct SherpaWake {
    spotter: KeywordSpotter,
    stream: OnlineStream,
}

/// The model file whose name starts with `part`, preferring the smaller int8 version.
fn model_file(dir: &Path, part: &str) -> anyhow::Result<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(part) && n.ends_with(".onnx"))
        })
        .collect();
    found.sort_by_key(|p| !p.to_string_lossy().contains(".int8."));
    found
        .into_iter()
        .next()
        .with_context(|| format!("no {part}*.onnx in {}", dir.display()))
}

impl SherpaWake {
    /// Loads a sherpa-onnx keyword model (e.g. sherpa-onnx-kws-zipformer-gigaspeech-3.3M).
    pub fn load(dir: &Path, phrase: &str, threshold: f32) -> anyhow::Result<Self> {
        let bpe = std::fs::read(dir.join("bpe.model"))
            .ok()
            .and_then(|b| Bpe::from_model(&b))
            .with_context(|| format!("reading {}/bpe.model", dir.display()))?;
        let Some(keywords) = bpe.keyword_line(phrase, "wake") else {
            bail!("the wake phrase {phrase:?} can't be spelled with this model's English letters");
        };
        let path = |p: PathBuf| Some(p.to_string_lossy().into_owned());
        let config = KeywordSpotterConfig {
            model_config: OnlineModelConfig {
                transducer: OnlineTransducerModelConfig {
                    encoder: path(model_file(dir, "encoder")?),
                    decoder: path(model_file(dir, "decoder")?),
                    joiner: path(model_file(dir, "joiner")?),
                },
                tokens: path(dir.join("tokens.txt")),
                num_threads: 1,
                ..Default::default()
            },
            keywords_threshold: threshold,
            keywords_buf: Some(keywords),
            ..Default::default()
        };
        let spotter = KeywordSpotter::create(&config).context("loading the wake word model")?;
        let stream = spotter.create_stream();
        Ok(Self { spotter, stream })
    }
}

impl WakeWord for SherpaWake {
    fn feed(&mut self, samples: &[f32]) -> bool {
        self.stream.accept_waveform(RATE as i32, samples);
        let mut heard = false;
        while self.spotter.is_ready(&self.stream) {
            self.spotter.decode(&self.stream);
            if let Some(result) = self.spotter.get_result(&self.stream)
                && !result.keyword.is_empty()
            {
                heard = true;
                self.spotter.reset(&self.stream);
            }
        }
        heard
    }

    fn reset(&mut self) {
        self.stream = self.spotter.create_stream();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::listen::{Heard, Listener};

    /// Speaks `text` with a Mac voice as 16 kHz audio, padded with quiet.
    fn say(voice: &str, text: &str) -> Vec<f32> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("say.wav");
        let ok = std::process::Command::new("say")
            .args([
                "-v",
                voice,
                "-o",
                path.to_str().unwrap(),
                "--data-format=LEI16@16000",
                text,
            ])
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let bytes = std::fs::read(&path).unwrap();
        let mut audio = vec![0.0; RATE / 2];
        audio.extend(
            bytes[44..]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
        );
        audio.extend(vec![0.0; RATE]);
        // A real microphone always hisses a little; perfect silence confuses the model.
        let mut state = 12_345u32;
        for s in &mut audio {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *s += ((state >> 16) as f32 / 32_768.0 - 1.0) * 0.002;
        }
        audio
    }

    /// Adds rumbly noise about 10 dB below the speech, roughly like music in the room.
    fn noisy(audio: &[f32], seed: u32) -> Vec<f32> {
        let loud = (audio.iter().map(|s| s * s).sum::<f32>() / audio.len() as f32).sqrt();
        let mut state = seed.wrapping_mul(2_654_435_761) | 1;
        let mut brown = 0.0f32;
        audio
            .iter()
            .map(|s| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let white = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
                brown = 0.97 * brown + white;
                s + brown * loud * 0.316 / 3.4
            })
            .collect()
    }

    /// With the keyword model: `VOICE_MODEL_DIR=… cargo test -- --ignored live_wake --nocapture`.
    #[test]
    #[ignore]
    fn live_wake() {
        let Ok(dir) = std::env::var("VOICE_MODEL_DIR") else {
            return;
        };
        let voices = ["Samantha", "Daniel", "Karen", "Fred"];
        let phrase = std::env::var("WAKE_PHRASE").unwrap_or_else(|_| DEFAULT_PHRASE.to_string());
        let should_wake = [
            phrase.clone(),
            format!("{phrase}, play graceland"),
            format!("{phrase} next song"),
        ];
        let should_not = [
            "hey music",
            "hello music",
            "hello there, how are you",
            "hey jude",
            "okay google",
            "hey mom",
            "the muzak in the elevator",
            "what's the weather like today",
            "hey max, can you pass the salt",
        ];
        for threshold in [0.25] {
            let (mut woke, mut missed, mut false_wakes) = (0, 0, 0);
            for (i, voice) in voices.iter().enumerate() {
                for (said, want) in should_wake
                    .iter()
                    .map(|p| (p.as_str(), true))
                    .chain(should_not.iter().map(|p| (*p, false)))
                {
                    let clean = say(voice, said);
                    for (kind, audio) in [
                        ("clean", clean.clone()),
                        ("noise", noisy(&clean, i as u32 + 1)),
                    ] {
                        let mut listener = Listener::new(
                            SherpaWake::load(Path::new(&dir), &phrase, threshold).unwrap(),
                        );
                        let heard: Vec<Heard> =
                            audio.chunks(1_600).flat_map(|c| listener.push(c)).collect();
                        let got = heard.first() == Some(&Heard::Woke);
                        match (want, got) {
                            (true, true) => woke += 1,
                            (true, false) => {
                                missed += 1;
                                println!("  missed: {voice} {kind} {said:?}");
                            }
                            (false, true) => {
                                false_wakes += 1;
                                println!("  false wake: {voice} {kind} {said:?}");
                            }
                            (false, false) => {}
                        }
                        // After the wake word, the rest of the request is recorded.
                        if want && got && said.contains(',') {
                            assert!(
                                heard.iter().any(|h| matches!(h, Heard::Request(_))),
                                "{voice} {kind}: no request after the wake word: {heard:?}"
                            );
                        }
                    }
                }
            }
            println!(
                "threshold {threshold}: woke {woke}, missed {missed}, false wakes {false_wakes}"
            );
        }
    }
}

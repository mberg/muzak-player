//! Waits for the wake word, then records the request until the speaker stops.

/// Samples per second everywhere in voice.
pub const RATE: usize = 16_000;
/// Loudness is judged over 20 ms frames.
const FRAME: usize = RATE / 50;
/// The request ends after this much quiet once speech started.
const END_SILENCE: usize = RATE * 8 / 10;
/// Give up if nobody speaks within this long after the wake word.
const NO_SPEECH: usize = RATE * 4;
/// The longest request.
const MAX_REQUEST: usize = RATE * 8;
/// Speech must be this much louder than the room was before the wake word...
const SPEECH_OVER_NOISE: f32 = 2.5;
/// ...and at least this loud (RMS of samples in -1.0..1.0).
const MIN_SPEECH: f32 = 0.01;
/// The room's noise counts for at most this when judging speech, so loud music before the
/// wake word (which is then turned down) can't hide the request.
const MAX_NOISE: f32 = 0.02;

/// Spots the wake word in a stream of 16 kHz audio.
pub trait WakeWord {
    /// True when the wake word ends in this audio.
    fn feed(&mut self, samples: &[f32]) -> bool;
    fn reset(&mut self);
}

#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    Woke,
    /// The request's audio, 16 kHz mono.
    Request(Vec<f32>),
    /// The wake word, then nothing.
    Nothing,
}

enum Phase {
    Waiting,
    Recording {
        audio: Vec<f32>,
        spoke: bool,
        quiet: usize,
    },
}

pub struct Listener<W: WakeWord> {
    wake: W,
    phase: Phase,
    /// The room's loudness, followed while waiting.
    noise: f32,
    /// Part of a frame left over from the last chunk.
    partial: Vec<f32>,
}

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt()
}

impl<W: WakeWord> Listener<W> {
    pub fn new(wake: W) -> Self {
        Self {
            wake,
            phase: Phase::Waiting,
            noise: MIN_SPEECH / SPEECH_OVER_NOISE,
            partial: Vec::new(),
        }
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. })
    }

    /// Feeds 16 kHz mono audio; returns what was heard, if anything.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Heard> {
        let mut out = Vec::new();
        self.partial.extend_from_slice(samples);
        let frames = self.partial.len() / FRAME;
        let audio: Vec<f32> = self.partial.drain(..frames * FRAME).collect();
        for frame in audio.chunks_exact(FRAME) {
            if let Some(heard) = self.frame(frame) {
                out.push(heard);
            }
        }
        out
    }

    fn frame(&mut self, frame: &[f32]) -> Option<Heard> {
        let level = rms(frame);
        match &mut self.phase {
            Phase::Waiting => {
                self.noise = 0.98 * self.noise + 0.02 * level;
                if self.wake.feed(frame) {
                    self.wake.reset();
                    self.phase = Phase::Recording {
                        audio: Vec::with_capacity(MAX_REQUEST),
                        spoke: false,
                        quiet: 0,
                    };
                    return Some(Heard::Woke);
                }
                None
            }
            Phase::Recording {
                audio,
                spoke,
                quiet,
            } => {
                audio.extend_from_slice(frame);
                let threshold = (self.noise.min(MAX_NOISE) * SPEECH_OVER_NOISE).max(MIN_SPEECH);
                if level > threshold {
                    *spoke = true;
                    *quiet = 0;
                } else {
                    *quiet += frame.len();
                }
                let done = if *spoke {
                    *quiet >= END_SILENCE || audio.len() >= MAX_REQUEST
                } else {
                    audio.len() >= NO_SPEECH
                };
                if !done {
                    return None;
                }
                let heard = if *spoke {
                    Heard::Request(std::mem::take(audio))
                } else {
                    Heard::Nothing
                };
                self.phase = Phase::Waiting;
                Some(heard)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "Hears" the wake word when it gets a frame of exactly 0.5.
    struct FakeWake;
    impl WakeWord for FakeWake {
        fn feed(&mut self, samples: &[f32]) -> bool {
            samples.iter().all(|s| *s == 0.5)
        }
        fn reset(&mut self) {}
    }

    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * RATE as f32) as usize]
    }

    fn speech(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.2 * (i as f32 * 0.05).sin())
            .collect()
    }

    fn wake() -> Vec<f32> {
        vec![0.5; FRAME]
    }

    #[test]
    fn records_from_the_wake_word_until_the_speaker_stops() {
        let mut l = Listener::new(FakeWake);
        assert!(l.push(&silence(1.0)).is_empty());
        assert_eq!(l.push(&wake()), [Heard::Woke]);
        assert!(l.push(&speech(1.5)).is_empty());
        assert!(l.is_recording());
        // A short pause mid-sentence doesn't end it.
        assert!(l.push(&silence(0.4)).is_empty());
        assert!(l.push(&speech(0.5)).is_empty());
        let heard = l.push(&silence(1.0));
        let [Heard::Request(audio)] = heard.as_slice() else {
            panic!("expected a request, got {heard:?}")
        };
        // 2 s of speech, the pause, and the quiet that ended it.
        assert!((2.6..3.4).contains(&(audio.len() as f32 / RATE as f32)));
        assert!(!l.is_recording());
    }

    #[test]
    fn the_wake_word_then_quiet_is_nothing() {
        let mut l = Listener::new(FakeWake);
        l.push(&wake());
        assert!(l.push(&silence(3.9)).is_empty());
        assert_eq!(l.push(&silence(0.2)), [Heard::Nothing]);
    }

    #[test]
    fn a_long_request_is_cut_off() {
        let mut l = Listener::new(FakeWake);
        l.push(&wake());
        let heard = l.push(&speech(9.0));
        assert!(matches!(heard.as_slice(), [Heard::Request(a)] if a.len() == MAX_REQUEST));
    }

    #[test]
    fn loud_music_before_the_wake_word_does_not_hide_the_request() {
        let mut l = Listener::new(FakeWake);
        // Loud music for a while, then the wake word; the music is turned down to a hum.
        l.push(&speech(5.0));
        l.push(&wake());
        let hum: Vec<f32> = speech(1.0).iter().map(|s| s * 0.05).collect();
        assert!(l.push(&hum).is_empty());
        let request: Vec<f32> = speech(1.0).iter().map(|s| s * 0.4).collect();
        l.push(&request);
        let heard = l.push(&hum.repeat(2));
        assert!(matches!(heard.as_slice(), [Heard::Request(_)]), "{heard:?}");
    }
}

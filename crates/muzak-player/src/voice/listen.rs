//! Waits for the wake word, then records the request until the speaker stops.

/// Samples per second everywhere in voice.
pub const RATE: usize = 16_000;
/// Loudness is judged over 20 ms frames.
const FRAME: usize = RATE / 50;
/// The request ends after this much quiet once speech started. Long enough for the pauses
/// people make mid-sentence.
const END_SILENCE: usize = RATE * 12 / 10;
/// Give up if nobody speaks within this long after the wake word.
const NO_SPEECH: usize = RATE * 4;
/// The longest request.
const MAX_REQUEST: usize = RATE * 8;
/// Audio kept from before the wake word was recognised. The model needs a moment after the
/// wake word ends, and a quick "ziggy stop" is over by then; this keeps it.
const PRE_ROLL: usize = RATE;
/// After the wake word, people often pause before the request. If nothing new is said, the
/// request ends after this much quiet (the command may already be in the audio from before).
const PAUSE_AFTER_WAKE: usize = RATE * 2;
/// Speech must be this much louder than the room was before the wake word...
const SPEECH_OVER_NOISE: f32 = 3.0;
/// ...and at least this loud (RMS of samples in -1.0..1.0). A MacBook microphone measured a
/// quiet room at about 0.0005 and ordinary speech at 0.003-0.015.
const MIN_SPEECH: f32 = 0.0015;
/// The room's noise counts for at most this when judging speech, so loud music before the
/// wake word (which is then turned down) can't hide the request.
const MAX_NOISE: f32 = 0.002;

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
        /// Stop here, whatever is said.
        limit: usize,
        /// Speech since recording started.
        spoke: bool,
        /// The audio starts with what was said before the wake word was recognised.
        after_wake: bool,
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
    /// The last `PRE_ROLL` samples heard while waiting.
    recent: std::collections::VecDeque<f32>,
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
            recent: std::collections::VecDeque::with_capacity(PRE_ROLL + FRAME),
        }
    }

    /// Starts recording now, without the wake word (the microphone button).
    pub fn listen_now(&mut self) {
        self.wake.reset();
        self.recent.clear();
        self.phase = Phase::Recording {
            audio: Vec::with_capacity(MAX_REQUEST),
            limit: MAX_REQUEST,
            spoke: false,
            after_wake: false,
            quiet: 0,
        };
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
                self.recent.extend(frame);
                let extra = self.recent.len().saturating_sub(PRE_ROLL);
                self.recent.drain(..extra);
                if self.wake.feed(frame) {
                    self.wake.reset();
                    // The request starts with what was just said, so a quick command right after
                    // the wake word isn't lost.
                    let mut audio = Vec::with_capacity(MAX_REQUEST + PRE_ROLL);
                    audio.extend(self.recent.drain(..));
                    self.phase = Phase::Recording {
                        limit: audio.len() + MAX_REQUEST,
                        audio,
                        spoke: false,
                        after_wake: true,
                        quiet: 0,
                    };
                    return Some(Heard::Woke);
                }
                None
            }
            Phase::Recording {
                audio,
                limit,
                spoke,
                after_wake,
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
                let done = if audio.len() >= *limit {
                    true
                } else if *spoke {
                    *quiet >= END_SILENCE
                } else if *after_wake {
                    *quiet >= PAUSE_AFTER_WAKE
                } else {
                    audio.len() >= NO_SPEECH
                };
                if !done {
                    return None;
                }
                // After the wake word, the audio from before may hold a quick command; Gemini
                // decides. From the button, silence is nothing.
                let heard = if *spoke || *after_wake {
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

    /// Speech at a level a MacBook microphone measured (RMS about 0.01).
    fn speech(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.014 * (i as f32 * 0.05).sin())
            .collect()
    }

    /// A quiet room (RMS about 0.0005), like the same microphone measured.
    fn room(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.0007 * (i as f32 * 1.3).sin())
            .collect()
    }

    fn wake() -> Vec<f32> {
        vec![0.5; FRAME]
    }

    fn request(heard: &[Heard]) -> &[f32] {
        match heard {
            [Heard::Request(audio)] => audio,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    fn secs(audio: &[f32]) -> f32 {
        audio.len() as f32 / RATE as f32
    }

    #[test]
    fn records_from_just_before_the_wake_word_until_the_speaker_stops() {
        let mut l = Listener::new(FakeWake);
        assert!(l.push(&silence(1.0)).is_empty());
        assert_eq!(l.push(&wake()), [Heard::Woke]);
        assert!(l.push(&speech(1.5)).is_empty());
        assert!(l.is_recording());
        // A short pause mid-sentence doesn't end it.
        assert!(l.push(&silence(0.4)).is_empty());
        assert!(l.push(&speech(0.5)).is_empty());
        let heard = l.push(&silence(1.4));
        // A second from before the wake word, 2 s of speech, the pause, and the quiet after.
        assert!(
            (4.3..4.9).contains(&secs(request(&heard))),
            "{}",
            secs(request(&heard))
        );
        assert!(!l.is_recording());
    }

    #[test]
    fn a_command_said_while_the_wake_word_was_being_recognised_is_kept() {
        let mut l = Listener::new(FakeWake);
        l.push(&silence(1.0));
        // "next song" was said before the wake word model caught up.
        let command = speech(0.6);
        l.push(&command);
        assert_eq!(l.push(&wake()), [Heard::Woke]);
        // Nothing more is said: the request ends after the longer pause.
        assert!(l.push(&silence(1.5)).is_empty());
        let audio = request(&l.push(&silence(0.6))).to_vec();
        let kept = audio
            .windows(command.len())
            .any(|w| w == command.as_slice());
        assert!(kept, "the command before the wake word is in the request");
    }

    #[test]
    fn a_pause_after_the_wake_word_does_not_end_the_request() {
        let mut l = Listener::new(FakeWake);
        l.push(&silence(1.0));
        l.push(&wake());
        assert!(l.push(&silence(1.5)).is_empty(), "waiting for the request");
        assert!(l.push(&speech(1.0)).is_empty());
        let heard = l.push(&silence(1.4));
        assert!(secs(request(&heard)) > 3.0);
    }

    #[test]
    fn soft_speech_and_pauses_mid_sentence_do_not_end_the_request() {
        let mut l = Listener::new(FakeWake);
        l.push(&room(2.0));
        l.listen_now();
        l.push(&speech(1.0));
        // Softer words, then a breath.
        let soft: Vec<f32> = speech(0.6).iter().map(|s| s * 0.35).collect();
        assert!(l.push(&soft).is_empty());
        assert!(
            l.push(&room(0.9)).is_empty(),
            "a pause mid-sentence isn't the end"
        );
        assert!(l.push(&speech(0.8)).is_empty());
        let heard = l.push(&room(1.4));
        assert!(secs(request(&heard)) > 3.0);
    }

    #[test]
    fn the_microphone_button_waits_for_speech() {
        let mut l = Listener::new(FakeWake);
        l.listen_now();
        assert!(l.is_recording());
        assert!(l.push(&silence(3.9)).is_empty());
        assert_eq!(l.push(&silence(0.2)), [Heard::Nothing]);
        l.listen_now();
        l.push(&speech(1.0));
        let heard = l.push(&silence(1.4));
        assert!((2.1..2.3).contains(&secs(request(&heard))));
    }

    #[test]
    fn a_long_request_is_cut_off() {
        let mut l = Listener::new(FakeWake);
        l.listen_now();
        let heard = l.push(&speech(9.0));
        assert_eq!(request(&heard).len(), MAX_REQUEST);
    }

    #[test]
    fn loud_music_before_the_wake_word_does_not_hide_the_request() {
        let mut l = Listener::new(FakeWake);
        // Loud music for a while, then the wake word; the music is turned down to a hum.
        let music: Vec<f32> = speech(5.0).iter().map(|s| s * 4.0).collect();
        l.push(&music);
        l.push(&wake());
        let hum: Vec<f32> = speech(1.0).iter().map(|s| s * 0.1).collect();
        assert!(l.push(&hum).is_empty());
        let request = speech(1.0);
        l.push(&request);
        let heard = l.push(&hum.repeat(2));
        assert!(matches!(heard.as_slice(), [Heard::Request(_)]), "{heard:?}");
    }
}

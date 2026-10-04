# Voice control

Say a wake word, then ask for music or a book. Everyday commands are understood on the player. Anything else goes to Gemini.

## Decisions so far (2026-10-04)

- **Microphone.** A USB microphone on each player.
- **Open source on the device.** No Picovoice. Its Rust libraries are unmaintained since July 2025.
- **Two paths.**
  - **On the device.** Fixed commands and "play" plus the name of something you own are handled on the Pi, offline and fast.
  - **Gemini.** Anything the Pi can't match cleanly goes to Gemini Flash on a paid key. Paid requests aren't used to train Google's models.
- **Safety rule.** Never act on a guess. A command the Pi isn't sure of goes to Gemini. A name Gemini returns is only played if it matches your library or a real Spotify search result.
- **Engine (decided).** sherpa-onnx streaming transcription, with your library names as hotwords. It beat Moonshine on CPU and memory at the same accuracy in the spike. It gets tested on the real Pi hardware.
- **Worst case (decided).** If the transcription model doesn't fit on the Pi, a small model handles only the key commands on the device, and all other audio goes to Gemini.

Assumed until said otherwise:

- **No spoken replies.** The player answers with a short sound and a message on screen.
- **A custom wake word**, such as "Hey Muzak".

## How a request flows

1. **Wake word.** A small always-on detector hears "Hey Muzak". The music dips and the screen shows "Listening…".
2. **Listen.** Speech is transcribed on the Pi as it comes in, and the audio is kept. Recording stops after a short silence or about 6 seconds.
3. **Match on the device.** The words are compared with:
   - the fixed commands: next, skip, pause, stop, go back, louder, quieter, skip forward, skip back, and sleep timer;
   - "play" followed by one of your playlists, albums, artists or books.

   Only a clean match counts. One stray short word at either end is allowed, never more. Names are compared by spelling and by a sounds-alike form, so "Greysland" can still match Graceland.
4. **Otherwise, Gemini.** The saved audio goes to Gemini along with what the player knows. Gemini returns one command (see below).
5. **Act.** The core runs the command through the same code a tap uses. The music comes back up, and the screen says what happened or "Didn't catch that".

Audio leaves the device only in step 4, and only after the wake word.

## Spike results

The spike in `spikes/voice` ran sherpa-onnx and Moonshine on 120 test clips. Fifteen requests were each spoken by four built-in Mac voices, once clean and once with rumbly background noise. Ten requests were commands and five should go to Gemini, such as "play Paul Simon's first album". Every transcription engine used the same matching rules.

| Engine | Commands handled on the Pi | Wrongly acted on | CPU time for the whole set | Peak memory |
|---|---|---|---|---|
| sherpa-onnx keyword spotting, 3.3M model | 64 of 80 at best | 8 of 40 | about 3 s | not measured |
| sherpa-onnx transcription, 20M model | 3 of 80 | 0 | about 4 s | not measured |
| sherpa-onnx transcription, 2023-06-26 model | 59 of 80 | 0 | 12 s | 230 MB |
| **sherpa-onnx, same model, with library names as hotwords** | **62 of 80** | **0** | **13 s** | **230 MB** |
| Moonshine tiny streaming | 50 of 80 | 0 | 23 s | 330 MB |
| Moonshine tiny streaming, with key terms | 52 of 80 | 0 | 23 s | 325 MB |
| Moonshine small streaming | 58 of 80 | 0 | 48 s | 750 MB |
| Moonshine small streaming, with key terms | 62 of 80 | 0 | 48 s | 730 MB |

The clips total about 320 seconds of audio. CPU time is summed over all cores on an Apple Silicon Mac. sherpa-onnx ran on one thread. Moonshine used its defaults, which spread work across cores, and ran through Python, which adds some memory.

What this shows:

- **Keyword spotting is the wrong tool for commands.** It fires on the first phrase it hears. "Play Paul Simon's first album" triggers "play Paul Simon", and "Paul" triggers "pause". It may still suit the wake word, which is a single phrase.
- **The small 20M model drops about the first second of speech**, so short commands vanish.
- **Transcription plus matching never acted wrongly.** Its 21 misses were misheard names and words, and all of them would go to Gemini.
- **Biasing towards your library names helps both engines.** sherpa-onnx's hotwords took it from 59 to 62. Moonshine's key terms took its small model from 58 to 62.
- **sherpa-onnx is the better fit for the Pi 3 A+.** With hotwords it matches Moonshine's best accuracy, using about a quarter of the CPU time and a third of the memory. Moonshine's tiny model is smaller, but it is less accurate and still needs more of both.
- **Synthetic voices aren't people.** Real voices and real music in the room will change these numbers.

Also confirmed:

- **It builds for the Pi.** The crate builds for 64-bit ARM Linux in the Pi build container, with sherpa-onnx linked in, and runs there.
- **Keyword lists need splitting.** For keyword spotting, keywords must be split into the model's word pieces first. The app would need a sentencepiece library. Transcription doesn't need this.

## The biggest risk: the Pi 3 A+

The Pi 3 A+ has 512 MB of memory, and Spotify playback and the screen already use part of it. The transcription model peaked at about 230 MB on the Mac. Speed should be fine. A Pi 3 core is perhaps 10 to 20 times slower than a Mac core, which would put transcription at roughly 40% to 70% of one core, and only while listening. That is an estimate.

Phase 0 measures this on a real Pi before anything else is built. If it doesn't fit, the options in order are:

1. A smaller sherpa-onnx model that handles only the key commands on the device: next, skip, pause, stop, go back, louder and quieter. Everything else, including "play" plus a name, goes to Gemini as audio. The spike's 3.3M keyword model is one candidate, if it only listens for those few words. That avoids the false matches it had on longer requests.
2. Moonshine tiny streaming with key terms, through its C API. It was less accurate and used more memory in the spike.
3. Gemini only. There's no on-device path, so every request takes about a second and needs the internet.

## Wake word

| Option | Notes |
|---|---|
| microWakeWord | Built for tiny chips, so very light. It has a Rust crate with 64-bit ARM support. Custom wake words are trained with Home Assistant's pipeline. The crate's maturity is unknown. |
| openWakeWord | Widely used, and trains a custom word from synthetic speech in under an hour. Heavier: about 70% of a Pi 3 core for its four stock models. No Rust port, so we'd run its ONNX models ourselves. |
| sherpa-onnx keyword spotting | Already in the build, so there'd be nothing extra to add. It needs testing as a single wake phrase, with music playing. |

Hearing the wake word over the player's own music is the hard part. When the wake word fires, the player lowers the music immediately. Phase 0 tests detection with music playing.

## Gemini

- **One request per utterance.** The request carries the audio, a function list, and context: the current screen, what's playing, and the names of your playlists, albums, artists and books.
- **Functions**, each returning one command:
  - play an item from the library, by ID;
  - search Spotify, with a query and a kind;
  - open a book or resume a book, by ID;
  - controls: pause, resume, next, previous, volume, skip forward or back, sleep timer;
  - not sure, with a couple of options to show on screen.
- **Searches are real.** The app runs the Spotify search and plays the top result, or shows the results to pick from.
- **The key** goes in the device config. It isn't kept in Settings.
- **Costs** about a tenth of a cent per request, estimated.

## Shape in the app

- **A `voice` module** owns the microphone thread (ALSA), the wake word, the on-device transcription and matching, and the Gemini client. It follows the same pattern as the audiobooks service.
- **The core receives three events:**
  - wake heard: dip the music and show the listening overlay;
  - a command, from the device or from Gemini;
  - didn't understand.
- **One command type** maps onto the existing actions: play an item, search, controls, books and the sleep timer. Commands are unit-tested like taps, with no microphone needed.
- **The match list is built from the library** that's already loaded: playlists, albums, artists, books and the commands. It's rebuilt when the library changes.
- **Settings** gets a voice on/off switch and a microphone test that shows a level meter.

## Phases

0. **Measure on the Pi 3 A+.**
   - Memory, CPU, and whether the music stays smooth with the transcription model running.
   - Wake-word detection with music playing.
   - Record the household saying about 30 real requests and rerun the spike on those.
1. **Wake word plus Gemini.** One path for every request, with the listening overlay and music ducking. Built on `feat/voice`:
   - **Wake word.** sherpa-onnx keyword spotting with the 3.3M model. The phrase is split into the model's pieces in Rust, so there's no Python on the Pi.
   - **Wake-word test.** Four Mac voices, clean and with noise, at a threshold of 0.25: "hey muzak" woke on 22 of 24 requests. It also woke on "hey music" 3 times in 8, because the model hears "muzak" much like "music". Any phrase ending in "muzak" has the same problem. "Hey jukebox" never woke falsely, but missed more.
   - **End of the request.** It's judged by loudness compared with the room before the wake word, so loud music that is then turned down can't hide the request.
   - **The overlay.** A "Listening…" pill with a pulsing microphone, then "Working on it…", then the result in the message bar.
2. **On-device commands.** Transcription and matching, with Gemini as the fallback.
3. **Polish.**
   - Showing choices on screen when a request is unclear.
   - Tuning the matching thresholds on real use.
   - A "did that work?" log to improve matching.

## Open questions

- Is "Hey Muzak" the wake word, and does every player use the same one?
- Should there be spoken replies, or is a sound plus the screen enough?

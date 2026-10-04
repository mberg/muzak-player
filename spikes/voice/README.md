# Voice spike

A throwaway test of on-device speech recognition with [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), for the voice control scope in `docs/superpowers/specs/2026-10-04-voice-design.md`.

Two approaches:

- `asr`: streaming transcription with the English zipformer model, then matching the words on the device. Anything that isn't a clean match would go to Gemini.
- `kws`: keyword spotting with the 3.3M keyword model.

## On a Mac

```sh
./get-models.sh
./make-clips.sh
cargo run --release --bin asr -- models/sherpa-onnx-streaming-zipformer-en-2023-06-26 wavs
```

## On the Pi 3 A+

Build in the Pi container, copy the binary, models and clips over, then run:

```sh
/usr/bin/time -v ./asr models/sherpa-onnx-streaming-zipformer-en-2023-06-26 wavs 1
```

The last lines give the share of real time used and the peak memory ("Maximum resident set size").

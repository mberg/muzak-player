//! Streaming transcription, then matching the words on the device; anything else would go to Gemini.
use std::time::Instant;

use voice_spike::{LIBRARY, Score, understand};
use sherpa_onnx::{OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineTransducerModelConfig};

fn read(path: &str) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).unwrap();
    r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (model, wavs) = (&args[1], &args[2]);
    let threads: i32 = args.get(3).map_or(1, |t| t.parse().unwrap());
    let library = LIBRARY;
    let suffix = std::env::var("SUFFIX").unwrap_or_else(|_| "-chunk-16-left-128".into()); let f = |n: &str| Some(format!("{model}/{n}-epoch-99-avg-1{suffix}.int8.onnx"));
    let config = OnlineRecognizerConfig {
        model_config: OnlineModelConfig {
            transducer: OnlineTransducerModelConfig {
                encoder: f("encoder"),
                decoder: f("decoder"),
                joiner: f("joiner"),
            },
            tokens: Some(format!("{model}/tokens.txt")),
            num_threads: threads,
            ..Default::default()
        },
        decoding_method: Some("greedy_search".into()),
        ..Default::default()
    };
    // HOTWORDS=1 biases towards the library names (needs bpe.vocab next to the model).
    let mut config = config;
    if std::env::var("HOTWORDS").is_ok() {
        config.decoding_method = Some("modified_beam_search".into());
        config.max_active_paths = 4;
        config.model_config.modeling_unit = Some("bpe".into());
        config.model_config.bpe_vocab = Some(format!("{model}/bpe.vocab"));
        config.hotwords_score = std::env::var("HOTWORDS").unwrap().parse().unwrap_or(1.5);
        let words: Vec<String> = library.iter().map(|n| n.to_uppercase()).collect();
        config.hotwords_buf = Some(words.join("\n").into_bytes());
    }
    let started = Instant::now();
    let recognizer = OnlineRecognizer::create(&config).expect("recognizer");
    println!("loaded in {:?}", started.elapsed());
    let mut files: Vec<_> = std::fs::read_dir(wavs)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_string_lossy().to_string())
        .filter(|p| p.ends_with(".wav"))
        .collect();
    files.sort();
    let (mut audio_secs, mut busy) = (0.0f64, std::time::Duration::ZERO);
    let mut score = Score::default();
    for path in &files {
        let name = path.rsplit('/').next().unwrap();
        let parts: Vec<&str> = name.trim_end_matches(".wav").split("__").collect();
        let (expect, voice, kind) = (parts[1], parts[2], parts[3]);
        let samples = read(path);
        audio_secs += samples.len() as f64 / 16000.0;
        let stream = recognizer.create_stream();
        let t = Instant::now();
        let chunk_len: usize = std::env::var("CHUNK").ok().map_or(1600, |c| c.parse().unwrap());
        let lead: usize = std::env::var("LEAD").ok().map_or(8000, |c| c.parse().unwrap());
        let mut padded = vec![0.0f32; lead];
        padded.extend_from_slice(&samples);
        padded.extend(std::iter::repeat_n(0.0f32, 8000));
        for chunk in padded.chunks(chunk_len) {
            stream.accept_waveform(16000, chunk);
            while recognizer.is_ready(&stream) {
                recognizer.decode(&stream);
            }
        }
        stream.input_finished();
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
        }
        busy += t.elapsed();
        let heard = recognizer.get_result(&stream).map(|r| r.text).unwrap_or_default();
        let action = understand(&heard, &library);
        let verdict = score.add(expect, action.as_deref());
        println!("{verdict:12} {kind:5} {voice:9} heard {heard:44} -> {action:?}");
    }
    println!("\n{}", score.summary());
    println!(
        "{:.1} s of audio in {:.2} s with {threads} thread(s): {:.1}% of real time",
        audio_secs,
        busy.as_secs_f64(),
        100.0 * busy.as_secs_f64() / audio_secs
    );
}

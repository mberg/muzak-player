//! Runs sherpa-onnx keyword spotting over test clips and reports matches and speed.
use std::time::Instant;

use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig, OnlineModelConfig, OnlineTransducerModelConfig};

fn read(path: &str) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).unwrap();
    r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = &args[1];
    let wavs = &args[2];
    let threads: i32 = args.get(3).map_or(1, |t| t.parse().unwrap());
    let threshold: f32 = args.get(4).map_or(0.25, |t| t.parse().unwrap());
    let keywords = std::fs::read_to_string("keywords.txt".to_string()).unwrap();
    let f = |n: &str| Some(format!("{model}/{n}-epoch-12-avg-2-chunk-16-left-64.int8.onnx"));
    let config = KeywordSpotterConfig {
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
        keywords_threshold: threshold,
        keywords_buf: Some(keywords),
        ..Default::default()
    };
    let started = Instant::now();
    let kws = KeywordSpotter::create(&config).expect("keyword spotter");
    println!("loaded in {:?}", started.elapsed());

    let mut files: Vec<_> = std::fs::read_dir(wavs)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path().to_string_lossy().to_string())
        .filter(|p| p.ends_with(".wav"))
        .collect();
    files.sort();
    let (mut audio_secs, mut busy) = (0.0f64, std::time::Duration::ZERO);
    let (mut right, mut missed, mut wrong, mut false_hits, mut quiet) = (0, 0, 0, 0, 0);
    for path in &files {
        let name = path.rsplit('/').next().unwrap();
        let parts: Vec<&str> = name.trim_end_matches(".wav").split("__").collect();
        let (expect, voice, kind) = (parts[1], parts[2], parts[3]);
        let samples = read(path);
        audio_secs += samples.len() as f64 / 16000.0;
        let stream = kws.create_stream();
        let mut hits = Vec::new();
        let t = Instant::now();
        // Feed 100 ms at a time, like a live microphone.
        for chunk in samples.chunks(1600) {
            stream.accept_waveform(16000, chunk);
            while kws.is_ready(&stream) {
                kws.decode(&stream);
                if let Some(r) = kws.get_result(&stream) {
                    if !r.keyword.is_empty() {
                        hits.push(r.keyword.clone());
                        kws.reset(&stream);
                    }
                }
            }
        }
        busy += t.elapsed();
        let first = hits.first().map(String::as_str).unwrap_or("-");
        let verdict = match (expect, first) {
            ("none", "-") => { quiet += 1; "ok (nothing)" }
            ("none", _) => { false_hits += 1; "FALSE HIT" }
            (_, "-") => { missed += 1; "MISSED" }
            (e, h) if e == h => { right += 1; "ok" }
            _ => { wrong += 1; "WRONG" }
        };
        let text = std::fs::read_to_string(path.replace(&format!("__{kind}.wav"), ".txt")).unwrap_or_default();
        println!("{verdict:12} {kind:5} {voice:9} {text:42} -> {hits:?}");
    }
    println!(
        "\ncommands: {right} right, {missed} missed, {wrong} wrong; other speech: {quiet} ignored, {false_hits} false hits"
    );
    println!(
        "{:.1} s of audio in {:.2} s with {threads} thread(s): {:.1}% of real time",
        audio_secs,
        busy.as_secs_f64(),
        100.0 * busy.as_secs_f64() / audio_secs
    );
}

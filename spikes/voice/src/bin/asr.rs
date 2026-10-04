//! Streaming transcription, then matching the words on the device; anything else would go to Gemini.
use std::time::Instant;

use sherpa_onnx::{OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineTransducerModelConfig};

fn read(path: &str) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).unwrap();
    r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn similarity(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    1.0 - prev[b.len()] as f32 / a.len().max(b.len()).max(1) as f32
}

/// Sounds-alike form: drops vowels after the first letter and merges similar consonants,
/// so "greysland" and "graceland" come out close.
fn sound(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            let mut out = String::new();
            for (i, c) in w.chars().enumerate() {
                let c = match c {
                    'c' | 'k' | 'q' => 'k',
                    's' | 'z' => 's',
                    'b' | 'p' => 'p',
                    'd' | 't' => 't',
                    'f' | 'v' => 'f',
                    'y' => 'i',
                    c => c,
                };
                if i > 0 && "aeiouh".contains(c) {
                    continue;
                }
                if !out.ends_with(c) {
                    out.push(c);
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn close(heard: &str, name: &str, min: f32) -> bool {
    similarity(heard, name) >= min || similarity(&sound(heard), &sound(name)) >= 0.85
}

/// What the device would do with the words: a command, or None (send to Gemini).
fn understand(text: &str, library: &[&str]) -> Option<String> {
    let words: Vec<String> = norm(text)
        .split_whitespace()
        .filter(|w| !["please", "hey", "um", "uh"].contains(w))
        .map(str::to_string)
        .collect();
    let commands: &[(&[&str], &str)] = &[
        (&["next", "next song", "skip this", "skip this song", "next one"], "next_song"),
        (&["skip", "skip it"], "skip"),
        (&["pause", "pause the music", "pause it", "stop", "stop the music"], "pause"),
        (&["go back", "previous", "previous song", "last song"], "go_back"),
        (&["turn it up", "louder", "volume up", "turn up"], "turn_it_up"),
        (&["turn it down", "quieter", "volume down", "turn down"], "turn_it_down"),
    ];
    // Allow one stray short word at either end ("to turn it up and"), never more.
    let mut tries = vec![words.join(" ")];
    if words.len() > 1 && words[0].len() <= 3 {
        tries.push(words[1..].join(" "));
    }
    if words.len() > 1 && words[words.len() - 1].len() <= 3 {
        tries.push(words[..words.len() - 1].join(" "));
    }
    if words.len() > 2 && words[0].len() <= 3 && words[words.len() - 1].len() <= 3 {
        tries.push(words[1..words.len() - 1].join(" "));
    }
    for t in &tries {
        for (phrases, command) in commands {
            if phrases.iter().any(|p| similarity(t, p) >= 0.8) {
                return Some(command.to_string());
            }
        }
    }
    for t in &tries {
        // "play" is often heard as "slay", "flay", "played".
        let Some((first, rest)) = t.split_once(' ') else { continue };
        if !close(first, "play", 0.6) {
            continue;
        }
        let rest = rest.strip_prefix("the album ").unwrap_or(rest);
        // The whole rest must be the name: "paul simon s first album" isn't "paul simon".
        if let Some(name) = library.iter().find(|name| close(rest, &norm(name), 0.75)) {
            return Some(format!("play_{}", name.replace(' ', "_")));
        }
    }
    None
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (model, wavs) = (&args[1], &args[2]);
    let threads: i32 = args.get(3).map_or(1, |t| t.parse().unwrap());
    let library = ["graceland", "the hobbit", "paul simon", "matilda", "road trip"];
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
    let (mut right, mut to_gemini, mut wrong, mut quiet, mut false_hits) = (0, 0, 0, 0, 0);
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
        let verdict = match (expect, action.as_deref()) {
            ("none", None) => { quiet += 1; "ok (Gemini)" }
            ("none", Some(_)) => { false_hits += 1; "FALSE HIT" }
            (_, None) => { to_gemini += 1; "to Gemini" }
            (e, Some(a)) if e == a => { right += 1; "ok" }
            _ => { wrong += 1; "WRONG" }
        };
        println!("{verdict:12} {kind:5} {voice:9} heard {heard:44} -> {action:?}");
    }
    println!("\ncommands: {right} right on device, {to_gemini} sent to Gemini, {wrong} wrong; other speech: {quiet} sent to Gemini, {false_hits} wrongly acted on");
    println!(
        "{:.1} s of audio in {:.2} s with {threads} thread(s): {:.1}% of real time",
        audio_secs,
        busy.as_secs_f64(),
        100.0 * busy.as_secs_f64() / audio_secs
    );
}

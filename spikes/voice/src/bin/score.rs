//! Scores transcripts from another engine: reads "clip path<TAB>transcript" lines on stdin.
use std::io::BufRead;

use voice_spike::{LIBRARY, Score, understand};

fn main() {
    let mut score = Score::default();
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let Some((path, heard)) = line.split_once('\t') else { continue };
        let name = path.rsplit('/').next().unwrap();
        let parts: Vec<&str> = name.trim_end_matches(".wav").split("__").collect();
        let (expect, voice, kind) = (parts[1], parts[2], parts[3]);
        let action = understand(heard, &LIBRARY);
        let verdict = score.add(expect, action.as_deref());
        println!("{verdict:12} {kind:5} {voice:9} heard {heard:44} -> {action:?}");
    }
    println!("\n{}", score.summary());
}

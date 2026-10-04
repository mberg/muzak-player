//! Matching transcribed words to commands on the device, shared by the test tools.

pub fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn similarity(a: &str, b: &str) -> f32 {
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
pub fn sound(s: &str) -> String {
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

pub fn close(heard: &str, name: &str, min: f32) -> bool {
    similarity(heard, name) >= min || similarity(&sound(heard), &sound(name)) >= 0.85
}

/// What the device would do with the words: a command, or None (send to Gemini).
pub fn understand(text: &str, library: &[&str]) -> Option<String> {
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


/// Tallies how a clip was handled and returns a verdict word for it.
#[derive(Default)]
pub struct Score {
    pub right: usize,
    pub to_gemini: usize,
    pub wrong: usize,
    pub quiet: usize,
    pub false_hits: usize,
}

impl Score {
    pub fn add(&mut self, expect: &str, action: Option<&str>) -> &'static str {
        match (expect, action) {
            ("none", None) => { self.quiet += 1; "ok (Gemini)" }
            ("none", Some(_)) => { self.false_hits += 1; "FALSE HIT" }
            (_, None) => { self.to_gemini += 1; "to Gemini" }
            (e, Some(a)) if e == a => { self.right += 1; "ok" }
            _ => { self.wrong += 1; "WRONG" }
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "commands: {} right on device, {} sent to Gemini, {} wrong; other speech: {} sent to Gemini, {} wrongly acted on",
            self.right, self.to_gemini, self.wrong, self.quiet, self.false_hits
        )
    }
}

pub const LIBRARY: [&str; 5] = ["graceland", "the hobbit", "paul simon", "matilda", "road trip"];

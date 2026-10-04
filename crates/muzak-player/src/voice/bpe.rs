//! Splits a phrase into a SentencePiece BPE model's word pieces, as the keyword spotter needs.
//!
//! Reads the `bpe.model` file that ships with sherpa-onnx models directly, so the device needs
//! no Python. Only what keyword phrases need is supported: upper-case ASCII words.

use std::collections::HashMap;

/// Score for a character the vocabulary doesn't have, as SentencePiece does.
const UNKNOWN_PENALTY: f32 = -100.0;

pub struct Bpe {
    scores: HashMap<String, f32>,
}

/// Reads a protobuf varint, moving `at` past it.
fn varint(bytes: &[u8], at: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*at)?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Calls `field` for each (number, wire type, payload) in a protobuf message.
fn fields<'a>(bytes: &'a [u8], mut field: impl FnMut(u64, &'a [u8], u64)) -> Option<()> {
    let mut at = 0;
    while at < bytes.len() {
        let key = varint(bytes, &mut at)?;
        let (number, wire) = (key >> 3, key & 7);
        match wire {
            0 => {
                let v = varint(bytes, &mut at)?;
                field(number, &[], v);
            }
            1 => {
                field(number, bytes.get(at..at + 8)?, 0);
                at += 8;
            }
            2 => {
                let len = varint(bytes, &mut at)? as usize;
                field(number, bytes.get(at..at + len)?, 0);
                at += len;
            }
            5 => {
                field(number, bytes.get(at..at + 4)?, 0);
                at += 4;
            }
            _ => return None,
        }
    }
    Some(())
}

impl Bpe {
    /// Parses a SentencePiece `ModelProto`: field 1 holds pieces as (1: text, 2: score).
    pub fn from_model(bytes: &[u8]) -> Option<Bpe> {
        let mut scores = HashMap::new();
        fields(bytes, |number, payload, _| {
            if number != 1 {
                return;
            }
            let (mut text, mut score) = (None, 0.0f32);
            fields(payload, |n, p, _| match n {
                1 => text = std::str::from_utf8(p).ok().map(str::to_string),
                2 if p.len() == 4 => score = f32::from_le_bytes([p[0], p[1], p[2], p[3]]),
                _ => {}
            });
            if let Some(text) = text {
                scores.insert(text, score);
            }
        })?;
        (!scores.is_empty()).then_some(Bpe { scores })
    }

    /// "hey muzak" → ["▁HE", "Y", "▁MU", "Z", "A", "K"].
    ///
    /// sherpa-onnx's English models are SentencePiece unigram models (the file is still called
    /// `bpe.model`), so each word is split into the pieces with the best total score.
    pub fn encode(&self, phrase: &str) -> Vec<String> {
        let mut out = Vec::new();
        // Letters and apostrophes only: punctuation isn't in the vocabulary.
        let cleaned: String = phrase
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '\'' {
                    c
                } else {
                    ' '
                }
            })
            .collect();
        for word in cleaned.split_whitespace() {
            let chars: Vec<char> = format!("▁{}", word.to_uppercase()).chars().collect();
            // best[i]: (score, start of the last piece) for the first i characters.
            let mut best: Vec<Option<(f32, usize)>> = vec![None; chars.len() + 1];
            best[0] = Some((0.0, 0));
            for end in 1..=chars.len() {
                for start in 0..end {
                    let Some((before, _)) = best[start] else {
                        continue;
                    };
                    let piece: String = chars[start..end].iter().collect();
                    let score = match self.scores.get(&piece) {
                        Some(s) => *s,
                        // A single unknown character still has to go somewhere.
                        None if end - start == 1 => UNKNOWN_PENALTY,
                        None => continue,
                    };
                    let total = before + score;
                    if best[end].is_none_or(|(b, _)| total > b) {
                        best[end] = Some((total, start));
                    }
                }
            }
            let mut pieces = Vec::new();
            let mut end = chars.len();
            while end > 0 {
                let (_, start) = best[end].expect("every prefix is reachable");
                pieces.push(chars[start..end].iter().collect::<String>());
                end = start;
            }
            pieces.reverse();
            out.extend(pieces);
        }
        out
    }

    /// One keyword line for sherpa-onnx: pieces, then "@" and a label. None when the phrase has
    /// nothing the model can spell; sherpa-onnx would exit the process on an unknown piece.
    pub fn keyword_line(&self, phrase: &str, label: &str) -> Option<String> {
        let pieces = self.encode(phrase);
        if pieces.is_empty() || pieces.iter().any(|p| !self.scores.contains_key(p)) {
            return None;
        }
        Some(format!("{} @{label}", pieces.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_sentencepiece() {
        let path = format!(
            "{}/tests/fixtures/voice/bpe.model",
            env!("CARGO_MANIFEST_DIR")
        );
        let bpe = Bpe::from_model(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(bpe.scores.len(), 500);
        // Expected splits come from Python's sentencepiece with the same model.
        let cases = [
            ("hey muzak", "▁HE Y ▁MU Z A K"),
            ("HEY JARVIS", "▁HE Y ▁JA R VI S"),
            ("play graceland", "▁PLAY ▁GRA CE LAND"),
            ("next song", "▁NEXT ▁SO NG"),
            ("ok computer", "▁O K ▁COMP U TER"),
            ("hello  world", "▁HE LL O ▁WORLD"),
        ];
        for (phrase, want) in cases {
            assert_eq!(bpe.encode(phrase).join(" "), want, "{phrase}");
        }
        assert_eq!(
            bpe.keyword_line("hey muzak", "wake").as_deref(),
            Some("▁HE Y ▁MU Z A K @wake")
        );
        assert_eq!(bpe.encode("hey, muzak!").join(" "), "▁HE Y ▁MU Z A K");
        assert_eq!(bpe.keyword_line("¿¡", "wake"), None);
        assert_eq!(bpe.keyword_line("hey müzak", "wake"), None);
    }
}

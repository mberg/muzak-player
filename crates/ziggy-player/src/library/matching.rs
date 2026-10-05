//! Case- and accent-insensitive matching for searching the user's own library.

/// Lowercases and strips common Latin accents, so "beyonce" matches "Beyoncé".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
            'ñ' | 'ń' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
            'ý' | 'ÿ' => 'y',
            'ś' | 'š' => 's',
            'ź' | 'ż' | 'ž' => 'z',
            'ł' => 'l',
            other => other,
        })
        .collect()
}

/// True when every word of `query` appears somewhere in `text`, ignoring case and accents.
/// An empty query matches nothing.
pub fn matches(text: &str, query: &str) -> bool {
    let text = fold(text);
    let words: Vec<String> = fold(query).split_whitespace().map(str::to_string).collect();
    !words.is_empty() && words.iter().all(|w| text.contains(w.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_lowercases_and_strips_accents() {
        assert_eq!(fold("Beyoncé Ñandú"), "beyonce nandu");
    }

    #[test]
    fn every_word_must_match_in_any_order() {
        assert!(matches("Abbey Road", "road ab"));
        assert!(matches("Beyoncé", "BEYONCE"));
        assert!(!matches("Abbey Road", "road x"));
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(!matches("Abbey Road", "  "));
    }
}

//! Sentence splitting for sentence-level models.
//!
//! OPUS-MT is trained on single sentences and tends to drop or merge parts of longer input, so
//! each block is translated sentence by sentence. A boundary is `.`, `!`, `?` or `…` (plus any
//! closing quotes or brackets), then whitespace, then an uppercase letter or an opening quote.
//! Common Swedish and English abbreviations don't end a sentence.

/// Lowercased words that end with a dot without ending the sentence (`kl. 23`, `t.ex. Word`).
const ABBREVIATIONS: [&str; 22] = [
    "t.ex", "bl.a", "kl", "ca", "dvs", "osv", "s.k", "jfr", "resp", "nr", "st", "p.g.a", "m.m",
    "e.g", "i.e", "mr", "mrs", "ms", "dr", "prof", "vs", "no",
];

const CLOSING: [char; 6] = ['"', '\'', ')', ']', '”', '’'];
const OPENING: [char; 5] = ['"', '\'', '(', '“', '‘'];

/// Split `text` into sentences, trimmed. Empty or whitespace-only text gives no sentences.
pub fn split_sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (_, c) = chars[i];
        if !matches!(c, '.' | '!' | '?' | '…') {
            i += 1;
            continue;
        }
        // Include repeated terminators and closing quotes/brackets.
        let mut end = i + 1;
        while end < chars.len()
            && (matches!(chars[end].1, '.' | '!' | '?' | '…') || CLOSING.contains(&chars[end].1))
        {
            end += 1;
        }
        let mut next = end;
        while next < chars.len() && chars[next].1.is_whitespace() {
            next += 1;
        }
        let boundary = next > end
            && next < chars.len()
            && (chars[next].1.is_uppercase() || OPENING.contains(&chars[next].1))
            && !(c == '.' && is_abbreviation(&text[start..chars[i].0]));
        if boundary {
            let byte_end = chars.get(end).map_or(text.len(), |&(b, _)| b);
            push(&mut out, &text[start..byte_end]);
            start = chars[next].0;
        }
        i = next.max(i + 1);
    }
    push(&mut out, &text[start..]);
    out
}

fn push<'a>(out: &mut Vec<&'a str>, s: &'a str) {
    let s = s.trim();
    if !s.is_empty() {
        out.push(s);
    }
}

/// Whether the last word of `before` (the text up to a `.`) is a known abbreviation or a single
/// letter (an initial).
fn is_abbreviation(before: &str) -> bool {
    let word = before
        .rsplit(|c: char| c.is_whitespace() || OPENING.contains(&c))
        .next()
        .unwrap_or_default();
    let lower = word.to_lowercase();
    (word.chars().count() == 1 && word.chars().all(char::is_alphabetic))
        || ABBREVIATIONS.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sentences_and_keeps_punctuation() {
        assert_eq!(
            split_sentences(
                "Säkerhetskopieringen körs varje natt. Alla filer kopieras! Klart? \"Ja.\" Bra."
            ),
            [
                "Säkerhetskopieringen körs varje natt.",
                "Alla filer kopieras!",
                "Klart?",
                "\"Ja.\"",
                "Bra."
            ]
        );
    }

    #[test]
    fn single_sentences_and_blank_text() {
        assert_eq!(
            split_sentences("  Spara ändringar?  "),
            ["Spara ändringar?"]
        );
        assert_eq!(split_sentences("Avbryt"), ["Avbryt"]);
        assert!(split_sentences(" \n ").is_empty());
    }

    #[test]
    fn numbers_abbreviations_and_lowercase_do_not_split() {
        for text in [
            "Rabatten på 12,5 % gäller till kl. 23:59",
            "Förfallodatum: 30 okt. 2026",
            "Version 4.2.1 släpptes 1 november",
            "Läs mer på https://exempel.se/villkor. tack",
            "Se t.ex. Dokument och Bilder",
            "Call Dr. Smith or e.g. Support",
            "Written by J. R. R. Tolkien",
        ] {
            assert_eq!(split_sentences(text), [text], "{text}");
        }
    }
}

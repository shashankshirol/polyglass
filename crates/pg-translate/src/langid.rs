//! Swedish/English language identification for "Auto (Swedish/English)" (SV-04) and the
//! already-in-target-language passthrough (TR-07).
//!
//! A small lexicon + morphology scorer: unambiguous function words, common UI verbs, å/ä/ö,
//! and characteristic suffixes. It only has to separate Swedish from English, and runs per
//! block in microseconds, so no model is needed.

use pg_core::LangTag;

/// Blocks shorter than this (in characters) are not identified (SV-04): they are treated as the
/// non-target language of the pair.
pub const MIN_IDENTIFY_CHARS: usize = 20;

/// Minimum total evidence before a verdict is trusted.
const MIN_EVIDENCE: f32 = 1.0;

/// Words that are common in one language and rare or absent in the other. Words shared by
/// both (in, man, under, men, till, start, version, ...) are deliberately left out.
const SV_WORDS: &[&str] = &[
    "och",
    "att",
    "det",
    "som",
    "en",
    "ett",
    "är",
    "på",
    "för",
    "med",
    "har",
    "inte",
    "av",
    "den",
    "de",
    "om",
    "jag",
    "du",
    "vi",
    "ni",
    "kan",
    "ska",
    "eller",
    "så",
    "från",
    "när",
    "vid",
    "efter",
    "här",
    "där",
    "vill",
    "alla",
    "alltid",
    "mer",
    "mycket",
    "nu",
    "också",
    "sig",
    "sina",
    "sitt",
    "din",
    "dina",
    "ditt",
    "min",
    "mina",
    "mitt",
    "vår",
    "våra",
    "deras",
    "hur",
    "vad",
    "vem",
    "vilken",
    "vilket",
    "vilka",
    "varför",
    "ja",
    "nej",
    "ingen",
    "inget",
    "inga",
    "bara",
    "redan",
    "igen",
    "får",
    "måste",
    "kommer",
    "blir",
    "finns",
    "gör",
    "sedan",
    "eftersom",
    "innan",
    "utan",
    "mellan",
    "genom",
    "över",
    "mot",
    "ut",
    "upp",
    "ner",
    "ned",
    "nya",
    "nytt",
    "fler",
    "fil",
    "filer",
    "spara",
    "avbryt",
    "stäng",
    "öppna",
    "sök",
    "hjälp",
    "logga",
    "konto",
    "lösenord",
    "användare",
    "välj",
    "klicka",
    "tryck",
    "ändra",
    "bort",
    "dölj",
    "kopiera",
    "klistra",
    "radera",
    "ladda",
    "sida",
    "uppdatera",
    "dela",
    "skicka",
    "försök",
    "gick",
    "fortsätt",
    "fortsätta",
    "tillbaka",
    "inställningar",
    "meddelanden",
];

const EN_WORDS: &[&str] = &[
    "the",
    "and",
    "of",
    "to",
    "is",
    "are",
    "was",
    "were",
    "been",
    "being",
    "for",
    "with",
    "this",
    "that",
    "these",
    "those",
    "you",
    "your",
    "it",
    "its",
    "we",
    "our",
    "they",
    "their",
    "them",
    "on",
    "at",
    "by",
    "from",
    "or",
    "not",
    "can't",
    "will",
    "would",
    "should",
    "could",
    "have",
    "has",
    "had",
    "do",
    "does",
    "did",
    "an",
    "what",
    "which",
    "who",
    "when",
    "where",
    "how",
    "why",
    "any",
    "more",
    "some",
    "no",
    "yes",
    "if",
    "then",
    "than",
    "there",
    "about",
    "into",
    "out",
    "up",
    "down",
    "new",
    "file",
    "files",
    "save",
    "cancel",
    "close",
    "open",
    "settings",
    "search",
    "help",
    "sign",
    "account",
    "password",
    "user",
    "select",
    "click",
    "press",
    "change",
    "remove",
    "show",
    "hide",
    "copy",
    "paste",
    "delete",
    "download",
    "page",
    "please",
    "also",
    "only",
    "just",
    "my",
    "me",
    "he",
    "she",
    "him",
    "her",
    "us",
    "again",
    "almost",
    "learn",
    "use",
    "latest",
    "available",
    "feature",
    "powered",
    "notes",
    "terms",
    "policy",
];

/// Result of identifying one block.
#[derive(Debug, Clone, PartialEq)]
pub struct Identification {
    pub lang: LangTag,
    /// Share of the evidence pointing at `lang` (0.5–1.0).
    pub confidence: f32,
}

/// Raw evidence scores (exposed for tests and tuning).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scores {
    pub sv: f32,
    pub en: f32,
}

pub fn scores(text: &str) -> Scores {
    let mut s = Scores::default();
    for raw in text.split(|c: char| !(c.is_alphabetic() || c == '\'')) {
        let raw = raw.trim_matches('\'');
        if raw.is_empty() {
            continue;
        }
        // Standalone pronoun "I" vs Swedish preposition "i".
        if raw == "i" {
            s.sv += 1.0;
            continue;
        }
        if raw == "I" {
            s.en += 0.5;
            continue;
        }
        let w = raw.to_lowercase();
        if w.contains('\'') {
            s.en += 0.5;
        }
        if SV_WORDS.contains(&w.as_str()) {
            s.sv += 1.0;
        }
        if EN_WORDS.contains(&w.as_str()) {
            s.en += 1.0;
        }
        if w.contains(['å', 'ä', 'ö']) {
            s.sv += 1.0;
        }
        if w.contains('w') {
            s.en += 0.5;
        }
        if w.starts_with("th") {
            s.en += 0.5;
        }
        if ["sj", "tj", "skj", "stj"].iter().any(|p| w.starts_with(p)) {
            s.sv += 0.5;
        }
        let n = w.chars().count();
        if n > 4 {
            if [
                "arna", "orna", "erna", "heten", "ningen", "ningar", "ning", "het",
            ]
            .iter()
            .any(|suf| w.ends_with(suf))
            {
                s.sv += 0.5;
            } else if w.ends_with("ing") || w.ends_with("ly") || w.ends_with("ness") {
                s.en += 0.5;
            } else if w.ends_with("ed") {
                s.en += 0.3;
            }
        }
    }
    s
}

/// Identify `text` as Swedish or English, or `None` if there isn't enough evidence.
pub fn identify(text: &str) -> Option<Identification> {
    let s = scores(text);
    let total = s.sv + s.en;
    if total < MIN_EVIDENCE || (s.sv - s.en).abs() < f32::EPSILON {
        return None;
    }
    let (lang, top) = if s.sv > s.en {
        (LangTag::sv(), s.sv)
    } else {
        (LangTag::en(), s.en)
    };
    Some(Identification {
        lang,
        confidence: top / total,
    })
}

/// The other language of the Swedish/English pair.
pub fn other_of_sv_en(target: &LangTag) -> LangTag {
    if target.is_swedish() {
        LangTag::en()
    } else {
        LangTag::sv()
    }
}

/// Decide the language of a block in "Auto (Swedish/English)" mode (SV-04).
///
/// Short blocks and blocks without a clear verdict are treated as the non-target language, so
/// they are translated rather than passed through.
pub fn decide_sv_en(text: &str, target: &LangTag) -> LangTag {
    let fallback = other_of_sv_en(target);
    if text.trim().chars().count() < MIN_IDENTIFY_CHARS {
        return fallback;
    }
    identify(text).map_or(fallback, |id| id.lang)
}

/// True if a block in language `block_lang` must be sent to the backend for `target` (TR-07).
pub fn needs_translation(block_lang: &LangTag, target: &LangTag) -> bool {
    !block_lang.same_language(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Truth {
        blocks: Vec<TruthBlock>,
    }

    #[derive(serde::Deserialize)]
    struct TruthBlock {
        text: String,
        lang: LangTag,
        #[serde(default)]
        short: bool,
    }

    fn mixed_fixture() -> Truth {
        let raw = include_str!("../../../tests/fixtures/mixed-sv-en/truth.json");
        serde_json::from_str(raw).expect("fixture parses")
    }

    fn accuracy_for_target(target: &LangTag) -> f32 {
        let truth = mixed_fixture();
        let mut correct = 0;
        for b in &truth.blocks {
            let expected = if b.short {
                other_of_sv_en(target)
            } else {
                b.lang.clone()
            };
            let got = decide_sv_en(&b.text, target);
            if got == expected {
                correct += 1;
            } else {
                eprintln!("wrong: {:?} → {got} (expected {expected})", b.text);
            }
        }
        correct as f32 / truth.blocks.len() as f32
    }

    #[test]
    fn sv04_mixed_fixture_accuracy_target_en() {
        let acc = accuracy_for_target(&LangTag::en());
        assert!(acc >= 0.9, "accuracy {acc}");
    }

    #[test]
    fn sv04_mixed_fixture_accuracy_target_sv() {
        let acc = accuracy_for_target(&LangTag::sv());
        assert!(acc >= 0.9, "accuracy {acc}");
    }

    #[derive(serde::Deserialize)]
    struct EvalSet {
        source: LangTag,
        segments: Vec<EvalSegment>,
    }

    #[derive(serde::Deserialize)]
    struct EvalSegment {
        source: String,
    }

    /// Held-out check: the scorer was tuned on `mixed-sv-en`, never on the SV-07 evaluation
    /// sets. Their 400 sources (UI, web and document text, one language per set) are decided
    /// against both targets; blocks under 20 characters follow the fallback rule instead, so
    /// only longer blocks are counted.
    #[test]
    fn sv04_held_out_eval_sources() {
        let sets: Vec<EvalSet> = [
            include_str!("../../../tests/eval/sv-en/segments.json"),
            include_str!("../../../tests/eval/en-sv/segments.json"),
        ]
        .iter()
        .map(|raw| serde_json::from_str(raw).expect("eval set parses"))
        .collect();
        let (mut correct, mut total) = (0, 0);
        for set in &sets {
            for s in &set.segments {
                if s.source.chars().count() < MIN_IDENTIFY_CHARS {
                    continue;
                }
                for target in [LangTag::en(), LangTag::sv()] {
                    total += 1;
                    let got = decide_sv_en(&s.source, &target);
                    if got == set.source {
                        correct += 1;
                    } else {
                        eprintln!("wrong: {:?} → {got} (target {target})", s.source);
                    }
                }
            }
        }
        let acc = correct as f32 / total as f32;
        eprintln!("held-out: {correct}/{total} = {acc}");
        assert!(total > 300, "{total}");
        assert!(acc >= 0.9, "accuracy {acc}");
    }

    #[test]
    fn short_blocks_are_non_target() {
        assert_eq!(decide_sv_en("Cancel", &LangTag::en()), LangTag::sv());
        assert_eq!(decide_sv_en("Spara", &LangTag::sv()), LangTag::en());
    }

    #[test]
    fn no_evidence_is_none() {
        assert_eq!(identify("Microsoft Teams Premium 365"), None);
    }

    #[test]
    fn tr07_passthrough() {
        let en_gb = LangTag::parse("en-GB").unwrap();
        assert!(!needs_translation(&LangTag::en(), &en_gb));
        assert!(needs_translation(&LangTag::sv(), &en_gb));
    }
}

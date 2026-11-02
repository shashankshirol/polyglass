//! Language identification for Auto mode, e.g. "Auto (Swedish/English)" (SV-04, MDL-07), and
//! the already-in-target-language passthrough (TR-07).
//!
//! A small lexicon + morphology scorer per language (Swedish, English, German): unambiguous
//! function words, common UI verbs, letters like å/ä/ö/ü/ß, and characteristic suffixes. It only
//! has to tell the two languages of the selected pair apart, and runs per block in microseconds,
//! so no model is needed. A word counts for a language only if the other language of the pair
//! doesn't use it too. A pair with a language that has no profile here still works: every block
//! is then translated (nothing passes through).

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

/// German words for German/English identification (M3-9). German words that are also common
/// English words (`will`, `also`, `was`, `an`, `am`, `man`, `so`, `in`, ...) are left out; they
/// are listed in [`DE_SHARED`] so they don't count as English either.
const DE_WORDS: &[&str] = &[
    "und",
    "oder",
    "aber",
    "der",
    "die",
    "das",
    "dem",
    "den",
    "des",
    "ein",
    "eine",
    "einen",
    "einem",
    "einer",
    "ist",
    "sind",
    "nicht",
    "kein",
    "keine",
    "mit",
    "für",
    "auf",
    "aus",
    "bei",
    "nach",
    "von",
    "vom",
    "zum",
    "zur",
    "zu",
    "im",
    "um",
    "ich",
    "du",
    "sie",
    "wir",
    "ihr",
    "ihre",
    "ihren",
    "ihrer",
    "sich",
    "es",
    "auch",
    "noch",
    "nur",
    "schon",
    "wenn",
    "dass",
    "weil",
    "wie",
    "wer",
    "wird",
    "werden",
    "wurde",
    "wurden",
    "kann",
    "können",
    "muss",
    "müssen",
    "soll",
    "sollte",
    "möchten",
    "möchtest",
    "haben",
    "hat",
    "habe",
    "sein",
    "ja",
    "nein",
    "bitte",
    "danke",
    "alle",
    "alles",
    "mehr",
    "neu",
    "neue",
    "neuen",
    "jetzt",
    "hier",
    "dies",
    "diese",
    "dieser",
    "dieses",
    "über",
    "unter",
    "durch",
    "gegen",
    "ohne",
    "zwischen",
    "datei",
    "dateien",
    "speichern",
    "abbrechen",
    "schließen",
    "öffnen",
    "suchen",
    "hilfe",
    "anmelden",
    "abmelden",
    "konto",
    "passwort",
    "kennwort",
    "benutzer",
    "auswählen",
    "klicken",
    "ändern",
    "entfernen",
    "löschen",
    "anzeigen",
    "ausblenden",
    "kopieren",
    "einfügen",
    "herunterladen",
    "seite",
    "einstellungen",
    "weiter",
    "zurück",
    "fertig",
    "fehler",
    "ordner",
    "aktualisieren",
    "teilen",
    "senden",
];

/// Common English words that are German words too: in a German/English decision they count for
/// neither side.
const DE_SHARED: &[&str] = &[
    "will", "also", "was", "an", "am", "man", "so", "in", "war", "bin", "hand", "name", "final",
];

/// Languages with a profile here. Auto mode between two of them identifies each block; between
/// other languages every block is treated as the source language (translated).
pub const PROFILED: [&str; 3] = ["sv", "en", "de"];

/// Whether [`decide`] can tell `lang` apart from another profiled language.
pub fn has_profile(lang: &LangTag) -> bool {
    PROFILED.contains(&lang.primary())
}

fn words(lang: &str) -> &'static [&'static str] {
    match lang {
        "sv" => SV_WORDS,
        "en" => EN_WORDS,
        "de" => DE_WORDS,
        _ => &[],
    }
}

/// Words of `other` that `lang` shares; they count for neither.
fn shared(lang: &str, other: &str) -> &'static [&'static str] {
    match (lang, other) {
        ("en", "de") | ("de", "en") => DE_SHARED,
        _ => &[],
    }
}

const SV_SUFFIXES: [&str; 8] = [
    "arna", "orna", "erna", "heten", "ningen", "ningar", "ning", "het",
];
const DE_SUFFIXES: [&str; 8] = [
    "ung", "ungen", "keit", "heit", "lich", "isch", "chen", "schaft",
];

/// Evidence that one lowercased word is in `lang` rather than `other`.
fn word_score(lang: &str, other: &str, w: &str) -> f32 {
    let mut s = 0.0;
    let n = w.chars().count();
    if words(lang).contains(&w) && !words(other).contains(&w) && !shared(lang, other).contains(&w) {
        s += 1.0;
    }
    match lang {
        "sv" => {
            if w.contains(['å', 'ä', 'ö']) {
                s += 1.0;
            }
            if ["sj", "tj", "skj", "stj"].iter().any(|p| w.starts_with(p)) {
                s += 0.5;
            }
            if n > 4 && SV_SUFFIXES.iter().any(|suf| w.ends_with(suf)) {
                s += 0.5;
            }
        }
        "de" => {
            if w.contains(['ä', 'ö', 'ü', 'ß']) {
                s += 1.0;
            }
            if w.starts_with("sch") {
                s += 0.5;
            }
            if n > 4 && DE_SUFFIXES.iter().any(|suf| w.ends_with(suf)) {
                s += 0.5;
            }
        }
        "en" => {
            if w.contains('\'') {
                s += 0.5;
            }
            // Swedish has almost no w; German has many.
            if other == "sv" && w.contains('w') {
                s += 0.5;
            }
            if w.starts_with("th") {
                s += 0.5;
            }
            // Against Swedish, a Swedish suffix wins (`-ning` ends in `-ing`).
            let swedish_suffix = other == "sv" && SV_SUFFIXES.iter().any(|suf| w.ends_with(suf));
            if n > 4 && !swedish_suffix {
                if w.ends_with("ing") || w.ends_with("ly") || w.ends_with("ness") {
                    s += 0.5;
                } else if w.ends_with("ed") {
                    s += 0.3;
                }
            }
        }
        _ => {}
    }
    s
}

/// Evidence for `a` and for `b` in `text`, or `None` unless both have a profile and differ.
pub fn scores(text: &str, a: &LangTag, b: &LangTag) -> Option<(f32, f32)> {
    let (a, b) = (a.primary(), b.primary());
    if !PROFILED.contains(&a) || !PROFILED.contains(&b) || a == b {
        return None;
    }
    let (mut sa, mut sb) = (0.0, 0.0);
    for raw in text.split(|c: char| !(c.is_alphabetic() || c == '\'')) {
        let raw = raw.trim_matches('\'');
        if raw.is_empty() {
            continue;
        }
        // The English pronoun "I" vs the Swedish preposition "i".
        if raw == "I" && (a == "en" || b == "en") {
            if a == "en" {
                sa += 0.5;
            } else {
                sb += 0.5;
            }
            continue;
        }
        if raw == "i" && (a == "sv" || b == "sv") {
            if a == "sv" {
                sa += 1.0;
            } else {
                sb += 1.0;
            }
            continue;
        }
        let w = raw.to_lowercase();
        sa += word_score(a, b, &w);
        sb += word_score(b, a, &w);
    }
    Some((sa, sb))
}

/// Result of identifying one block.
#[derive(Debug, Clone, PartialEq)]
pub struct Identification {
    pub lang: LangTag,
    /// Share of the evidence pointing at `lang` (0.5–1.0).
    pub confidence: f32,
}

/// Identify `text` as `a` or `b`, or `None` if there isn't enough evidence or a language has no
/// profile.
pub fn identify(text: &str, a: &LangTag, b: &LangTag) -> Option<Identification> {
    let (sa, sb) = scores(text, a, b)?;
    let total = sa + sb;
    if total < MIN_EVIDENCE || (sa - sb).abs() < f32::EPSILON {
        return None;
    }
    let (lang, top) = if sa > sb { (a, sa) } else { (b, sb) };
    Some(Identification {
        lang: lang.clone(),
        confidence: top / total,
    })
}

/// Decide the language of a block in Auto mode (SV-04): `source` or `target`.
///
/// Short blocks, blocks without a clear verdict and languages without a profile are treated as
/// the source (the non-target language), so they are translated rather than passed through.
pub fn decide(text: &str, source: &LangTag, target: &LangTag) -> LangTag {
    if text.trim().chars().count() < MIN_IDENTIFY_CHARS {
        return source.clone();
    }
    identify(text, source, target).map_or_else(|| source.clone(), |id| id.lang)
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
            let source = other_of_sv_en(target);
            let expected = if b.short {
                source.clone()
            } else {
                b.lang.clone()
            };
            let got = decide(&b.text, &source, target);
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
                    let got = decide(&s.source, &other_of_sv_en(&target), &target);
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

    fn other_of_sv_en(target: &LangTag) -> LangTag {
        if target.is_swedish() {
            LangTag::en()
        } else {
            LangTag::sv()
        }
    }

    /// M3-9: the de-en eval set's German sources and English references, decided between German
    /// and English. Written after the German profile, never tuned on it.
    #[test]
    fn m3_9_held_out_german_english() {
        #[derive(serde::Deserialize)]
        struct Segment {
            source: String,
            reference: String,
        }
        #[derive(serde::Deserialize)]
        struct Set {
            segments: Vec<Segment>,
        }
        let set: Set =
            serde_json::from_str(include_str!("../../../tests/eval/de-en/segments.json")).unwrap();
        let (de, en) = (LangTag::parse("de").unwrap(), LangTag::en());
        let (mut correct, mut total) = (0, 0);
        for s in &set.segments {
            for (text, want) in [(&s.source, &de), (&s.reference, &en)] {
                if text.chars().count() < MIN_IDENTIFY_CHARS {
                    continue;
                }
                total += 1;
                let got = decide(text, &de, &en);
                if got == *want {
                    correct += 1;
                } else {
                    eprintln!("wrong: {text:?} → {got}");
                }
            }
        }
        let acc = correct as f32 / total as f32;
        eprintln!("held-out de/en: {correct}/{total} = {acc}");
        assert!(total > 200, "{total}");
        assert!(acc >= 0.9, "accuracy {acc}");
    }

    #[test]
    fn short_blocks_are_non_target() {
        let (sv, en) = (LangTag::sv(), LangTag::en());
        assert_eq!(decide("Cancel", &sv, &en), sv);
        assert_eq!(decide("Spara", &en, &sv), en);
    }

    #[test]
    fn no_evidence_is_none() {
        let (sv, en) = (LangTag::sv(), LangTag::en());
        assert_eq!(identify("Microsoft Teams Premium 365", &sv, &en), None);
    }

    #[test]
    fn mdl07_unprofiled_languages_are_always_translated() {
        let fi = LangTag::parse("fi").unwrap();
        assert!(!has_profile(&fi) && has_profile(&LangTag::parse("de-DE").unwrap()));
        let text = "The quick brown fox jumps over the lazy dog again and again";
        assert_eq!(identify(text, &fi, &LangTag::en()), None);
        assert_eq!(decide(text, &fi, &LangTag::en()), fi);
    }

    #[test]
    fn m3_9_german_and_english_are_told_apart() {
        let (de, en) = (LangTag::parse("de").unwrap(), LangTag::en());
        let german = [
            "Möchten Sie die Änderungen speichern, bevor Sie das Fenster schließen?",
            "Die Datei konnte nicht geöffnet werden.",
            "Ich will das also noch einmal versuchen, wenn es geht.",
            "Einstellungen für die Benachrichtigungen",
        ];
        let english = [
            "Do you want to save your changes before closing the window?",
            "The file could not be opened.",
            "We will also show a warning when the download is finished.",
            "Settings for notifications and sounds",
        ];
        for t in german {
            assert_eq!(decide(t, &de, &en), de, "{t}");
        }
        for t in english {
            assert_eq!(decide(t, &de, &en), en, "{t}");
        }
    }

    #[test]
    fn tr07_passthrough() {
        let en_gb = LangTag::parse("en-GB").unwrap();
        assert!(!needs_translation(&LangTag::en(), &en_gb));
        assert!(needs_translation(&LangTag::sv(), &en_gb));
    }
}

//! Scoring recognized text against ground truth (M0-FIX-3; OCR-04, SV-03).
//!
//! Both texts are NFC-normalized and every run of whitespace (line breaks included) becomes one
//! space, so a line wrapped differently is not an error; case and punctuation are.
//! - **CER**: character edit distance (Levenshtein) / characters in the reference.
//! - **WER**: the same over whitespace-separated words.
//! - **Swedish letters** (SV-03): the reference's å ä ö Å Ä Ö that the alignment doesn't map to
//!   the same letter, and what each became (a substitution or a deletion).
//!
//! Scores add up ([`Score`] implements `AddAssign`), so a fixture set's rate is total errors over
//! total reference characters, not a mean of per-image rates.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::AddAssign;

use serde::{Serialize, Serializer};
use unicode_normalization::UnicodeNormalization;

/// The letters SV-03 scores separately.
pub const SWEDISH_LETTERS: [char; 6] = ['å', 'ä', 'ö', 'Å', 'Ä', 'Ö'];

/// `errors` out of `total`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Rate {
    pub errors: usize,
    pub total: usize,
}

impl Rate {
    /// `errors / total`, or 0 when there is nothing to get wrong.
    pub fn value(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.errors as f64 / self.total as f64
        }
    }
}

impl AddAssign for Rate {
    fn add_assign(&mut self, rhs: Rate) {
        self.errors += rhs.errors;
        self.total += rhs.total;
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:.2}% ({}/{})",
            self.value() * 100.0,
            self.errors,
            self.total
        )
    }
}

/// How a reference Swedish letter was misread: `to` is `None` when it was deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Misread {
    pub from: char,
    pub to: Option<char>,
}

impl fmt::Display for Misread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.to {
            Some(to) => write!(f, "{}→{to}", self.from),
            None => write!(f, "{}→∅", self.from),
        }
    }
}

/// Accuracy of one recognized text, or of several added together.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Score {
    pub chars: Rate,
    pub words: Rate,
    /// Reference å/ä/ö/Å/Ä/Ö not reproduced exactly.
    pub swedish: Rate,
    /// Count of each Swedish-letter misreading.
    #[serde(serialize_with = "misreads_as_strings")]
    pub misreads: BTreeMap<Misread, usize>,
}

fn misreads_as_strings<S: Serializer>(
    m: &BTreeMap<Misread, usize>,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.collect_map(m.iter().map(|(k, v)| (k.to_string(), v)))
}

impl Score {
    pub fn cer(&self) -> f64 {
        self.chars.value()
    }

    pub fn wer(&self) -> f64 {
        self.words.value()
    }

    /// Misreadings seen at least `min_count` times: systematic, not incidental (SV-03).
    pub fn systematic(&self, min_count: usize) -> Vec<(Misread, usize)> {
        self.misreads
            .iter()
            .filter(|(_, n)| **n >= min_count)
            .map(|(m, n)| (*m, *n))
            .collect()
    }
}

impl AddAssign<&Score> for Score {
    fn add_assign(&mut self, rhs: &Score) {
        self.chars += rhs.chars;
        self.words += rhs.words;
        self.swedish += rhs.swedish;
        for (m, n) in &rhs.misreads {
            *self.misreads.entry(*m).or_default() += n;
        }
    }
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CER {}  WER {}  åäö {}",
            self.chars, self.words, self.swedish
        )?;
        if !self.misreads.is_empty() {
            let list: Vec<String> = self
                .misreads
                .iter()
                .map(|(m, n)| format!("{m}×{n}"))
                .collect();
            write!(f, "  [{}]", list.join(" "))?;
        }
        Ok(())
    }
}

/// NFC with whitespace runs collapsed to single spaces and trimmed.
pub fn normalize(s: &str) -> String {
    s.nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Score `recognized` against `reference` (both normalized first).
pub fn score(recognized: &str, reference: &str) -> Score {
    let (hyp, reference) = (normalize(recognized), normalize(reference));
    let hyp_chars: Vec<char> = hyp.chars().collect();
    let ref_chars: Vec<char> = reference.chars().collect();
    let (char_edits, aligned) = align(&hyp_chars, &ref_chars);
    let hyp_words: Vec<&str> = hyp.split(' ').filter(|w| !w.is_empty()).collect();
    let ref_words: Vec<&str> = reference.split(' ').filter(|w| !w.is_empty()).collect();
    let (word_edits, _) = align(&hyp_words, &ref_words);

    let mut swedish = Rate::default();
    let mut misreads = BTreeMap::new();
    for (want, got) in ref_chars.iter().zip(aligned) {
        if !SWEDISH_LETTERS.contains(want) {
            continue;
        }
        swedish.total += 1;
        if got != Some(*want) {
            swedish.errors += 1;
            *misreads
                .entry(Misread {
                    from: *want,
                    to: got,
                })
                .or_default() += 1;
        }
    }
    Score {
        chars: Rate {
            errors: char_edits,
            total: ref_chars.len(),
        },
        words: Rate {
            errors: word_edits,
            total: ref_words.len(),
        },
        swedish,
        misreads,
    }
}

/// Character error rate of `recognized` against `reference`.
pub fn cer(recognized: &str, reference: &str) -> f64 {
    score(recognized, reference).cer()
}

/// Levenshtein distance between `hyp` and `reference`, and for each reference item the
/// hypothesis item aligned to it (`None` where it was deleted). Ties prefer a match or
/// substitution over a deletion or insertion.
fn align<T: PartialEq + Copy>(hyp: &[T], reference: &[T]) -> (usize, Vec<Option<T>>) {
    let (n, m) = (reference.len(), hyp.len());
    let width = m + 1;
    let mut d = vec![0u32; (n + 1) * width];
    for (j, cell) in d.iter_mut().enumerate().take(width) {
        *cell = j as u32;
    }
    for i in 1..=n {
        d[i * width] = i as u32;
        for j in 1..=m {
            let sub = d[(i - 1) * width + j - 1] + u32::from(reference[i - 1] != hyp[j - 1]);
            let del = d[(i - 1) * width + j] + 1;
            let ins = d[i * width + j - 1] + 1;
            d[i * width + j] = sub.min(del).min(ins);
        }
    }
    let mut aligned = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 {
        let here = d[i * width + j];
        if j > 0 && here == d[(i - 1) * width + j - 1] + u32::from(reference[i - 1] != hyp[j - 1]) {
            aligned[i - 1] = Some(hyp[j - 1]);
            i -= 1;
            j -= 1;
        } else if here == d[(i - 1) * width + j] + 1 {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    (d[n * width + m] as usize, aligned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_and_rates() {
        assert_eq!(align(&['s', 'i', 't'], &['k', 'i', 't']).0, 1);
        let s = score("sitting", "kitten");
        assert_eq!(
            s.chars,
            Rate {
                errors: 3,
                total: 6
            }
        );
        assert_eq!(cer("ändringama", "ändringarna"), 2.0 / 11.0);
        assert_eq!(cer("", ""), 0.0);
        assert_eq!(cer("", "abc"), 1.0);
        assert_eq!(
            cer("abc", ""),
            0.0,
            "no reference characters: rate is 0 by definition"
        );
    }

    #[test]
    fn whitespace_and_line_breaks_are_not_errors() {
        let s = score("Spara\nändringar?  ", "Spara ändringar?");
        assert_eq!(s.chars.errors, 0);
        assert_eq!(
            s.words,
            Rate {
                errors: 0,
                total: 2
            }
        );
        // Case and punctuation are.
        assert_eq!(score("spara ändringar", "Spara ändringar?").chars.errors, 2);
    }

    #[test]
    fn decomposed_input_is_normalized() {
        // "ä" as a + combining diaeresis.
        let s = score("a\u{308}ndra", "ändra");
        assert_eq!(s.chars.errors, 0);
        assert_eq!(
            s.swedish,
            Rate {
                errors: 0,
                total: 1
            }
        );
    }

    #[test]
    fn word_error_rate() {
        let s = score(
            "Vill du spara dem innan",
            "Vill du spara dem innan du stänger?",
        );
        assert_eq!(
            s.words,
            Rate {
                errors: 2,
                total: 7
            }
        );
    }

    #[test]
    fn swedish_letters_report_substitutions_and_deletions() {
        let s = score("Andringar for ar", "Ändringar för år");
        assert_eq!(
            s.swedish,
            Rate {
                errors: 3,
                total: 3
            }
        );
        let want: BTreeMap<Misread, usize> = [('Ä', 'A'), ('å', 'a'), ('ö', 'o')]
            .into_iter()
            .map(|(from, to)| (Misread { from, to: Some(to) }, 1))
            .collect();
        assert_eq!(s.misreads, want);

        let s = score("fr", "för");
        assert_eq!(
            s.misreads.keys().copied().collect::<Vec<_>>(),
            [Misread {
                from: 'ö',
                to: None
            }]
        );
        assert_eq!(s.misreads.keys().next().unwrap().to_string(), "ö→∅");
    }

    #[test]
    fn scores_add_up_and_flag_systematic_misreads() {
        let mut total = Score::default();
        for _ in 0..3 {
            total += &score("for", "för");
        }
        total += &score("hår", "här");
        assert_eq!(
            total.chars,
            Rate {
                errors: 4,
                total: 12
            }
        );
        assert_eq!(
            total.swedish,
            Rate {
                errors: 4,
                total: 4
            }
        );
        let systematic = total.systematic(3);
        assert_eq!(
            systematic,
            [(
                Misread {
                    from: 'ö',
                    to: Some('o')
                },
                3
            )]
        );
        assert!(total.to_string().contains("ö→o×3"), "{total}");
        let json = serde_json::to_value(&total).unwrap();
        assert_eq!(json["misreads"]["ö→o"], 3);
    }
}

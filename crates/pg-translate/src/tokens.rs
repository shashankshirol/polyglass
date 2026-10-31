//! Preserved-token checks (SV-05).
//!
//! Numbers, amounts, percentages, dates, times, URLs, e-mail addresses and ID numbers must keep
//! the same digits in the same order. Separators may be localized (`1 234,50 kr` →
//! `SEK 1,234.50`) only if the value is unchanged.

use pg_core::LangTag;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Url,
    Email,
    /// A number normalized to `[-]digits[.digits]` using the text language's separators.
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// URL/e-mail verbatim, or the normalized number.
    pub value: String,
}

/// Why a translation fails the SV-05 check. Carries no source text (PRIV-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenMismatch {
    /// URL or e-mail missing or altered (index into the source's URL/e-mail tokens).
    Verbatim { index: usize },
    /// The digit sequence differs.
    Digits,
    /// Same digits but a different numeric value (e.g. decimal separator mistranslated).
    Value { index: usize },
}

/// Thousands/decimal conventions for a language.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NumberStyle {
    pub(crate) decimal: char,
    /// Characters accepted as thousands separators (only when followed by exactly 3 digits).
    pub(crate) thousands: &'static [char],
}

pub(crate) fn style_for(lang: &LangTag) -> NumberStyle {
    match lang.primary() {
        // Swedish: space / no-break space / narrow no-break space for thousands, comma decimal.
        "sv" | "fi" | "nb" | "nn" | "no" | "fr" | "pl" | "cs" => NumberStyle {
            decimal: ',',
            thousands: &[' ', '\u{a0}', '\u{202f}'],
        },
        "de" | "da" | "nl" | "es" | "it" | "pt" | "id" | "tr" => NumberStyle {
            decimal: ',',
            thousands: &['.', '\u{a0}', '\u{202f}'],
        },
        _ => NumberStyle {
            decimal: '.',
            thousands: &[',', '\u{a0}', '\u{202f}'],
        },
    }
}

pub(crate) fn is_url_start(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("www.")
}

/// Split off URLs and e-mails (whitespace-delimited words) and return them plus the remaining
/// text with those words removed.
fn split_verbatim(text: &str) -> (Vec<Token>, String) {
    let mut tokens = Vec::new();
    let mut rest = String::with_capacity(text.len());
    for word in text.split_inclusive(char::is_whitespace) {
        let core = word
            .trim_end()
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\''])
            .trim_start_matches(['(', '"', '\'']);
        let kind = if is_url_start(core) {
            Some(TokenKind::Url)
        } else if is_email(core) {
            Some(TokenKind::Email)
        } else {
            None
        };
        match kind {
            Some(kind) => {
                tokens.push(Token {
                    kind,
                    value: core.to_string(),
                });
                rest.push(' ');
            }
            None => rest.push_str(word),
        }
    }
    (tokens, rest)
}

pub(crate) fn is_email(s: &str) -> bool {
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && s.chars().all(|c| !c.is_whitespace())
}

/// Scan numbers in `text` according to `style`.
///
/// Normalized form: optional `-`, integer digits without grouping, then `.`-separated digit
/// groups for the decimal part and for dotted identifiers (`4.2`, `192.168.0.1`), which read
/// the same in every language.
pub(crate) fn scan_numbers(text: &str, style: NumberStyle) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let digit_at = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut v = String::new();
        if i > 0 && matches!(chars[i - 1], '-' | '−') && (i < 2 || !chars[i - 2].is_alphanumeric())
        {
            v.push('-');
        }
        // Integer part; a thousands separator is skipped only before a group of three digits.
        while digit_at(i) {
            v.push(chars[i]);
            i += 1;
            if i < chars.len()
                && style.thousands.contains(&chars[i])
                && group_of_three(&chars, i + 1)
            {
                i += 1;
            }
        }
        // Decimal part (once) and dotted continuations.
        let mut seen_decimal = false;
        while i < chars.len() && digit_at(i + 1) {
            let c = chars[i];
            let is_decimal = c == style.decimal && !seen_decimal;
            let is_dot = c == '.' && !style.thousands.contains(&'.');
            if !(is_decimal || is_dot) {
                break;
            }
            seen_decimal |= is_decimal;
            v.push('.');
            i += 1;
            while digit_at(i) {
                v.push(chars[i]);
                i += 1;
            }
        }
        out.push(v);
    }
    out
}

/// True if exactly three digits start at `at` and are not followed by another digit.
fn group_of_three(chars: &[char], at: usize) -> bool {
    at + 3 <= chars.len()
        && chars[at..at + 3].iter().all(char::is_ascii_digit)
        && chars.get(at + 3).is_none_or(|c| !c.is_ascii_digit())
}

/// Extract preserved tokens from `text` written in `lang`.
pub fn extract(text: &str, lang: &LangTag) -> Vec<Token> {
    let (mut tokens, rest) = split_verbatim(text);
    tokens.extend(
        scan_numbers(&rest, style_for(lang))
            .into_iter()
            .map(|value| Token {
                kind: TokenKind::Number,
                value,
            }),
    );
    tokens
}

fn digits(text: &str) -> String {
    let (_, rest) = split_verbatim(text);
    rest.chars().filter(char::is_ascii_digit).collect()
}

/// Check that `output` (in `out_lang`) preserves the tokens of `source` (in `src_lang`).
pub fn check_preserved(
    source: &str,
    src_lang: &LangTag,
    output: &str,
    out_lang: &LangTag,
) -> Result<(), TokenMismatch> {
    let src = extract(source, src_lang);
    let out = extract(output, out_lang);

    let verbatim = |t: &&Token| t.kind != TokenKind::Number;
    let out_verbatim: Vec<&Token> = out.iter().filter(verbatim).collect();
    for (index, t) in src.iter().filter(verbatim).enumerate() {
        if out_verbatim.get(index) != Some(&t) {
            return Err(TokenMismatch::Verbatim { index });
        }
    }

    if digits(source) != digits(output) {
        return Err(TokenMismatch::Digits);
    }

    // With equal digits, a value can only change through a decimal separator being added,
    // dropped or moved. Integer grouping is ambiguous (`08-123 456 78` looks like thousands
    // grouping in Swedish), so compare only the numbers that have a fractional/dotted part.
    let fractional = |ts: &[Token]| -> Vec<String> {
        ts.iter()
            .filter(|t| t.kind == TokenKind::Number && t.value.contains('.'))
            .map(|t| t.value.trim_start_matches('-').to_owned())
            .collect()
    };
    let (src_frac, out_frac) = (fractional(&src), fractional(&out));
    if src_frac != out_frac {
        let index = src_frac
            .iter()
            .zip(&out_frac)
            .position(|(a, b)| a != b)
            .unwrap_or(src_frac.len().min(out_frac.len()));
        return Err(TokenMismatch::Value { index });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv() -> LangTag {
        LangTag::sv()
    }
    fn en() -> LangTag {
        LangTag::en()
    }

    fn numbers(text: &str, lang: &LangTag) -> Vec<String> {
        extract(text, lang)
            .into_iter()
            .filter(|t| t.kind == TokenKind::Number)
            .map(|t| t.value)
            .collect()
    }

    #[test]
    fn swedish_number_formats() {
        assert_eq!(numbers("Summa: 1 234,50 kr", &sv()), ["1234.50"]);
        assert_eq!(numbers("12,5 %", &sv()), ["12.5"]);
        assert_eq!(numbers("30 okt. 2026", &sv()), ["30", "2026"]);
        assert_eq!(numbers("2026-10-30", &sv()), ["2026", "10", "30"]);
        assert_eq!(numbers("kl. 14:30", &sv()), ["14", "30"]);
        assert_eq!(numbers("1\u{a0}000\u{a0}000 användare", &sv()), ["1000000"]);
        assert_eq!(numbers("Temperatur -5 grader", &sv()), ["-5"]);
    }

    #[test]
    fn english_number_formats() {
        assert_eq!(numbers("Total: SEK 1,234.50", &en()), ["1234.50"]);
        assert_eq!(numbers("12.5%", &en()), ["12.5"]);
        assert_eq!(numbers("items 1,5 and 7", &en()), ["1", "5", "7"]);
    }

    #[test]
    fn localized_separators_pass() {
        assert_eq!(
            check_preserved("Summa: 1 234,50 kr", &sv(), "Total: SEK 1,234.50", &en()),
            Ok(())
        );
        assert_eq!(
            check_preserved(
                "Rabatt 12,5 % till 30 okt. 2026",
                &sv(),
                "12.5% off until 30 Oct 2026",
                &en()
            ),
            Ok(())
        );
    }

    #[test]
    fn changed_value_fails() {
        // Comma kept as-is in English reads as a thousands separator: value changed.
        assert_eq!(
            check_preserved("12,5 %", &sv(), "12,5%", &en()),
            Err(TokenMismatch::Value { index: 0 })
        );
        assert_eq!(
            check_preserved("Pris 199 kr", &sv(), "Price SEK 19.9", &en()),
            Err(TokenMismatch::Value { index: 0 })
        );
        assert_eq!(
            check_preserved("Ordernummer 4471", &sv(), "Order number 4417", &en()),
            Err(TokenMismatch::Digits)
        );
    }

    #[test]
    fn urls_and_emails_verbatim() {
        let src = "Läs mer på https://example.se/hjälp eller mejla support@example.se.";
        let ok = "Read more at https://example.se/hjälp or email support@example.se.";
        assert_eq!(check_preserved(src, &sv(), ok, &en()), Ok(()));
        let bad = "Read more at https://example.com/help or email support@example.se.";
        assert_eq!(
            check_preserved(src, &sv(), bad, &en()),
            Err(TokenMismatch::Verbatim { index: 0 })
        );
    }

    #[test]
    fn dotted_identifiers_match_across_languages() {
        assert_eq!(numbers("version 4.2", &sv()), ["4.2"]);
        assert_eq!(numbers("version 4.2", &en()), ["4.2"]);
        assert_eq!(numbers("IP 192.168.0.1", &sv()), ["192.168.0.1"]);
        assert_eq!(numbers("IP 192.168.0.1", &en()), ["192.168.0.1"]);
        assert_eq!(
            check_preserved("Version 4.2 är klar", &sv(), "Version 4.2 is ready", &en()),
            Ok(())
        );
    }

    #[test]
    fn phone_numbers_with_spaces_pass() {
        assert_eq!(
            check_preserved("Ring 08-123 456 78", &sv(), "Call 08-123 456 78", &en()),
            Ok(())
        );
    }

    #[derive(serde::Deserialize)]
    struct Fixture {
        blocks: Vec<FixtureBlock>,
    }
    #[derive(serde::Deserialize)]
    struct FixtureBlock {
        text: String,
    }

    #[test]
    fn sv05_numbers_fixture_pairs() {
        let sv_fx: Fixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/numbers-sv/truth.json"
        ))
        .unwrap();
        let en_fx: Fixture = serde_json::from_str(include_str!(
            "../../../tests/fixtures/numbers-en/truth.json"
        ))
        .unwrap();
        assert_eq!(sv_fx.blocks.len(), en_fx.blocks.len());
        for (s, e) in sv_fx.blocks.iter().zip(&en_fx.blocks) {
            assert_eq!(
                check_preserved(&s.text, &sv(), &e.text, &en()),
                Ok(()),
                "{}",
                s.text
            );
            assert_eq!(
                check_preserved(&e.text, &en(), &s.text, &sv()),
                Ok(()),
                "{}",
                e.text
            );
        }
    }

    #[test]
    fn digits_inside_urls_are_not_numbers() {
        assert_eq!(
            numbers("se https://x.se/v2/2026", &sv()),
            Vec::<String>::new()
        );
    }
}

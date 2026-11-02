//! Preserved-token checks (SV-05).
//!
//! Numbers, amounts, percentages, dates, times, URLs, e-mail addresses and ID numbers must keep
//! the same digits in the same order. Separators may be localized (`1 234,50 kr` →
//! `SEK 1,234.50`) only if the value is unchanged, and a clock time may change between 12- and
//! 24-hour form (`kl. 15.00` → `3 p.m.`) only if it names the same time (D-10).

use pg_core::LangTag;

use crate::clock;

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

    let (src_rest, out_rest) = (split_verbatim(source).1, split_verbatim(output).1);
    let numbers = check_numbers(&src_rest, src_lang, &out_rest, out_lang);
    if numbers.is_err()
        && let Some((src_rest, out_rest)) = without_converted_times(&src_rest, &out_rest)
    {
        return check_numbers(&src_rest, src_lang, &out_rest, out_lang);
    }
    numbers
}

/// Compare the numbers of two texts that have no URLs or e-mails left.
fn check_numbers(
    source: &str,
    src_lang: &LangTag,
    output: &str,
    out_lang: &LangTag,
) -> Result<(), TokenMismatch> {
    let digits = |text: &str| -> String { text.chars().filter(char::is_ascii_digit).collect() };
    if digits(source) != digits(output) {
        return Err(TokenMismatch::Digits);
    }

    // With equal digits, a value can only change through a decimal separator being added,
    // dropped or moved. Integer grouping is ambiguous (`08-123 456 78` looks like thousands
    // grouping in Swedish), so compare only the numbers that have a fractional/dotted part.
    let fractional = |text: &str, lang: &LangTag| -> Vec<String> {
        scan_numbers(text, style_for(lang))
            .into_iter()
            .filter(|v| v.contains('.'))
            .map(|v| v.trim_start_matches('-').to_owned())
            .collect()
    };
    let (src_frac, out_frac) = (fractional(source, src_lang), fractional(output, out_lang));
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

/// Owner decision D-10: a clock time may change between 12- and 24-hour form if it names the
/// same time of day (`kl. 15.00` → `3 p.m.`, `7:45 a.m.` → `07.45`, `kl. 8 och 17` →
/// `8 a.m. and 5 p.m.`).
///
/// Walks the digits of both texts in order. Where a 12-hour time on one side meets a time of
/// the same value on the other, both are blanked out; every other digit must match one to one.
/// Returns the two texts without those times for the usual number check, or `None` if no such
/// pair lines up (the caller keeps its first answer then).
fn without_converted_times(source: &str, output: &str) -> Option<(String, String)> {
    let mut src: Vec<char> = source.chars().collect();
    let mut out: Vec<char> = output.chars().collect();
    let (src_times, out_times) = (clock::spans(&src), clock::spans(&out));
    if !src_times.iter().chain(&out_times).any(|t| t.twelve_hour) {
        return None;
    }
    let next_digit = |text: &[char], from: usize| {
        (from..text.len())
            .find(|&k| text[k].is_ascii_digit())
            .unwrap_or(text.len())
    };
    let starting_at =
        |times: &[clock::ClockSpan], at: usize| times.iter().find(|t| t.start == at).copied();
    let mut converted = Vec::new();
    let (mut i, mut j) = (0, 0);
    loop {
        i = next_digit(&src, i);
        j = next_digit(&out, j);
        match (i < src.len(), j < out.len()) {
            (false, false) => break,
            (true, true) => {}
            _ => return None,
        }
        if let (Some(a), Some(b)) = (starting_at(&src_times, i), starting_at(&out_times, j))
            && (a.twelve_hour || b.twelve_hour)
            && (a.hour, a.minute) == (b.hour, b.minute)
        {
            converted.push((a, b));
            (i, j) = (a.end, b.end);
            continue;
        }
        if src[i] != out[j] {
            return None;
        }
        (i, j) = (i + 1, j + 1);
    }
    if converted.is_empty() {
        return None;
    }
    // `|` is neither a digit nor a separator, so the numbers around a time stay apart.
    for (a, b) in converted {
        src[a.start..a.end].fill('|');
        out[b.start..b.end].fill('|');
    }
    Some((src.into_iter().collect(), out.into_iter().collect()))
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
    fn d10_clock_time_in_the_other_form_passes() {
        let sv_en = [
            ("Incheckning från kl. 15.00", "Check-in from 3 p.m."),
            ("Incheckning från kl. 15.00", "Check-in from 3:00 PM"),
            (
                "Mötet flyttas till torsdag den 14 maj kl. 10.00.",
                "The meeting is moved to Thursday, May 14, at 10 a.m.",
            ),
            (
                "Föräldramötet hålls i aulan klockan 18.30.",
                "The parent meeting is held in the hall at 6.30 p.m.",
            ),
            (
                "Tåget går 07.45 från Centralstationen.",
                "The train leaves Central Station at 7:45 a.m.",
            ),
            ("Öppet kl. 0.00–12.00", "Open 12 a.m.–12 p.m."),
        ];
        for (s, o) in sv_en {
            assert_eq!(check_preserved(s, &sv(), o, &en()), Ok(()), "{s} → {o}");
        }
        let en_sv = [
            ("The kitchen closes at 10 p.m.", "Köket stänger kl. 22.00."),
            ("The kitchen closes at 10 p.m.", "Köket stänger kl. 22."),
            (
                "Call us Monday to Friday between 8 a.m. and 5 p.m.",
                "Ring oss måndag till fredag mellan kl. 8 och 17.",
            ),
            ("Open 8 a.m.–5 p.m.", "Öppet kl. 8–17"),
            (
                "Delivery on 2026-10-30 at 9:15 a.m.",
                "Leverans 2026-10-30 kl. 09.15",
            ),
        ];
        for (s, o) in en_sv {
            assert_eq!(check_preserved(s, &en(), o, &sv()), Ok(()), "{s} → {o}");
        }
    }

    #[test]
    fn d10_clock_time_with_a_new_value_fails() {
        let check = |s, o| check_preserved(s, &sv(), o, &en());
        assert_eq!(
            check("Incheckning från kl. 15.00", "Check-in from 4 p.m."),
            Err(TokenMismatch::Digits)
        );
        assert_eq!(
            check("Klockan 18.30", "At 6.30 a.m."),
            Err(TokenMismatch::Digits)
        );
        assert_eq!(
            check("Tåget går 07.45", "The train leaves at 7:54 a.m."),
            Err(TokenMismatch::Digits)
        );
        assert_eq!(
            check("Mellan kl. 8 och 17", "Between 8 a.m. and 7 p.m."),
            Err(TokenMismatch::Digits)
        );
        assert_eq!(
            check("Mellan kl. 8 och 17", "Between 5 p.m. and 8 a.m."),
            Err(TokenMismatch::Digits),
            "times in another order"
        );
        assert_eq!(
            check("Rum 15", "Room 3 p.m."),
            Err(TokenMismatch::Digits),
            "a number that isn't a time"
        );
        assert_eq!(
            check("Incheckning kl. 15.00", "Check-in at 15"),
            Err(TokenMismatch::Digits),
            "only 12/24-hour conversions, not dropped minutes"
        );
    }

    #[test]
    fn d10_other_numbers_next_to_a_converted_time_are_still_checked() {
        assert_eq!(
            check_preserved(
                "Mötet den 14 maj kl. 10.00",
                &sv(),
                "The meeting on May 15 at 10 a.m.",
                &en()
            ),
            Err(TokenMismatch::Digits)
        );
        assert_eq!(
            check_preserved(
                "12,5 % rabatt från kl. 15.00",
                &sv(),
                "12,5% off from 3 p.m.",
                &en()
            ),
            Err(TokenMismatch::Value { index: 0 })
        );
        assert_eq!(
            check_preserved(
                "12,5 % rabatt från kl. 15.00",
                &sv(),
                "12.5% off from 3 p.m.",
                &en()
            ),
            Ok(())
        );
    }

    #[test]
    fn d10_numbers_written_as_words_still_fail() {
        assert_eq!(
            check_preserved(
                "Incorrect password. You have 2 attempts left.",
                &en(),
                "Fel lösenord. Du har två försök kvar.",
                &sv()
            ),
            Err(TokenMismatch::Digits)
        );
        assert_eq!(
            check_preserved(
                "You have 2 attempts left until 5 p.m.",
                &en(),
                "Du har två försök kvar till kl. 17.",
                &sv()
            ),
            Err(TokenMismatch::Digits)
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

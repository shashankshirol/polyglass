//! Keep preserved tokens intact through any backend (SV-05).
//!
//! Neural models sometimes rewrite numbers, URLs and e-mail addresses (`1 234,50 kr` →
//! `1,245 SEK`, `kl. 23:59` → `2. 23:59`). [`TokenGuard`] wraps a backend and checks every
//! translation with [`check_preserved`]. A block that fails is translated again with its tokens
//! masked as `{1}`, `{2}`, ... and the tokens are put back afterwards, localized where the
//! verbatim form would read as a different value in the target language. If that fails too,
//! the source text is returned unchanged: a block is never drawn with altered digits.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use pg_core::{LangPair, LangTag};

use crate::tokens::{
    NumberStyle, check_preserved, is_email, is_url_start, scan_numbers, style_for,
};
use crate::{Result, Translator};

/// Separators that keep a number run going when a digit follows (`1 234,50`, `2026-10-30`,
/// `23:59`, `08-123 456 78`, `4.2.1`).
const RUN_SEPARATORS: [char; 9] = [' ', '\u{a0}', '\u{202f}', ',', '.', ':', '-', '/', '−'];

/// A block with its preserved tokens replaced by `{n}` placeholders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    /// `tokens[n - 1]` is what `{n}` stands for.
    pub tokens: Vec<MaskedToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskedToken {
    /// URL or e-mail, restored as is.
    Verbatim(String),
    /// A run of digits and separators, restored localized if needed.
    Number(String),
}

/// Mask URLs, e-mails and number runs. `None` if there is nothing to mask, or the text already
/// contains something that looks like a placeholder.
pub fn mask(text: &str) -> Option<Masked> {
    if has_placeholder(text) {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut tokens = Vec::new();
    let mut plain = String::new();
    for word in text.split_inclusive(char::is_whitespace) {
        let core = word
            .trim_end()
            .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\''])
            .trim_start_matches(['(', '"', '\'']);
        if !core.is_empty() && (is_url_start(core) || is_email(core)) {
            let start = word.find(core).expect("core is a substring of word");
            plain.push_str(&word[..start]);
            mask_numbers(&plain, &mut out, &mut tokens);
            plain.clear();
            tokens.push(MaskedToken::Verbatim(core.to_owned()));
            out.push_str(&format!("{{{}}}", tokens.len()));
            plain.push_str(&word[start + core.len()..]);
        } else {
            plain.push_str(word);
        }
    }
    mask_numbers(&plain, &mut out, &mut tokens);
    (!tokens.is_empty()).then_some(Masked { text: out, tokens })
}

fn mask_numbers(text: &str, out: &mut String, tokens: &mut Vec<MaskedToken>) {
    let chars: Vec<char> = text.chars().collect();
    let digit_at = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while digit_at(i)
            || (i < chars.len() && RUN_SEPARATORS.contains(&chars[i]) && digit_at(i + 1))
        {
            i += 1;
        }
        tokens.push(MaskedToken::Number(chars[start..i].iter().collect()));
        out.push_str(&format!("{{{}}}", tokens.len()));
    }
}

fn has_placeholder(text: &str) -> bool {
    text.match_indices('{')
        .any(|(i, _)| text[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
}

/// Put the tokens back into a translated masked text. `None` if any placeholder is missing,
/// repeated or unknown.
pub fn unmask(translated: &str, masked: &Masked, pair: &LangPair) -> Option<String> {
    let mut out = String::with_capacity(translated.len() + 16);
    let mut seen = vec![false; masked.tokens.len()];
    let mut rest = translated;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = after.find('}')?;
        let n: usize = after[..close].parse().ok()?;
        let token = masked.tokens.get(n.checked_sub(1)?)?;
        if std::mem::replace(&mut seen[n - 1], true) {
            return None;
        }
        match token {
            MaskedToken::Verbatim(v) => out.push_str(v),
            MaskedToken::Number(run) => out.push_str(&localize(run, &pair.source, &pair.target)),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    seen.iter().all(|s| *s).then_some(out)
}

/// Rewrite a number run for the target language, but only if reading it verbatim there would
/// give a different value (`12,5` → `12.5` for English; `4.2` stays). Runs holding several
/// numbers (dates, phone numbers, IDs) are kept as they are.
pub fn localize(run: &str, source: &LangTag, target: &LangTag) -> String {
    let (src, tgt) = (style_for(source), style_for(target));
    let values = scan_numbers(run, src);
    if values.len() != 1 || scan_numbers(run, tgt) == values {
        return run.to_owned();
    }
    let grouped = run.chars().any(|c| src.thousands.contains(&c));
    format_number(&values[0], tgt, grouped).unwrap_or_else(|| run.to_owned())
}

/// Format a normalized number (`1234.50`) in `style`. `None` for dotted identifiers.
fn format_number(value: &str, style: NumberStyle, grouped: bool) -> Option<String> {
    let (int, frac) = match value.split_once('.') {
        Some((_, f)) if f.contains('.') => return None,
        Some((i, f)) => (i, Some(f)),
        None => (value, None),
    };
    let mut out = String::new();
    for (k, c) in int.chars().enumerate() {
        if grouped && k > 0 && (int.len() - k) % 3 == 0 {
            out.push(style.thousands[0]);
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push(style.decimal);
        out.push_str(f);
    }
    Some(out)
}

/// Wraps a backend so its output passes the SV-05 preserved-token check, or falls back to the
/// source text for that block.
#[derive(Debug)]
pub struct TokenGuard<T> {
    inner: T,
    masked_retries: AtomicUsize,
    fallbacks: AtomicUsize,
}

impl<T> TokenGuard<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            masked_retries: AtomicUsize::new(0),
            fallbacks: AtomicUsize::new(0),
        }
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Blocks that needed the masked second pass.
    pub fn masked_retries(&self) -> usize {
        self.masked_retries.load(Ordering::Relaxed)
    }

    /// Blocks returned untranslated because no pass kept their tokens.
    pub fn fallbacks(&self) -> usize {
        self.fallbacks.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl<T: Translator> Translator for TokenGuard<T> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn display_name(&self) -> &str {
        self.inner.display_name()
    }

    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    fn supports(&self, pair: &LangPair) -> bool {
        self.inner.supports(pair)
    }

    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>> {
        let mut out = self.inner.translate(blocks, pair).await?;
        let preserved =
            |src: &str, tr: &str| check_preserved(src, &pair.source, tr, &pair.target).is_ok();
        let failed: Vec<usize> = (0..blocks.len())
            .filter(|&i| !preserved(&blocks[i], &out[i]))
            .collect();
        if failed.is_empty() {
            return Ok(out);
        }
        let masked: Vec<(usize, Masked)> = failed
            .iter()
            .filter_map(|&i| mask(&blocks[i]).map(|m| (i, m)))
            .collect();
        self.masked_retries
            .fetch_add(masked.len(), Ordering::Relaxed);
        let texts: Vec<String> = masked.iter().map(|(_, m)| m.text.clone()).collect();
        let second = if texts.is_empty() {
            Vec::new()
        } else {
            self.inner.translate(&texts, pair).await?
        };
        for &i in &failed {
            let retry = masked
                .iter()
                .position(|(j, _)| *j == i)
                .and_then(|k| unmask(second.get(k)?, &masked[k].1, pair))
                .filter(|t| preserved(&blocks[i], t));
            out[i] = retry.unwrap_or_else(|| {
                self.fallbacks.fetch_add(1, Ordering::Relaxed);
                blocks[i].clone()
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockTranslator;

    fn sv_en() -> LangPair {
        LangPair::sv_en()
    }

    #[test]
    fn masks_urls_emails_and_number_runs() {
        let m =
            mask("Kontakta support@exempel.se eller 08-123 456 78, senast 30 okt. 2026.").unwrap();
        assert_eq!(m.text, "Kontakta {1} eller {2}, senast {3} okt. {4}.");
        assert_eq!(
            m.tokens,
            [
                MaskedToken::Verbatim("support@exempel.se".into()),
                MaskedToken::Number("08-123 456 78".into()),
                MaskedToken::Number("30".into()),
                MaskedToken::Number("2026".into()),
            ]
        );
        let m = mask("Läs villkoren på https://exempel.se/villkor.").unwrap();
        assert_eq!(m.text, "Läs villkoren på {1}.");
        assert_eq!(mask("Spara ändringar?"), None);
        assert_eq!(
            mask("Mall {1} och 2"),
            None,
            "existing placeholder-like text"
        );
    }

    #[test]
    fn localizes_only_when_the_value_would_change() {
        let (sv, en) = (LangTag::sv(), LangTag::en());
        assert_eq!(localize("1 234,50", &sv, &en), "1,234.50");
        assert_eq!(localize("12,5", &sv, &en), "12.5");
        assert_eq!(localize("12 500", &sv, &en), "12,500");
        assert_eq!(localize("2026", &sv, &en), "2026");
        assert_eq!(localize("08-123 456 78", &sv, &en), "08-123 456 78");
        assert_eq!(localize("2026-10-30", &sv, &en), "2026-10-30");
        assert_eq!(localize("4.2.1", &sv, &en), "4.2.1");
        assert_eq!(localize("1,234.50", &en, &sv), "1 234,50");
        assert_eq!(localize("12,500", &en, &sv), "12 500");
        assert_eq!(
            localize("12.5", &en, &sv),
            "12.5",
            "reads the same in Swedish"
        );
        assert_eq!(localize("4.2", &en, &sv), "4.2");
    }

    #[test]
    fn unmask_rejects_missing_repeated_and_unknown_placeholders() {
        let m = mask("Totalt 1 234,50 kr till 2026-10-30").unwrap();
        let pair = sv_en();
        assert_eq!(
            unmask("Total {1} SEK by {2}", &m, &pair).as_deref(),
            Some("Total 1,234.50 SEK by 2026-10-30")
        );
        assert_eq!(unmask("Total {1} SEK", &m, &pair), None);
        assert_eq!(unmask("Total {1} {1} by {2}", &m, &pair), None);
        assert_eq!(unmask("Total {1} by {3} {2}", &m, &pair), None);
        assert_eq!(unmask("Total {x} by {2}", &m, &pair), None);
    }

    fn guard(entries: &[(&str, &str)]) -> TokenGuard<MockTranslator> {
        let mut mock = MockTranslator::new();
        for (s, t) in entries {
            mock = mock.with_entry(s, t);
        }
        TokenGuard::new(mock)
    }

    #[tokio::test]
    async fn sv05_good_output_passes_untouched() {
        let g = guard(&[("Summa: 1 234,50 kr", "Total: SEK 1,234.50")]);
        let out = g
            .translate(&["Summa: 1 234,50 kr".into()], &sv_en())
            .await
            .unwrap();
        assert_eq!(out, ["Total: SEK 1,234.50"]);
        assert_eq!((g.masked_retries(), g.fallbacks()), (0, 0));
        assert_eq!(g.inner().calls(), 1);
    }

    #[tokio::test]
    async fn sv05_altered_numbers_are_retried_masked() {
        let g = guard(&[
            ("Att betala: 1 234,50 kr", "To pay: 1,245 SEK"),
            ("Att betala: {1} kr", "To pay: {1} SEK"),
            ("Gäller till kl. 23:59", "Valid until 2. 23:59"),
            ("Gäller till kl. {1}", "Valid until {1}"),
        ]);
        let blocks = [
            "Att betala: 1 234,50 kr".into(),
            "Gäller till kl. 23:59".into(),
        ];
        let out = g.translate(&blocks, &sv_en()).await.unwrap();
        assert_eq!(out, ["To pay: 1,234.50 SEK", "Valid until 23:59"]);
        assert_eq!((g.masked_retries(), g.fallbacks()), (2, 0));
        assert_eq!(g.inner().calls(), 2, "one batch per pass");
    }

    #[tokio::test]
    async fn sv05_altered_url_is_restored_verbatim() {
        let g = guard(&[
            (
                "Läs på https://exempel.se/villkor",
                "Read at https://exem.se/conditions",
            ),
            ("Läs på {1}", "Read at {1}"),
        ]);
        let out = g
            .translate(&["Läs på https://exempel.se/villkor".into()], &sv_en())
            .await
            .unwrap();
        assert_eq!(out, ["Read at https://exempel.se/villkor"]);
    }

    #[tokio::test]
    async fn sv05_falls_back_to_source_when_no_pass_keeps_tokens() {
        // The masked pass drops the placeholder: show the source, never wrong digits.
        let g = guard(&[
            ("Ordernummer 4471", "Order number 4417"),
            ("Ordernummer {1}", "Order number"),
        ]);
        let out = g
            .translate(&["Ordernummer 4471".into(), "Spara".into()], &sv_en())
            .await
            .unwrap();
        assert_eq!(out, ["Ordernummer 4471", "[en] Spara"]);
        assert_eq!(g.fallbacks(), 1);
    }

    #[tokio::test]
    async fn guard_delegates_backend_identity_and_errors() {
        let g = TokenGuard::new(MockTranslator::new().with_id("x").online());
        assert_eq!(g.id(), "x");
        assert!(!g.is_local());
        g.inner()
            .set_failure(Some(crate::TranslateError::ModelMissing(sv_en())));
        assert!(g.translate(&["a".into()], &sv_en()).await.is_err());
    }
}

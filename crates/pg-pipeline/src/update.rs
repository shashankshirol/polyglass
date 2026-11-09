//! What the lens UI receives (PLAN §3.3). The UI only draws these; no logic in JS.

use std::fmt;
use std::time::Duration;

use pg_core::LangTag;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LensUpdate {
    /// Scheduler generation that produced this update. The driver never emits an update for
    /// an older generation (CAP-03).
    pub generation: u64,
    pub status: LensStatus,
    /// In reading order of the source (MODE-02, REN-05).
    pub blocks: Vec<RenderedBlock>,
    pub backend: BackendInfo,
    pub notices: Vec<Notice>,
    pub timings: Timings,
}

impl LensUpdate {
    /// What the copy action puts on the clipboard (REN-05): each block's displayed text, in
    /// reading order, one block per line.
    pub fn copy_text(&self) -> String {
        let texts: Vec<&str> = self
            .blocks
            .iter()
            .map(RenderedBlock::display_text)
            .collect();
        texts.join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LensStatus {
    Ok,
    /// Region is blank/uniform, or OCR found nothing (CAP-06).
    NoTextFound,
    /// Message ID of a distinct, human-readable error (TR-06, CAP-05, OCR-03).
    Error {
        message_id: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        lang: Option<LangTag>,
    },
}

/// Non-fatal information shown alongside results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    /// Auto mode could not use the Swedish OCR language and fell back to English (SV-02).
    OcrLanguageMissing { lang: LangTag },
}

/// Privacy indicator data (PRIV-05).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackendInfo {
    pub id: String,
    pub name: String,
    pub local: bool,
}

/// Rectangle in lens-local logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LensRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, PartialEq, Serialize)]
pub struct RenderedBlock {
    pub rect: LensRect,
    /// Mean height of the source lines in lens-local logical px: where font fitting starts
    /// (REN-02).
    pub line_height: f32,
    pub source: String,
    pub source_lang: LangTag,
    /// `None` when translation failed (see the update's status).
    pub translation: Option<String>,
    pub target_lang: LangTag,
    /// Already in the target language; shown unchanged and not sent anywhere (TR-07).
    pub passthrough: bool,
    /// Below the confidence threshold with "dim" selected (OCR-05).
    pub dim: bool,
    /// `#rrggbb` sampled background and contrasting text colour (REN-01).
    pub bg: String,
    pub fg: String,
}

impl RenderedBlock {
    /// What to display: the translation, or the source when there is none.
    pub fn display_text(&self) -> &str {
        self.translation.as_deref().unwrap_or(&self.source)
    }
}

/// Redacts text so blocks can't leak recognized or translated text into logs (PRIV-07).
impl fmt::Debug for RenderedBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RenderedBlock")
            .field("rect", &self.rect)
            .field("source_chars", &self.source.chars().count())
            .field("source_lang", &self.source_lang)
            .field("translated", &self.translation.is_some())
            .field("target_lang", &self.target_lang)
            .field("passthrough", &self.passthrough)
            .field("dim", &self.dim)
            .finish_non_exhaustive()
    }
}

/// Per-stage durations of one cycle (`--trace-timings`, PLAN §6 budget).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Timings {
    #[serde(serialize_with = "ser_ms")]
    pub capture: Duration,
    #[serde(serialize_with = "ser_ms")]
    pub change: Duration,
    #[serde(serialize_with = "ser_ms")]
    pub ocr: Duration,
    #[serde(serialize_with = "ser_ms")]
    pub group: Duration,
    #[serde(serialize_with = "ser_ms")]
    pub translate: Duration,
    #[serde(serialize_with = "ser_ms")]
    pub total: Duration,
}

fn ser_ms<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(source: &str, translation: Option<&str>) -> RenderedBlock {
        RenderedBlock {
            rect: LensRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 14.0,
            },
            line_height: 14.0,
            source: source.into(),
            source_lang: LangTag::sv(),
            translation: translation.map(Into::into),
            target_lang: LangTag::en(),
            passthrough: false,
            dim: false,
            bg: "#ffffff".into(),
            fg: "#000000".into(),
        }
    }

    #[test]
    fn ren05_copy_text_is_the_displayed_blocks_in_order() {
        let u = LensUpdate {
            generation: 1,
            status: LensStatus::Ok,
            blocks: vec![
                block("Spara ändringar", Some("Save changes")),
                // Untranslated (failed) blocks copy their source, with å ä ö intact.
                block("Göteborg, Malmö och Västerås", None),
                block("Avbryt", Some("Avbryt – Ångra")),
            ],
            backend: BackendInfo {
                id: "mock".into(),
                name: "Mock".into(),
                local: true,
            },
            notices: vec![],
            timings: Timings::default(),
        };
        assert_eq!(
            u.copy_text(),
            "Save changes\nGöteborg, Malmö och Västerås\nAvbryt – Ångra"
        );
    }
}

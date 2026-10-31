//! OCR engines for Polyglass.
//!
//! This crate defines the engine trait, default-engine selection (OCR-01), language
//! resolution against runtime-queried languages (OCR-06), and a mock for tests. Platform
//! engines: [`WindowsOcr`] on Windows; Apple Vision and Tesseract follow per CHECKLIST
//! M1-OCR-3/4.

mod mock;
mod select;
#[cfg(windows)]
mod winrt;

pub use mock::MockOcr;
pub use select::{EngineKind, Platform, default_engine, resolve_language};
#[cfg(windows)]
pub use winrt::WindowsOcr;

use pg_core::{Frame, LangTag, Line, Result};

pub trait OcrEngine: Send + Sync {
    /// Stable identifier, e.g. `"windows-ocr"`, `"vision"`, `"tesseract"`.
    fn id(&self) -> &'static str;

    /// Recognize text in `frame`. `lang` is the language to recognize; `None` lets the engine
    /// pick (engines that need a language use their default).
    fn recognize(&self, frame: &Frame, lang: Option<&LangTag>) -> Result<Vec<Line>>;

    /// Languages this engine can recognize right now. Must be queried at runtime, never
    /// hard-coded, so OS updates that add or remove languages are picked up (OCR-06).
    fn supported_languages(&self) -> Vec<LangTag>;
}

impl<T: OcrEngine + ?Sized> OcrEngine for std::sync::Arc<T> {
    fn id(&self) -> &'static str {
        (**self).id()
    }

    fn recognize(&self, frame: &Frame, lang: Option<&LangTag>) -> Result<Vec<Line>> {
        (**self).recognize(frame, lang)
    }

    fn supported_languages(&self) -> Vec<LangTag> {
        (**self).supported_languages()
    }
}

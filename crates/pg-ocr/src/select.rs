use pg_core::{Error, LangTag, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    WindowsOcr,
    AppleVision,
    Tesseract,
}

/// Default OCR engine for a platform and source language (OCR-01 table).
///
/// `vision_swedish_ok` is true only if Vision reports Swedish among its runtime languages
/// (OCR-06) *and* Vision has passed the SV-03 accuracy gate on the Swedish fixtures; until then
/// Swedish on macOS uses the bundled Tesseract.
pub fn default_engine(platform: Platform, source: &LangTag, vision_swedish_ok: bool) -> EngineKind {
    match platform {
        Platform::Windows => EngineKind::WindowsOcr,
        Platform::MacOs if source.is_swedish() && !vision_swedish_ok => EngineKind::Tesseract,
        Platform::MacOs => EngineKind::AppleVision,
        Platform::Linux => EngineKind::Tesseract,
    }
}

/// Pick the engine language tag to use for `wanted` from the engine's runtime languages.
///
/// Prefers an exact match, then any tag with the same primary language (`sv` → `sv-SE`).
/// Returns [`Error::OcrLanguageMissing`] so the UI can show an actionable message (OCR-03, SV-02).
pub fn resolve_language(supported: &[LangTag], wanted: &LangTag) -> Result<LangTag> {
    supported
        .iter()
        .find(|t| *t == wanted)
        .or_else(|| supported.iter().find(|t| t.same_language(wanted)))
        .cloned()
        .ok_or_else(|| Error::OcrLanguageMissing(wanted.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> LangTag {
        LangTag::parse(s).unwrap()
    }

    #[test]
    fn ocr01_default_table() {
        let sv = LangTag::sv();
        let en = LangTag::en();
        assert_eq!(
            default_engine(Platform::Windows, &en, false),
            EngineKind::WindowsOcr
        );
        assert_eq!(
            default_engine(Platform::Windows, &sv, false),
            EngineKind::WindowsOcr
        );
        assert_eq!(
            default_engine(Platform::MacOs, &en, false),
            EngineKind::AppleVision
        );
        assert_eq!(
            default_engine(Platform::MacOs, &sv, false),
            EngineKind::Tesseract
        );
        assert_eq!(
            default_engine(Platform::Linux, &en, false),
            EngineKind::Tesseract
        );
        assert_eq!(
            default_engine(Platform::Linux, &sv, false),
            EngineKind::Tesseract
        );
    }

    #[test]
    fn vision_becomes_swedish_default_only_when_qualified() {
        assert_eq!(
            default_engine(Platform::MacOs, &LangTag::sv(), true),
            EngineKind::AppleVision
        );
    }

    #[test]
    fn resolve_prefers_exact_then_primary() {
        let supported = [tag("en-GB"), tag("en-US"), tag("sv-SE")];
        assert_eq!(
            resolve_language(&supported, &tag("en-US")).unwrap(),
            tag("en-US")
        );
        assert_eq!(
            resolve_language(&supported, &tag("en")).unwrap(),
            tag("en-GB")
        );
        assert_eq!(
            resolve_language(&supported, &tag("sv")).unwrap(),
            tag("sv-SE")
        );
    }

    #[test]
    fn resolve_reports_missing_swedish() {
        // The dev machine's actual Windows OCR languages (no sv-SE): SV-02 path.
        let supported = [tag("en-GB"), tag("en-US")];
        let err = resolve_language(&supported, &LangTag::sv()).unwrap_err();
        assert!(matches!(err, Error::OcrLanguageMissing(t) if t.is_swedish()));
    }
}

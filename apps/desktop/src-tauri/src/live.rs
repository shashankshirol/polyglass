//! The default pipeline: real screen capture and OCR under the lens.
//!
//! Translation is still `MockTranslator` until the on-device backend lands (M1-TR-1): Swedish
//! blocks show as `[en] <recognized text>` and English passes through. The toolbar shows a
//! "MOCK MT" badge meanwhile.

#[cfg(not(windows))]
use pg_core::Error;
use pg_core::Result;
use pg_pipeline::Pipeline;

/// Fluent message ID shown when the lens can't keep itself out of captures.
pub const SELF_EXCLUSION_ERROR: &str = "cap-error-self-exclusion";

/// Build the live pipeline. Only call it once the lens is excluded from capture
/// (`lens::exclude_from_capture`), or the lens would read its own translations (CAP-02).
#[cfg(windows)]
pub fn pipeline() -> Result<Pipeline> {
    use std::sync::Arc;

    use pg_core::{LangTag, SourceLang};
    use pg_pipeline::PipelineConfig;
    use pg_translate::MockTranslator;

    let capturer = pg_capture::WgcCapturer::new()?.with_self_excluded(true);
    let ocr = pg_ocr::WindowsOcr::new()?;
    Ok(Pipeline::new(
        Box::new(capturer),
        Arc::new(ocr),
        Arc::new(MockTranslator::new()),
        PipelineConfig::new(SourceLang::AutoSvEn, LangTag::en()),
    ))
}

/// Capture on macOS and Linux is M1-CAP-3 / M2-1.
#[cfg(not(windows))]
pub fn pipeline() -> Result<Pipeline> {
    Err(Error::Capture(
        "no capture backend on this platform yet".into(),
    ))
}

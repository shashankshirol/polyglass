//! The default pipeline: real screen capture, OCR and on-device translation under the lens.
//!
//! Translation is Bergamot (ADR-0004) with both Swedish directions loaded, behind the SV-05
//! token guard. Without the models the backend reports `tr-error-model-missing` for Swedish
//! blocks (English still passes through) and the toolbar offers the download (`models.rs`).

use std::sync::Arc;

use pg_bergamot::BergamotTranslator;
#[cfg(not(windows))]
use pg_core::Error;
use pg_core::Result;
use pg_models::{Manifest, ModelStore};
use pg_pipeline::Pipeline;
use pg_translate::{TokenGuard, Translator};

/// Fluent message ID shown when the lens can't keep itself out of captures.
pub const SELF_EXCLUSION_ERROR: &str = "cap-error-self-exclusion";

/// The on-device translator with every installed model loaded. Never fails: a model that
/// won't load is logged and treated as missing, so the lens says so instead of going blank.
pub fn translator(store: &ModelStore) -> Arc<dyn Translator> {
    let bergamot = Manifest::bundled()
        .map_err(|e| e.to_string())
        .and_then(|m| BergamotTranslator::load_installed(&m, store).map_err(|e| e.to_string()))
        .unwrap_or_else(|error| {
            tracing::error!(%error, "on-device models failed to load");
            BergamotTranslator::empty()
        });
    let pairs: Vec<String> = bergamot.pairs().iter().map(ToString::to_string).collect();
    if pairs.is_empty() {
        tracing::warn!("no on-device translation models installed");
    } else {
        tracing::info!(?pairs, "on-device translation models loaded");
    }
    Arc::new(TokenGuard::new(bergamot))
}

/// Build the live pipeline. Only call it once the lens is excluded from capture
/// (`lens::exclude_from_capture`), or the lens would read its own translations (CAP-02).
#[cfg(windows)]
pub fn pipeline(store: &ModelStore) -> Result<Pipeline> {
    use pg_core::{LangTag, SourceLang};
    use pg_pipeline::PipelineConfig;

    let capturer = pg_capture::WgcCapturer::new()?.with_self_excluded(true);
    let ocr = pg_ocr::WindowsOcr::new()?;
    Ok(Pipeline::new(
        Box::new(capturer),
        Arc::new(ocr),
        translator(store),
        PipelineConfig::new(SourceLang::AutoSvEn, LangTag::en()),
    ))
}

/// Capture on macOS and Linux is M1-CAP-3 / M2-1.
#[cfg(not(windows))]
pub fn pipeline(_store: &ModelStore) -> Result<Pipeline> {
    Err(Error::Capture(
        "no capture backend on this platform yet".into(),
    ))
}

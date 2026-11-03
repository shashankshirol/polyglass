use crate::LangTag;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors shared across the pipeline.
///
/// Messages must never contain recognized or translated text (PRIV-07), so variants carry only
/// technical context.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("screen capture failed: {0}")]
    Capture(String),

    /// The OS denied screen capture to the app (CAP-05).
    #[error("screen capture permission denied")]
    CapturePermissionDenied,

    #[error("OCR failed: {0}")]
    Ocr(String),

    /// The OCR engine has no data for this language (OCR-03, SV-02).
    #[error("OCR language not installed: {0}")]
    OcrLanguageMissing(LangTag),

    #[error("model error: {0}")]
    Model(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

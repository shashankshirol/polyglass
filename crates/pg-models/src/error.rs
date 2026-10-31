pub type Result<T, E = ModelError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("unknown model: {0}")]
    UnknownModel(String),

    /// The manifest lists the model without download URLs; install it by import (MDL-04).
    #[error("{0} has no download URL yet; import it from local files")]
    NotHosted(String),

    #[error("refusing non-HTTPS download URL")]
    InsecureUrl,

    #[error("network error: {0}")]
    Network(String),

    #[error("server returned HTTP {0}")]
    Http(u16),

    /// The file didn't match the manifest; it has been deleted (MDL-02).
    #[error("checksum mismatch for {file}")]
    HashMismatch { file: String },

    #[error("download cancelled")]
    Cancelled,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

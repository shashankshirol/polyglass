pub type Result<T, E = ModelError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("unknown model: {0}")]
    UnknownModel(String),

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

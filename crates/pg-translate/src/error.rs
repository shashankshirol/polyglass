use pg_core::LangPair;

pub type Result<T, E = TranslateError> = std::result::Result<T, E>;

/// Translation failures. Each kind maps to a distinct, human-readable message in the lens
/// (TR-06). Messages never include the text being translated (PRIV-07).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    /// Connection refused, DNS failure, TLS error, ...
    #[error("network error: {0}")]
    Network(String),

    /// No response within the backend timeout (5 s for online backends).
    #[error("request timed out")]
    Timeout,

    /// HTTP 401/403: missing, wrong or revoked API key.
    #[error("authentication failed")]
    Auth,

    /// HTTP 429 or a provider-specific quota error.
    #[error("quota exceeded")]
    Quota,

    /// The backend can't translate this pair at all.
    #[error("unsupported language pair {0}")]
    UnsupportedPair(LangPair),

    /// The on-device model for this pair isn't downloaded.
    #[error("model not installed for {0}")]
    ModelMissing(LangPair),

    /// Anything else the backend reported.
    #[error("translation backend error: {0}")]
    Backend(String),

    #[error("translation cancelled")]
    Cancelled,
}

impl TranslateError {
    /// Fluent message ID for the lens (I18N-01). Distinct per kind (TR-06).
    pub fn message_id(&self) -> &'static str {
        match self {
            TranslateError::Network(_) => "tr-error-network",
            TranslateError::Timeout => "tr-error-timeout",
            TranslateError::Auth => "tr-error-auth",
            TranslateError::Quota => "tr-error-quota",
            TranslateError::UnsupportedPair(_) => "tr-error-unsupported-pair",
            TranslateError::ModelMissing(_) => "tr-error-model-missing",
            TranslateError::Backend(_) => "tr-error-backend",
            TranslateError::Cancelled => "tr-error-cancelled",
        }
    }

    /// Map an HTTP status from an online backend to an error kind.
    pub fn from_http_status(status: u16) -> Option<Self> {
        match status {
            200..=299 => None,
            401 | 403 => Some(TranslateError::Auth),
            429 => Some(TranslateError::Quota),
            // DeepL reports exhausted character quota as 456.
            456 => Some(TranslateError::Quota),
            _ => Some(TranslateError::Backend(format!("HTTP {status}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tr06_message_ids_are_distinct() {
        let all = [
            TranslateError::Network("x".into()),
            TranslateError::Timeout,
            TranslateError::Auth,
            TranslateError::Quota,
            TranslateError::UnsupportedPair(LangPair::sv_en()),
            TranslateError::ModelMissing(LangPair::sv_en()),
            TranslateError::Backend("x".into()),
            TranslateError::Cancelled,
        ];
        let ids: HashSet<_> = all.iter().map(TranslateError::message_id).collect();
        assert_eq!(ids.len(), all.len());
    }

    #[test]
    fn http_status_mapping() {
        assert_eq!(TranslateError::from_http_status(200), None);
        assert_eq!(
            TranslateError::from_http_status(401),
            Some(TranslateError::Auth)
        );
        assert_eq!(
            TranslateError::from_http_status(403),
            Some(TranslateError::Auth)
        );
        assert_eq!(
            TranslateError::from_http_status(429),
            Some(TranslateError::Quota)
        );
        assert_eq!(
            TranslateError::from_http_status(456),
            Some(TranslateError::Quota)
        );
        assert!(matches!(
            TranslateError::from_http_status(500),
            Some(TranslateError::Backend(_))
        ));
    }
}

//! Translation for Polyglass.
//!
//! Defines the [`Translator`] trait shared by all backends (TR-01), the error kinds the UI
//! distinguishes (TR-06), Swedish/English language identification (SV-04, TR-07), preserved-token
//! checks (SV-05) and English pivoting (TR-03). Backends are added per CHECKLIST M1-TR-1, M3-*.

mod error;
pub mod langid;
mod mock;
mod pivot;
pub mod tokens;

pub use error::{Result, TranslateError};
pub use mock::MockTranslator;
pub use pivot::{PivotTranslator, Route, plan_route};

use async_trait::async_trait;
use pg_core::LangPair;

#[async_trait]
pub trait Translator: Send + Sync {
    /// Stable identifier, used in cache keys and settings (`"bergamot"`, `"deepl"`, ...).
    fn id(&self) -> &str;

    /// Name shown next to the privacy indicator for online backends (PRIV-05).
    fn display_name(&self) -> &str {
        self.id()
    }

    /// `true` = never touches the network. Drives the privacy indicator (PRIV-05).
    fn is_local(&self) -> bool;

    /// Whether this backend can translate `pair` directly right now (models installed, etc.).
    fn supports(&self, pair: &LangPair) -> bool;

    /// Translate each block. The output has the same length and order as `blocks`.
    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>>;
}

#[async_trait]
impl<T: Translator + ?Sized> Translator for std::sync::Arc<T> {
    fn id(&self) -> &str {
        (**self).id()
    }

    fn display_name(&self) -> &str {
        (**self).display_name()
    }

    fn is_local(&self) -> bool {
        (**self).is_local()
    }

    fn supports(&self, pair: &LangPair) -> bool {
        (**self).supports(pair)
    }

    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>> {
        (**self).translate(blocks, pair).await
    }
}

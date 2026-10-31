use std::sync::Arc;

use async_trait::async_trait;
use pg_core::{LangPair, LangTag};

use crate::{Result, TranslateError, Translator};

/// How a pair is translated with the available on-device models (TR-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Direct(LangPair),
    /// source → English → target; the toolbar shows "via EN".
    ViaEnglish {
        first: LangPair,
        second: LangPair,
    },
}

impl Route {
    pub fn is_pivot(&self) -> bool {
        matches!(self, Route::ViaEnglish { .. })
    }
}

/// Choose a route for `pair` given the directly supported pairs.
pub fn plan_route(supports: impl Fn(&LangPair) -> bool, pair: &LangPair) -> Option<Route> {
    if supports(pair) {
        return Some(Route::Direct(pair.clone()));
    }
    let en = LangTag::en();
    if pair.source.same_language(&en) || pair.target.same_language(&en) {
        return None;
    }
    let first = LangPair::new(pair.source.clone(), en.clone());
    let second = LangPair::new(en, pair.target.clone());
    (supports(&first) && supports(&second)).then_some(Route::ViaEnglish { first, second })
}

/// Wraps a backend so pairs without a direct model pivot through English (TR-03).
#[derive(Debug, Clone)]
pub struct PivotTranslator<T> {
    inner: Arc<T>,
}

impl<T: Translator> PivotTranslator<T> {
    pub fn new(inner: Arc<T>) -> Self {
        Self { inner }
    }

    pub fn route(&self, pair: &LangPair) -> Option<Route> {
        plan_route(|p| self.inner.supports(p), pair)
    }
}

#[async_trait]
impl<T: Translator> Translator for PivotTranslator<T> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn display_name(&self) -> &str {
        self.inner.display_name()
    }

    fn is_local(&self) -> bool {
        self.inner.is_local()
    }

    fn supports(&self, pair: &LangPair) -> bool {
        self.route(pair).is_some()
    }

    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>> {
        match self.route(pair) {
            Some(Route::Direct(p)) => self.inner.translate(blocks, &p).await,
            Some(Route::ViaEnglish { first, second }) => {
                let english = self.inner.translate(blocks, &first).await?;
                self.inner.translate(&english, &second).await
            }
            None => Err(TranslateError::UnsupportedPair(pair.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MockTranslator;

    fn tag(s: &str) -> LangTag {
        LangTag::parse(s).unwrap()
    }

    fn models() -> Vec<LangPair> {
        vec![
            LangPair::sv_en(),
            LangPair::en_sv(),
            LangPair::new(LangTag::en(), tag("de")),
        ]
    }

    #[test]
    fn routes() {
        let m = models();
        let s = |p: &LangPair| m.contains(p);
        assert_eq!(
            plan_route(s, &LangPair::sv_en()),
            Some(Route::Direct(LangPair::sv_en()))
        );
        let sv_de = LangPair::new(LangTag::sv(), tag("de"));
        let r = plan_route(s, &sv_de).unwrap();
        assert!(r.is_pivot());
        // de→sv needs de→en, which is missing.
        assert_eq!(plan_route(s, &sv_de.swapped()), None);
    }

    #[tokio::test]
    async fn pivot_translates_twice() {
        let mock = Arc::new(
            MockTranslator::new()
                .with_pairs(models())
                .with_entry("Hej", "Hello")
                .with_entry("Hello", "Hallo"),
        );
        let t = PivotTranslator::new(mock.clone());
        let out = t
            .translate(&["Hej".into()], &LangPair::new(LangTag::sv(), tag("de")))
            .await
            .unwrap();
        assert_eq!(out, ["Hallo"]);
        assert_eq!(mock.calls(), 2);
    }
}

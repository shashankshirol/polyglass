//! In-memory translation cache (TR-04). Never written to disk: no `Serialize` impls here.

use std::num::NonZeroUsize;

use lru::LruCache;
use pg_core::LangPair;
use unicode_normalization::UnicodeNormalization;

pub const DEFAULT_CAPACITY: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    backend: String,
    pair: LangPair,
    text: String,
}

impl CacheKey {
    pub fn new(backend: &str, pair: &LangPair, text: &str) -> Self {
        Self {
            backend: backend.to_owned(),
            pair: pair.clone(),
            text: normalize_text(text),
        }
    }
}

/// NFC (so `a` + combining ring equals `å`), collapse whitespace runs, trim.
pub fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.nfc().collect::<String>().split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[derive(Debug)]
pub struct TranslationCache {
    inner: LruCache<CacheKey, String>,
    hits: u64,
    misses: u64,
}

impl TranslationCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);
        Self {
            inner: LruCache::new(cap),
            hits: 0,
            misses: 0,
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<String> {
        let v = self.inner.get(key).cloned();
        if v.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        v
    }

    pub fn put(&mut self, key: CacheKey, translation: String) {
        self.inner.put(key, translation);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// (hits, misses) since creation.
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
}

impl Default for TranslationCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_unifies_composed_and_spacing() {
        // "på" with a decomposed å (a + U+030A) and irregular spacing.
        let decomposed = "  Klicka  pa\u{30a}\n knappen ";
        assert_eq!(normalize_text(decomposed), "Klicka på knappen");
        let pair = LangPair::sv_en();
        assert_eq!(
            CacheKey::new("x", &pair, decomposed),
            CacheKey::new("x", &pair, "Klicka på knappen")
        );
    }

    #[test]
    fn keys_differ_by_backend_and_pair() {
        let a = CacheKey::new("bergamot", &LangPair::sv_en(), "Hej");
        assert_ne!(a, CacheKey::new("deepl", &LangPair::sv_en(), "Hej"));
        assert_ne!(a, CacheKey::new("bergamot", &LangPair::en_sv(), "Hej"));
    }

    #[test]
    fn lru_eviction_and_stats() {
        let mut c = TranslationCache::new(2);
        let p = LangPair::sv_en();
        c.put(CacheKey::new("m", &p, "a"), "A".into());
        c.put(CacheKey::new("m", &p, "b"), "B".into());
        assert_eq!(c.get(&CacheKey::new("m", &p, "a")).as_deref(), Some("A"));
        c.put(CacheKey::new("m", &p, "c"), "C".into()); // evicts "b"
        assert_eq!(c.get(&CacheKey::new("m", &p, "b")), None);
        assert_eq!(c.len(), 2);
        assert_eq!(c.stats(), (1, 1));
    }

    #[test]
    fn default_capacity_is_2000() {
        let mut c = TranslationCache::default();
        let p = LangPair::sv_en();
        for i in 0..2_100 {
            c.put(CacheKey::new("m", &p, &i.to_string()), String::new());
        }
        assert_eq!(c.len(), DEFAULT_CAPACITY);
    }
}

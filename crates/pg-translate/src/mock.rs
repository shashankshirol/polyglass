use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use pg_core::LangPair;

use crate::{Result, TranslateError, Translator};

/// Test backend. Looks texts up in a dictionary and otherwise returns `"[target] text"`.
#[derive(Debug)]
pub struct MockTranslator {
    id: &'static str,
    local: bool,
    pairs: Option<HashSet<LangPair>>,
    dict: HashMap<String, String>,
    fail_with: Mutex<Option<TranslateError>>,
    calls: AtomicUsize,
    texts: AtomicUsize,
}

impl MockTranslator {
    pub fn new() -> Self {
        Self {
            id: "mock",
            local: true,
            pairs: None,
            dict: HashMap::new(),
            fail_with: Mutex::new(None),
            calls: AtomicUsize::new(0),
            texts: AtomicUsize::new(0),
        }
    }

    pub fn with_id(mut self, id: &'static str) -> Self {
        self.id = id;
        self
    }

    pub fn online(mut self) -> Self {
        self.local = false;
        self
    }

    /// Restrict supported pairs (default: everything).
    pub fn with_pairs(mut self, pairs: impl IntoIterator<Item = LangPair>) -> Self {
        self.pairs = Some(pairs.into_iter().collect());
        self
    }

    pub fn with_entry(mut self, source: &str, translation: &str) -> Self {
        self.dict.insert(source.into(), translation.into());
        self
    }

    /// Make the next and all following calls fail (`None` to recover).
    pub fn set_failure(&self, err: Option<TranslateError>) {
        *self.fail_with.lock().expect("mock lock") = err;
    }

    /// Number of `translate` calls (a batch counts once).
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Total number of texts sent across all calls.
    pub fn texts_sent(&self) -> usize {
        self.texts.load(Ordering::SeqCst)
    }
}

impl Default for MockTranslator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Translator for MockTranslator {
    fn id(&self) -> &str {
        self.id
    }

    fn is_local(&self) -> bool {
        self.local
    }

    fn supports(&self, pair: &LangPair) -> bool {
        self.pairs.as_ref().is_none_or(|p| p.contains(pair))
    }

    async fn translate(&self, blocks: &[String], pair: &LangPair) -> Result<Vec<String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.texts.fetch_add(blocks.len(), Ordering::SeqCst);
        if let Some(err) = self.fail_with.lock().expect("mock lock").clone() {
            return Err(err);
        }
        if !self.supports(pair) {
            return Err(TranslateError::UnsupportedPair(pair.clone()));
        }
        Ok(blocks
            .iter()
            .map(|b| {
                self.dict
                    .get(b)
                    .cloned()
                    .unwrap_or_else(|| format!("[{}] {b}", pair.target))
            })
            .collect())
    }
}

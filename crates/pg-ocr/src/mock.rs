use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pg_core::{Frame, LangTag, Line, Result};

use crate::OcrEngine;

type RecognizeFn = dyn Fn(&Frame) -> Vec<Line> + Send + Sync;

/// Test engine: returns lines computed from the frame by a closure, optionally after a delay.
pub struct MockOcr {
    f: Box<RecognizeFn>,
    delay: Duration,
    languages: Vec<LangTag>,
    calls: AtomicUsize,
}

impl MockOcr {
    /// Always returns `lines`.
    pub fn fixed(lines: Vec<Line>) -> Self {
        Self::with_fn(move |_| lines.clone())
    }

    pub fn with_fn(f: impl Fn(&Frame) -> Vec<Line> + Send + Sync + 'static) -> Self {
        Self {
            f: Box::new(f),
            delay: Duration::ZERO,
            languages: vec![LangTag::sv(), LangTag::en()],
            calls: AtomicUsize::new(0),
        }
    }

    /// Simulate engine latency (blocking sleep, as real engines block).
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn with_languages(mut self, languages: Vec<LangTag>) -> Self {
        self.languages = languages;
        self
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl fmt::Debug for MockOcr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockOcr")
            .field("delay", &self.delay)
            .field("calls", &self.calls())
            .finish_non_exhaustive()
    }
}

impl OcrEngine for MockOcr {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn recognize(&self, frame: &Frame, lang: Option<&LangTag>) -> Result<Vec<Line>> {
        if let Some(l) = lang {
            crate::resolve_language(&self.languages, l)?;
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        Ok((self.f)(frame))
    }

    fn supported_languages(&self) -> Vec<LangTag> {
        self.languages.clone()
    }
}

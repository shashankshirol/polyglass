//! One pipeline cycle: capture → change detect → OCR → group → cache → translate.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use pg_capture::ScreenCapturer;
use pg_core::{Error, Frame, LangPair, LangTag, SourceLang};
use pg_ocr::OcrEngine;
use pg_translate::{Translator, langid};

use crate::cache::{CacheKey, DEFAULT_CAPACITY, TranslationCache};
use crate::change::{ChangeConfig, FrameSignature};
use crate::color::{foreground_for, sample_background};
use crate::grouping::{GroupingConfig, group_lines};
use crate::scheduler::Tick;
use crate::update::{
    BackendInfo, LensRect, LensStatus, LensUpdate, Notice, RenderedBlock, Timings,
};

/// What to do with lines below the confidence threshold (OCR-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowConfidence {
    Hide,
    Dim,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    pub source: SourceLang,
    pub target: LangTag,
    /// OCR-05 threshold, default 0.5.
    pub min_confidence: f32,
    pub low_confidence: LowConfidence,
    pub change: ChangeConfig,
    pub grouping: GroupingConfig,
    pub cache_capacity: usize,
}

impl PipelineConfig {
    pub fn new(source: SourceLang, target: LangTag) -> Self {
        Self {
            source,
            target,
            min_confidence: 0.5,
            low_confidence: LowConfidence::Hide,
            change: ChangeConfig::default(),
            grouping: GroupingConfig::default(),
            cache_capacity: DEFAULT_CAPACITY,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CycleOutcome {
    /// Frame unchanged since the last cycle: OCR and translation skipped (CAP-03).
    Unchanged,
    Update(Box<LensUpdate>),
}

/// Result of the blocking half of a cycle (capture + OCR + grouping).
struct Recognized {
    blocks: Vec<(pg_core::Block, String, String)>, // block, bg hex, fg hex
    scale: f32,
    notices: Vec<Notice>,
}

enum BlockingOutcome {
    Unchanged,
    Blank,
    Failed(LensStatus),
    Recognized(Recognized),
}

pub struct Pipeline {
    capturer: Arc<Mutex<Box<dyn ScreenCapturer>>>,
    ocr: Arc<dyn OcrEngine>,
    translator: RwLock<Arc<dyn Translator>>,
    config: RwLock<PipelineConfig>,
    cache: Mutex<TranslationCache>,
    last_signature: Arc<Mutex<Option<FrameSignature>>>,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("ocr", &self.ocr.id())
            .field("translator", &self.translator().id().to_owned())
            .finish_non_exhaustive()
    }
}

impl Pipeline {
    pub fn new(
        capturer: Box<dyn ScreenCapturer>,
        ocr: Arc<dyn OcrEngine>,
        translator: Arc<dyn Translator>,
        config: PipelineConfig,
    ) -> Self {
        Self {
            capturer: Arc::new(Mutex::new(capturer)),
            ocr,
            translator: RwLock::new(translator),
            cache: Mutex::new(TranslationCache::new(config.cache_capacity)),
            config: RwLock::new(config),
            last_signature: Arc::new(Mutex::new(None)),
        }
    }

    pub fn config(&self) -> PipelineConfig {
        self.config.read().expect("config lock").clone()
    }

    /// Replace the configuration. Call the driver's `invalidate` afterwards.
    pub fn set_config(&self, config: PipelineConfig) {
        *self.config.write().expect("config lock") = config;
    }

    pub fn translator(&self) -> Arc<dyn Translator> {
        self.translator.read().expect("translator lock").clone()
    }

    /// Switch backend. The cache is keyed by backend, so entries are not mixed.
    pub fn set_translator(&self, translator: Arc<dyn Translator>) {
        *self.translator.write().expect("translator lock") = translator;
    }

    /// Drop all cached translations (pg-bench uncached runs; language model changes).
    /// Let the capturer free OS capture resources while the lens is hidden (LENS-06). Waits
    /// for a capture in progress to finish.
    pub fn release_capture(&self) {
        self.capturer.lock().expect("capturer lock").release();
    }

    pub fn clear_cache(&self) {
        self.cache.lock().expect("cache lock").clear();
    }

    /// (hits, misses) of the translation cache.
    pub fn cache_stats(&self) -> (u64, u64) {
        self.cache.lock().expect("cache lock").stats()
    }

    /// Run one cycle for `tick`. Capture and OCR run on the blocking pool.
    pub async fn run_cycle(&self, tick: Tick) -> CycleOutcome {
        let started = Instant::now();
        let config = self.config();
        let translator = self.translator();
        let backend = BackendInfo {
            id: translator.id().to_owned(),
            name: translator.display_name().to_owned(),
            local: translator.is_local(),
        };
        let mut timings = Timings::default();

        let capturer = self.capturer.clone();
        let ocr = self.ocr.clone();
        let last_sig = self.last_signature.clone();
        let cfg = config.clone();
        let blocking = tokio::task::spawn_blocking(move || {
            let mut t = Timings::default();
            let out = recognize(&capturer, ocr.as_ref(), &last_sig, &cfg, tick, &mut t);
            (out, t)
        })
        .await;
        let (outcome, t) = match blocking {
            Ok(v) => v,
            Err(_) => (
                BlockingOutcome::Failed(LensStatus::Error {
                    message_id: "ocr-error",
                    lang: None,
                }),
                Timings::default(),
            ),
        };
        timings.capture = t.capture;
        timings.change = t.change;
        timings.ocr = t.ocr;
        timings.group = t.group;

        let update = |status, blocks, notices, mut timings: Timings| {
            timings.total = started.elapsed();
            log_timings(&timings);
            CycleOutcome::Update(Box::new(LensUpdate {
                generation: tick.generation,
                status,
                blocks,
                backend: backend.clone(),
                notices,
                timings,
            }))
        };

        let rec = match outcome {
            BlockingOutcome::Unchanged => return CycleOutcome::Unchanged,
            BlockingOutcome::Blank => {
                return update(LensStatus::NoTextFound, vec![], vec![], timings);
            }
            BlockingOutcome::Failed(status) => return update(status, vec![], vec![], timings),
            BlockingOutcome::Recognized(r) => r,
        };
        if rec.blocks.is_empty() {
            return update(LensStatus::NoTextFound, vec![], rec.notices, timings);
        }

        let t_translate = Instant::now();
        let (blocks, status) = self
            .translate_blocks(&rec, &config, translator.as_ref())
            .await;
        timings.translate = t_translate.elapsed();
        update(status, blocks, rec.notices, timings)
    }

    async fn translate_blocks(
        &self,
        rec: &Recognized,
        config: &PipelineConfig,
        translator: &dyn Translator,
    ) -> (Vec<RenderedBlock>, LensStatus) {
        let target = &config.target;
        let mut rendered: Vec<RenderedBlock> = Vec::with_capacity(rec.blocks.len());
        // Pair → indices of blocks that still need a translation.
        let mut pending: HashMap<LangPair, Vec<usize>> = HashMap::new();
        {
            let mut cache = self.cache.lock().expect("cache lock");
            for (block, bg, fg) in &rec.blocks {
                let text = block.text();
                let source_lang = match &config.source {
                    SourceLang::Fixed(l) => l.clone(),
                    SourceLang::AutoSvEn => langid::decide_sv_en(&text, target),
                };
                let passthrough = !langid::needs_translation(&source_lang, target);
                let pair = LangPair::new(source_lang.clone(), target.clone());
                let cached = if passthrough {
                    Some(text.clone())
                } else {
                    cache.get(&CacheKey::new(translator.id(), &pair, &text))
                };
                if cached.is_none() {
                    pending.entry(pair).or_default().push(rendered.len());
                }
                let b = block.bbox.scaled(1.0 / rec.scale);
                rendered.push(RenderedBlock {
                    rect: LensRect {
                        x: b.x,
                        y: b.y,
                        w: b.w,
                        h: b.h,
                    },
                    dim: block.confidence() < config.min_confidence,
                    source: text,
                    source_lang,
                    translation: cached,
                    target_lang: target.clone(),
                    passthrough,
                    bg: bg.clone(),
                    fg: fg.clone(),
                });
            }
        }

        let mut status = LensStatus::Ok;
        // One request per language pair per frame (TR-05); duplicates sent once.
        for (pair, indices) in pending {
            let mut unique: Vec<String> = Vec::new();
            for &i in &indices {
                if !unique.contains(&rendered[i].source) {
                    unique.push(rendered[i].source.clone());
                }
            }
            match translator.translate(&unique, &pair).await {
                Ok(out) if out.len() == unique.len() => {
                    let mut cache = self.cache.lock().expect("cache lock");
                    for (src, tr) in unique.iter().zip(&out) {
                        cache.put(CacheKey::new(translator.id(), &pair, src), tr.clone());
                    }
                    for &i in &indices {
                        let pos = unique
                            .iter()
                            .position(|u| *u == rendered[i].source)
                            .expect("source is in unique");
                        rendered[i].translation = Some(out[pos].clone());
                    }
                }
                Ok(_) => {
                    status = LensStatus::Error {
                        message_id: "tr-error-backend",
                        lang: None,
                    };
                }
                Err(e) => {
                    status = LensStatus::Error {
                        message_id: e.message_id(),
                        lang: None,
                    };
                }
            }
        }
        (rendered, status)
    }
}

/// The blocking half: capture, change detection, OCR, grouping, colour sampling.
fn recognize(
    capturer: &Mutex<Box<dyn ScreenCapturer>>,
    ocr: &dyn OcrEngine,
    last_sig: &Mutex<Option<FrameSignature>>,
    cfg: &PipelineConfig,
    tick: Tick,
    t: &mut Timings,
) -> BlockingOutcome {
    let t0 = Instant::now();
    let frame: Frame = match capturer.lock().expect("capturer lock").capture(tick.region) {
        Ok(f) => f,
        Err(Error::CapturePermissionDenied) => {
            return BlockingOutcome::Failed(LensStatus::Error {
                message_id: "cap-error-permission",
                lang: None,
            });
        }
        Err(_) => {
            return BlockingOutcome::Failed(LensStatus::Error {
                message_id: "cap-error",
                lang: None,
            });
        }
    };
    t.capture = t0.elapsed();

    let t1 = Instant::now();
    let sig = FrameSignature::of(&frame);
    let changed = {
        let mut last = last_sig.lock().expect("signature lock");
        let changed = last
            .as_ref()
            .is_none_or(|prev| sig.differs(prev, &cfg.change));
        *last = Some(sig.clone());
        changed
    };
    t.change = t1.elapsed();
    if !changed && !tick.force {
        return BlockingOutcome::Unchanged;
    }
    if sig.is_blank(&cfg.change) {
        return BlockingOutcome::Blank;
    }

    let t2 = Instant::now();
    let mut notices = Vec::new();
    let ocr_lang = match &cfg.source {
        SourceLang::Fixed(l) => l.clone(),
        // The Swedish recognizer also reads English text; prefer it in Auto mode.
        SourceLang::AutoSvEn => LangTag::sv(),
    };
    let lines = match ocr.recognize(&frame, Some(&ocr_lang)) {
        Ok(lines) => lines,
        Err(Error::OcrLanguageMissing(lang)) if cfg.source == SourceLang::AutoSvEn => {
            // SV-02: keep English source working while Swedish OCR is missing.
            notices.push(Notice::OcrLanguageMissing { lang });
            match ocr.recognize(&frame, Some(&LangTag::en())) {
                Ok(lines) => lines,
                Err(e) => return BlockingOutcome::Failed(ocr_status(e)),
            }
        }
        Err(e) => return BlockingOutcome::Failed(ocr_status(e)),
    };
    t.ocr = t2.elapsed();

    let t3 = Instant::now();
    let lines = match cfg.low_confidence {
        LowConfidence::Hide => lines
            .into_iter()
            .filter(|l| l.confidence >= cfg.min_confidence)
            .collect(),
        LowConfidence::Dim => lines,
    };
    let blocks = group_lines(lines, &cfg.grouping)
        .into_iter()
        .map(|b| {
            let bg = sample_background(&frame, b.bbox, 2);
            let fg = foreground_for(bg);
            (b, bg.hex(), fg.hex())
        })
        .collect();
    t.group = t3.elapsed();

    BlockingOutcome::Recognized(Recognized {
        blocks,
        scale: frame.scale_factor as f32,
        notices,
    })
}

fn ocr_status(e: Error) -> LensStatus {
    match e {
        Error::OcrLanguageMissing(lang) => LensStatus::Error {
            message_id: "ocr-error-language-missing",
            lang: Some(lang),
        },
        _ => LensStatus::Error {
            message_id: "ocr-error",
            lang: None,
        },
    }
}

fn log_timings(t: &Timings) {
    tracing::debug!(
        target: "pg::timings",
        capture_ms = t.capture.as_secs_f64() * 1000.0,
        change_ms = t.change.as_secs_f64() * 1000.0,
        ocr_ms = t.ocr.as_secs_f64() * 1000.0,
        group_ms = t.group.as_secs_f64() * 1000.0,
        translate_ms = t.translate.as_secs_f64() * 1000.0,
        total_ms = t.total.as_secs_f64() * 1000.0,
        "cycle"
    );
}

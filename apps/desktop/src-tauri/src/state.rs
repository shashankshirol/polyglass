//! Shared lens state: visibility, pause, interior region, and the pipeline driver handle.
//!
//! Every lens event funnels through here so the driver and the click-through poller always see
//! the same picture (LENS-06: hidden or paused means the pipeline is parked).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use pg_core::{LangTag, PhysRect, SourceLang};
use pg_pipeline::{DriverHandle, LensUpdate, Pipeline};
use pg_translate::Translator;
use serde::Serialize;

use crate::clickthrough::ClickThrough;
use crate::devflags::Trace;
use crate::hold::Hold;

/// What runs behind the lens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// `--demo`: mock capture, OCR and translation.
    Demo,
    /// Real capture, OCR and on-device translation.
    Live,
    /// No capture backend on this platform yet.
    None,
}

#[derive(Debug)]
pub struct Lens {
    pub click: Arc<ClickThrough>,
    /// Hold left Alt for the original (REN-04).
    pub hold: Arc<Hold>,
    pub mode: Mode,
    driver: OnceLock<DriverHandle>,
    pipeline: OnceLock<Arc<Pipeline>>,
    /// Fluent message ID if the pipeline couldn't start.
    pipeline_error: OnceLock<&'static str>,
    visible: AtomicBool,
    paused: AtomicBool,
    region: Mutex<Option<PhysRect>>,
    /// What the copy action copies: the text of the last update drawn (REN-05).
    copy_text: Mutex<String>,
    /// `--trace-timings`: told when the lens moves or is shown.
    pub trace: Option<Arc<Trace>>,
}

impl Lens {
    pub fn new(mode: Mode) -> Self {
        Self {
            click: ClickThrough::new(true),
            hold: Hold::new(),
            mode,
            driver: OnceLock::new(),
            pipeline: OnceLock::new(),
            pipeline_error: OnceLock::new(),
            visible: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            region: Mutex::new(None),
            copy_text: Mutex::new(String::new()),
            trace: None,
        }
    }

    /// With `--trace-timings`.
    pub fn with_trace(mut self, trace: Option<Arc<Trace>>) -> Self {
        self.trace = trace;
        self
    }

    fn moved(&self) {
        if let Some(t) = &self.trace {
            t.lens_moved(std::time::Instant::now());
            // `--trace-timings=uncached`: the result after coming to rest is translated afresh.
            if t.uncached()
                && let Some(p) = self.pipeline.get()
            {
                p.clear_cache();
            }
        }
    }

    /// Remember what `update` shows, for the copy action.
    pub fn set_last_update(&self, update: &LensUpdate) {
        *self.copy_text.lock().expect("copy lock") = update.copy_text();
    }

    /// The text the lens shows right now, in reading order (empty when it shows none).
    pub fn copy_text(&self) -> String {
        self.copy_text.lock().expect("copy lock").clone()
    }

    /// Whether a pipeline is running behind the lens.
    pub fn has_pipeline(&self) -> bool {
        self.driver.get().is_some()
    }

    pub fn pipeline_error(&self) -> Option<&'static str> {
        self.pipeline_error.get().copied()
    }

    pub fn set_pipeline_error(&self, message_id: &'static str) {
        let _ = self.pipeline_error.set(message_id);
    }

    /// Keep the running pipeline so its translator can be swapped (`set_translator`).
    pub fn set_pipeline(&self, pipeline: Arc<Pipeline>) {
        let _ = self.pipeline.set(pipeline);
    }

    /// Switch the running pipeline's backend and redo what's under the lens. Cached translations
    /// are dropped: they may come from a model that was just deleted. `false` if no pipeline runs
    /// yet.
    pub fn set_translator(&self, translator: Arc<dyn Translator>) -> bool {
        let Some(pipeline) = self.pipeline.get() else {
            return false;
        };
        pipeline.set_translator(translator);
        pipeline.clear_cache();
        self.with_driver(DriverHandle::invalidate);
        true
    }

    /// Translate from `source` into `target` from now on and redo what's under the lens (the
    /// swap, SV-01; the toolbar's pickers, MDL-07).
    pub fn set_languages(&self, source: SourceLang, target: LangTag) {
        let Some(pipeline) = self.pipeline.get() else {
            return;
        };
        let mut config = pipeline.config();
        if config.source != source || config.target != target {
            config.source = source;
            config.target = target;
            pipeline.set_config(config);
            self.with_driver(DriverHandle::invalidate);
        }
    }

    /// Attach the driver and bring it up to date with the current lens state.
    pub fn set_driver(&self, driver: DriverHandle) {
        if self.driver.set(driver).is_err() {
            return;
        }
        if let Some(r) = *self.region.lock().expect("region lock") {
            self.with_driver(|d| d.lens_moved(r));
        }
        self.sync_driver();
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }

    pub fn set_visible(&self, visible: bool) {
        if visible && !self.is_visible() {
            self.moved();
        }
        self.visible.store(visible, Ordering::SeqCst);
        self.click.set_visible(visible);
        self.hold.set_visible(visible);
        self.sync_driver();
    }

    pub fn set_paused(&self, paused: bool) {
        if !paused && self.paused.load(Ordering::SeqCst) {
            self.moved();
        }
        self.paused.store(paused, Ordering::SeqCst);
        if paused {
            // The UI clears the blocks on pause.
            self.copy_text.lock().expect("copy lock").clear();
        }
        self.sync_driver();
    }

    /// The window moved or was resized; `None` while it has no interior.
    pub fn set_region(&self, region: Option<PhysRect>) {
        let changed = {
            let mut cur = self.region.lock().expect("region lock");
            let changed = *cur != region;
            *cur = region;
            changed
        };
        if !changed {
            return;
        }
        self.moved();
        self.click.set_region(region);
        match region {
            Some(r) => self.with_driver(|d| d.lens_moved(r)),
            // Too small to capture anything: treat like hidden until it grows again.
            None => self.with_driver(DriverHandle::hide),
        }
        if region.is_some() {
            self.sync_driver();
        }
    }

    pub fn shutdown(&self) {
        self.click.shutdown();
        self.hold.shutdown();
        self.with_driver(DriverHandle::shutdown);
    }

    fn sync_driver(&self) {
        let active = self.is_visible()
            && !self.paused.load(Ordering::SeqCst)
            && self.region.lock().expect("region lock").is_some();
        if active {
            self.with_driver(DriverHandle::show);
        } else {
            self.with_driver(DriverHandle::hide);
        }
    }

    fn with_driver(&self, f: impl FnOnce(&DriverHandle)) {
        if let Some(d) = self.driver.get() {
            f(d);
        }
    }
}

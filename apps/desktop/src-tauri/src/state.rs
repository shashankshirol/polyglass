//! Shared lens state: visibility, pause, interior region, and the pipeline driver handle.
//!
//! Every lens event funnels through here so the driver and the click-through poller always see
//! the same picture (LENS-06: hidden or paused means the pipeline is parked).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use pg_core::PhysRect;
use pg_pipeline::DriverHandle;

use crate::clickthrough::ClickThrough;

#[derive(Debug)]
pub struct Lens {
    pub click: Arc<ClickThrough>,
    pub demo: bool,
    driver: OnceLock<DriverHandle>,
    visible: AtomicBool,
    paused: AtomicBool,
    region: Mutex<Option<PhysRect>>,
}

impl Lens {
    pub fn new(demo: bool) -> Self {
        Self {
            click: ClickThrough::new(true),
            demo,
            driver: OnceLock::new(),
            visible: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            region: Mutex::new(None),
        }
    }

    /// Whether a pipeline is running behind the lens (only with `--demo` until M1-CAP-1).
    pub fn has_pipeline(&self) -> bool {
        self.driver.get().is_some()
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
        self.visible.store(visible, Ordering::SeqCst);
        self.click.set_visible(visible);
        self.sync_driver();
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
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

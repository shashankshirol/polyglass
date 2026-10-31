//! Click-through interior (LENS-04, PLAN §5 spike, ADR-0002).
//!
//! `set_ignore_cursor_events` is whole-window, so a background thread polls the cursor at about
//! 30 Hz and makes the window ignore the mouse only while the cursor is over the interior. The
//! frame, handle and toolbar stay interactive. The thread parks (no polling at all) while the
//! lens is hidden or click-through is switched off.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, Thread};
use std::time::Duration;

use pg_core::PhysRect;
use tauri::{AppHandle, WebviewWindow};

/// ~30 Hz (PLAN §5).
pub const POLL_INTERVAL: Duration = Duration::from_millis(33);

#[derive(Debug)]
pub struct ClickThrough {
    enabled: AtomicBool,
    visible: AtomicBool,
    stop: AtomicBool,
    /// Interior in physical px, kept current from window move/resize events so the poller
    /// never has to query the window.
    region: Mutex<Option<PhysRect>>,
    toggles: AtomicU64,
    thread: OnceLock<Thread>,
}

impl ClickThrough {
    pub fn new(enabled: bool) -> Arc<Self> {
        Arc::new(Self {
            enabled: AtomicBool::new(enabled),
            visible: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            region: Mutex::new(None),
            toggles: AtomicU64::new(0),
            thread: OnceLock::new(),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
        self.wake();
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
        self.wake();
    }

    pub fn set_region(&self, region: Option<PhysRect>) {
        *self.region.lock().expect("region lock") = region;
    }

    /// How often the window switched between interactive and click-through.
    pub fn toggles(&self) -> u64 {
        self.toggles.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake();
    }

    fn wake(&self) {
        if let Some(t) = self.thread.get() {
            t.unpark();
        }
    }

    /// Start the poller for `window`. Call once.
    pub fn spawn(self: &Arc<Self>, app: AppHandle, window: WebviewWindow) {
        let state = self.clone();
        let handle = thread::Builder::new()
            .name("pg-clickthrough".into())
            .spawn(move || state.run(&app, &window))
            .expect("spawn click-through thread");
        let _ = self.thread.set(handle.thread().clone());
    }

    fn run(&self, app: &AppHandle, window: &WebviewWindow) {
        let mut ignoring = false;
        while !self.stop.load(Ordering::SeqCst) {
            let active = self.visible.load(Ordering::SeqCst) && self.is_enabled();
            let want = active
                && app.cursor_position().is_ok_and(|c| {
                    over_interior(*self.region.lock().expect("region lock"), c.x, c.y)
                });
            if want != ignoring && window.set_ignore_cursor_events(want).is_ok() {
                ignoring = want;
                self.toggles.fetch_add(1, Ordering::Relaxed);
            }
            if active {
                thread::sleep(POLL_INTERVAL);
            } else {
                // Hidden or switched off: no polling until something changes (LENS-06).
                thread::park();
            }
        }
    }
}

/// Whether the cursor (physical virtual-desktop px) is over the click-through interior.
pub fn over_interior(region: Option<PhysRect>, x: f64, y: f64) -> bool {
    region.is_some_and(|r| {
        x >= f64::from(r.x) && y >= f64::from(r.y) && x < r.right() as f64 && y < r.bottom() as f64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens04_only_the_interior_is_click_through() {
        let r = Some(PhysRect::new(106, 80, 468, 164));
        assert!(over_interior(r, 106.0, 80.0));
        assert!(over_interior(r, 573.9, 243.9));
        // Frame, toolbar and outside stay interactive.
        assert!(!over_interior(r, 105.9, 100.0));
        assert!(!over_interior(r, 200.0, 79.0));
        assert!(!over_interior(r, 574.0, 100.0));
        assert!(!over_interior(r, 200.0, 244.0));
        assert!(!over_interior(None, 200.0, 100.0));
    }

    #[test]
    fn negative_monitor_coordinates() {
        let r = Some(PhysRect::new(-1911, 345, 702, 246));
        assert!(over_interior(r, -1500.0, 400.0));
        assert!(!over_interior(r, -1912.0, 400.0));
    }
}

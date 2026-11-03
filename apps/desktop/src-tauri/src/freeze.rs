//! Freeze mode (MODE-03): one snapshot of the monitor under the lens, shown fullscreen under the
//! lens, which then reads the snapshot instead of the live screen while it is moved over it.
//!
//! The snapshot is taken through the pipeline's own capturer (so the lens isn't in it, CAP-02),
//! kept in memory only and dropped when Freeze mode ends (PRIV-02). The `freeze` window gets the
//! pixels over IPC and draws them into a canvas; nothing is written to disk. Entered and left
//! with Ctrl+Alt+F (HK-01) or the toolbar's ❄; Esc in the freeze window or the lens leaves it.

use std::sync::{Arc, Mutex, PoisonError};

use pg_capture::ScreenCapturer;
use pg_core::{Frame, PhysRect, PixelFormat, Result};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::lens;
use crate::state::Lens;

/// Label of the fullscreen window that shows the snapshot.
pub const WINDOW: &str = "freeze";
/// Event carrying `true` while the lens is frozen.
pub const EVENT: &str = "lens-frozen";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Live,
    /// The snapshot is being taken or its window is opening.
    Entering,
    Frozen,
}

/// Freeze mode's state, shared by the app and the pipeline's capturer ([`Freezable`]).
#[derive(Debug)]
pub struct Freeze {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    phase: Phase,
    /// The only long-lived copy of the snapshot (PRIV-02: dropped by [`Freeze::thaw`]).
    snapshot: Option<Arc<Frame>>,
}

impl Freeze {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                phase: Phase::Live,
                snapshot: None,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether Freeze mode is on or being entered.
    pub fn is_active(&self) -> bool {
        self.lock().phase != Phase::Live
    }

    /// Start entering; `false` when Freeze mode is already on or being entered.
    fn begin(&self) -> bool {
        let mut inner = self.lock();
        if inner.phase != Phase::Live {
            return false;
        }
        inner.phase = Phase::Entering;
        true
    }

    /// Hold `frame` and read regions from it from now on. `false` (and the frame dropped) when
    /// Freeze mode was left while the snapshot was being taken.
    fn hold(&self, frame: Frame) -> bool {
        let mut inner = self.lock();
        if inner.phase != Phase::Entering {
            return false;
        }
        inner.phase = Phase::Frozen;
        inner.snapshot = Some(Arc::new(frame));
        true
    }

    /// The snapshot while frozen.
    pub fn snapshot(&self) -> Option<Arc<Frame>> {
        self.lock().snapshot.clone()
    }

    /// Back to the live screen; the snapshot is dropped (PRIV-02). `true` if it was frozen or
    /// being frozen.
    pub fn thaw(&self) -> bool {
        let mut inner = self.lock();
        inner.snapshot = None;
        std::mem::replace(&mut inner.phase, Phase::Live) != Phase::Live
    }
}

/// The pipeline's capturer: while frozen, regions on the snapshot's monitor are cut from the
/// snapshot; everything else is captured live.
pub struct Freezable<C> {
    inner: C,
    freeze: Arc<Freeze>,
}

impl<C> std::fmt::Debug for Freezable<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Freezable").finish_non_exhaustive()
    }
}

impl<C: ScreenCapturer> Freezable<C> {
    pub fn new(inner: C, freeze: Arc<Freeze>) -> Self {
        Self { inner, freeze }
    }
}

impl<C: ScreenCapturer> ScreenCapturer for Freezable<C> {
    fn capture(&mut self, region: PhysRect) -> Result<Frame> {
        if let Some(snapshot) = self.freeze.snapshot()
            && let Some(clip) = region.intersect(&snapshot.origin)
        {
            let (x, y) = (clip.x - snapshot.origin.x, clip.y - snapshot.origin.y);
            return Ok(snapshot.crop(x as u32, y as u32, clip.width, clip.height));
        }
        self.inner.capture(region)
    }

    fn excludes_self(&self) -> bool {
        self.inner.excludes_self()
    }

    fn release(&mut self) {
        self.inner.release();
    }
}

/// Enter Freeze mode, or leave it when it is on (Ctrl+Alt+F, the toolbar's ❄).
pub fn toggle(app: &AppHandle) {
    if app.state::<Lens>().freeze.is_active() {
        exit(app);
    } else {
        enter(app);
    }
}

/// Take the snapshot of the monitor under the lens's toolbar and open the window that shows it.
/// The lens stays on top and reads the snapshot until [`exit`].
pub fn enter(app: &AppHandle) {
    let state = app.state::<Lens>();
    let (Some(pipeline), Some(window)) = (state.pipeline(), lens::get(app)) else {
        tracing::info!("freeze: no pipeline behind the lens");
        return;
    };
    if !state.is_visible() || !state.freeze.begin() {
        return;
    }
    let Some(monitor) = monitor_under(&window) else {
        state.freeze.thaw();
        return;
    };
    let _ = app.emit_to(lens::LABEL, EVENT, true);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        let taken = tokio::task::spawn_blocking(move || pipeline.capture(monitor)).await;
        let frame = match taken {
            Ok(Ok(frame)) => frame,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "freeze: the snapshot failed");
                exit(&app);
                return;
            }
            Err(_) => {
                exit(&app);
                return;
            }
        };
        tracing::info!(
            width = frame.width(),
            height = frame.height(),
            ms = started.elapsed().as_millis() as u64,
            "freeze: snapshot taken (memory only)"
        );
        if !app.state::<Lens>().freeze.hold(frame) {
            return;
        }
        if let Err(e) = open_window(&app, monitor) {
            tracing::warn!(error = %e, "freeze: could not open the snapshot window");
            exit(&app);
        }
    });
}

/// Leave Freeze mode: close the snapshot window and drop the snapshot (PRIV-02). The pipeline
/// reads the live screen again from its next capture.
pub fn exit(app: &AppHandle) {
    if !app.state::<Lens>().freeze.thaw() {
        return;
    }
    if let Some(w) = app.get_webview_window(WINDOW) {
        // Destroy, not close: the window's canvas holds the pixels too.
        let _ = w.destroy();
    }
    let _ = app.emit_to(lens::LABEL, EVENT, false);
    tracing::info!("freeze: back to the live screen");
}

/// The whole monitor under the lens's toolbar (physical px).
fn monitor_under(window: &WebviewWindow) -> Option<PhysRect> {
    let (x, y, _, _) = lens::geometry(window)?;
    let m = window
        .monitor_from_point(f64::from(x) + 16.0, f64::from(y) + 8.0)
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())?;
    let (p, s) = (m.position(), m.size());
    Some(PhysRect::new(p.x, p.y, s.width, s.height))
}

/// The snapshot window: borderless, on top, covering `monitor` exactly. It stays hidden until
/// its page has drawn the snapshot (`freeze_ready`), so the live screen never flashes black.
fn open_window(app: &AppHandle, monitor: PhysRect) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("freeze.html".into()))
        .title("Polyglass – Freeze")
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .additional_browser_args(lens::WEBVIEW_ARGS)
        .build()?;
    // Physical px, after the move: moving onto a monitor with another scale resizes the window.
    window.set_position(PhysicalPosition::new(monitor.x, monitor.y))?;
    window.set_size(PhysicalSize::new(monitor.width, monitor.height))?;
    Ok(())
}

/// The snapshot window drew the snapshot: show it under the lens.
pub fn shown(app: &AppHandle, window: &WebviewWindow) -> tauri::Result<()> {
    window.show()?;
    if let Some(lens) = lens::get(app) {
        // Back on top of the snapshot, with the keyboard focus for Esc and the arrow keys.
        lens.set_focus()?;
    }
    Ok(())
}

/// The snapshot as the freeze window's canvas wants it: width and height (u32, little-endian),
/// then RGBA rows without padding.
pub fn rgba(frame: &Frame) -> Vec<u8> {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let mut out = Vec::with_capacity(8 + w * h * 4);
    out.extend_from_slice(&frame.width().to_le_bytes());
    out.extend_from_slice(&frame.height().to_le_bytes());
    let (data, stride) = (frame.data(), frame.stride());
    for row in 0..h {
        match frame.format() {
            PixelFormat::Rgba8 => out.extend_from_slice(&data[row * stride..][..w * 4]),
            PixelFormat::Bgra8 => {
                for p in data[row * stride..][..w * 4].as_chunks::<4>().0 {
                    out.extend_from_slice(&[p[2], p[1], p[0], 255]);
                }
            }
            PixelFormat::Gray8 => {
                for &v in &data[row * stride..][..w] {
                    out.extend_from_slice(&[v, v, v, 255]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_capture::StaticCapturer;

    /// A "live" desktop of two monitors and a snapshot of the right one in another colour.
    fn setup() -> (Arc<Freeze>, Freezable<StaticCapturer>) {
        let live = Frame::solid(400, 100, [200, 200, 200])
            .with_origin(PhysRect::new(-200, 0, 400, 100), 1.0);
        let freeze = Freeze::new();
        (
            freeze.clone(),
            Freezable::new(StaticCapturer::new(live), freeze),
        )
    }

    fn snapshot() -> Frame {
        let mut f = Frame::solid(200, 100, [10, 20, 30]);
        // One marked pixel at (5, 7) of the monitor.
        f.data_mut()[(7 * 200 + 5) * 4..][..3].copy_from_slice(&[99, 98, 97]);
        f.with_origin(PhysRect::new(0, 0, 200, 100), 1.5)
    }

    #[test]
    fn mode03_frozen_lens_reads_the_snapshot() {
        let (freeze, mut capturer) = setup();
        let region = PhysRect::new(5, 7, 40, 20);
        assert_eq!(capturer.capture(region).unwrap().rgb(0, 0), [200, 200, 200]);

        assert!(freeze.begin());
        assert!(!freeze.begin(), "entering twice");
        assert!(freeze.hold(snapshot()));
        assert!(freeze.is_active());
        let f = capturer.capture(region).unwrap();
        assert_eq!(f.origin, region);
        assert_eq!(f.scale_factor, 1.5);
        assert_eq!(f.rgb(0, 0), [99, 98, 97], "the marked snapshot pixel");
        assert_eq!(f.rgb(1, 0), [10, 20, 30]);
        // Off the frozen monitor the lens still reads the live screen.
        let other = capturer.capture(PhysRect::new(-150, 10, 40, 20)).unwrap();
        assert_eq!(other.rgb(0, 0), [200, 200, 200]);
        // Straddling the edge: only the frozen part.
        let edge = capturer.capture(PhysRect::new(-20, 0, 40, 20)).unwrap();
        assert_eq!(edge.origin, PhysRect::new(0, 0, 20, 20));

        assert!(freeze.thaw());
        assert!(!freeze.is_active());
        assert_eq!(capturer.capture(region).unwrap().rgb(0, 0), [200, 200, 200]);
    }

    #[test]
    fn priv02_snapshot_is_dropped_when_freeze_ends() {
        let (freeze, mut capturer) = setup();
        assert!(freeze.begin());
        assert!(freeze.hold(snapshot()));
        let weak = Arc::downgrade(&freeze.snapshot().unwrap());
        capturer.capture(PhysRect::new(0, 0, 10, 10)).unwrap();
        assert!(weak.upgrade().is_some());
        assert!(freeze.thaw());
        assert!(
            weak.upgrade().is_none(),
            "nothing keeps the snapshot after Freeze mode ends"
        );
        assert!(!freeze.thaw(), "already live");
    }

    #[test]
    fn mode03_leaving_while_the_snapshot_is_taken_drops_it() {
        let (freeze, _) = setup();
        assert!(freeze.begin());
        assert!(freeze.thaw());
        assert!(!freeze.hold(snapshot()));
        assert!(freeze.snapshot().is_none());
    }

    #[test]
    fn mode03_snapshot_pixels_for_the_canvas() {
        let bgra = Frame::new(
            2,
            1,
            12,
            PixelFormat::Bgra8,
            vec![1, 2, 3, 0, 4, 5, 6, 0, 0xEE, 0xEE, 0xEE, 0xEE],
        )
        .unwrap();
        assert_eq!(
            rgba(&bgra),
            [2, 0, 0, 0, 1, 0, 0, 0, 3, 2, 1, 255, 6, 5, 4, 255]
        );
    }
}

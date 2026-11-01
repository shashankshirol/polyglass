//! Lens window geometry: creation and placement (LENS-01), keyboard nudging (LENS-02), size
//! limits (LENS-03), and the interior region handed to the pipeline (CAP-01, CAP-04).

use pg_core::{LogicalRect, PhysRect};
use serde::Serialize;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::settings::LensSettings;

pub const LABEL: &str = "lens";

/// First-launch size in logical px (LENS-01).
pub const DEFAULT_SIZE: (f64, f64) = (480.0, 200.0);
/// Minimum size in logical px (LENS-03).
pub const MIN_SIZE: (f64, f64) = (120.0, 48.0);

/// Where the interactive chrome sits, in logical px. Rust owns these numbers and the UI reads
/// them through `lens_config`, so the click-through hit-test and the CSS can't drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// Frame thickness on the left, right and bottom edges (drag + resize zone).
    pub frame: f64,
    /// Toolbar height at the top; it doubles as the drag handle.
    pub toolbar: f64,
}

pub const LAYOUT: Layout = Layout {
    frame: 6.0,
    toolbar: 30.0,
};

impl Layout {
    /// The lens interior (what gets captured and what is click-through) for a window whose
    /// client area is `window` (logical px). Empty when the window is smaller than the chrome.
    pub fn interior(&self, window: LogicalRect) -> LogicalRect {
        LogicalRect::new(
            window.x + self.frame,
            window.y + self.toolbar,
            (window.width - 2.0 * self.frame).max(0.0),
            (window.height - self.toolbar - self.frame).max(0.0),
        )
    }
}

/// Interior region of `window` in physical virtual-desktop pixels, or `None` while the window is
/// minimised or too small to have an interior.
pub fn interior_region(window: &WebviewWindow) -> Option<PhysRect> {
    let pos = window.inner_position().ok()?;
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    interior_from_physical(pos.x, pos.y, size.width, size.height, scale)
}

fn interior_from_physical(x: i32, y: i32, w: u32, h: u32, scale: f64) -> Option<PhysRect> {
    if w == 0 || h == 0 || scale <= 0.0 {
        return None;
    }
    let logical = LogicalRect::new(
        f64::from(x) / scale,
        f64::from(y) / scale,
        f64::from(w) / scale,
        f64::from(h) / scale,
    );
    let region = LAYOUT.interior(logical).to_physical(scale);
    (!region.is_empty()).then_some(region)
}

/// Create the (hidden) lens window. It is shown by [`place_and_show`].
pub fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Polyglass lens")
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .maximizable(false)
        .minimizable(false)
        .visible(false)
        .build()
}

/// Where the lens was last (outer position, physical px) and its size (logical px).
pub fn geometry(window: &WebviewWindow) -> Option<(i32, i32, f64, f64)> {
    let pos = window.outer_position().ok()?;
    let scale = window.scale_factor().ok()?;
    let size: LogicalSize<f64> = window.inner_size().ok()?.to_logical(scale);
    Some((pos.x, pos.y, size.width, size.height))
}

/// Whether a lens whose outer top-left corner was at (`x`, `y`) can go back there: its toolbar
/// (where it is dragged from) must start on one of `monitors` (physical px), e.g. not on a
/// monitor that has been unplugged.
pub fn restorable(x: i32, y: i32, monitors: &[PhysRect]) -> bool {
    let (px, py) = (i64::from(x) + 16, i64::from(y) + 8);
    monitors
        .iter()
        .any(|m| px >= i64::from(m.x) && py >= i64::from(m.y) && px < m.right() && py < m.bottom())
}

/// Put the lens back where it was (LENS-01) when that spot is still on a monitor; otherwise
/// centre it on the monitor under the cursor. Cap it to that monitor's work area (LENS-03),
/// then show it.
pub fn place_and_show(
    app: &AppHandle,
    window: &WebviewWindow,
    saved: &LensSettings,
) -> tauri::Result<()> {
    let monitors: Vec<PhysRect> = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            PhysRect::new(p.x, p.y, s.width, s.height)
        })
        .collect();
    let restore = match (saved.x, saved.y) {
        (Some(x), Some(y)) if restorable(x, y, &monitors) => Some((x, y)),
        _ => None,
    };
    let size = (saved.width.max(MIN_SIZE.0), saved.height.max(MIN_SIZE.1));
    let monitor = match restore {
        Some((x, y)) => app
            .monitor_from_point(f64::from(x) + 16.0, f64::from(y) + 8.0)
            .ok()
            .flatten(),
        None => app
            .cursor_position()
            .ok()
            .and_then(|c| app.monitor_from_point(c.x, c.y).ok().flatten()),
    }
    .or_else(|| app.primary_monitor().ok().flatten());

    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let area = m.work_area();
        let area_w = f64::from(area.size.width) / scale;
        let area_h = f64::from(area.size.height) / scale;
        let w = size.0.min(area_w);
        let h = size.1.min(area_h);
        window.set_max_size(Some(LogicalSize::new(area_w, area_h)))?;
        window.set_size(LogicalSize::new(w, h))?;
        let (x, y) = restore.unwrap_or_else(|| {
            let x = f64::from(area.position.x) + (area_w - w) / 2.0 * scale;
            let y = f64::from(area.position.y) + (area_h - h) / 2.0 * scale;
            (x.round() as i32, y.round() as i32)
        });
        window.set_position(PhysicalPosition::new(x, y))?;
    } else {
        window.center()?;
    }
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// Move the lens by `(dx, dy)` logical px (LENS-02 arrow keys). Inputs are validated by
/// `commands::nudge_lens` before they get here.
pub fn nudge(window: &WebviewWindow, dx: i32, dy: i32) -> tauri::Result<()> {
    let scale = window.scale_factor()?;
    let pos: LogicalPosition<f64> = window.outer_position()?.to_logical(scale);
    window.set_position(LogicalPosition::new(
        pos.x + f64::from(dx),
        pos.y + f64::from(dy),
    ))
}

pub fn get(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Keep the lens out of every screen capture, including our own, so OCR never reads the
/// translations it draws (CAP-02). Needs Windows 10 2004+. Returns whether it worked.
#[cfg(windows)]
pub fn exclude_from_capture(window: &WebviewWindow) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };
    let Ok(hwnd) = window.hwnd() else {
        return false;
    };
    // SAFETY: `hwnd` is the live lens window owned by this process; the call only sets a window
    // attribute and fails cleanly on older Windows. One of the two `unsafe` calls in the desktop
    // shell; the other reads the key state (`hold.rs`).
    #[allow(unsafe_code)]
    let result = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) };
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(error = %e, "could not exclude the lens from screen capture");
            false
        }
    }
}

#[cfg(not(windows))]
pub fn exclude_from_capture(_window: &WebviewWindow) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens01_restores_only_onto_a_monitor_that_is_there() {
        // This machine: 2560×1440 primary, 2160×3840 above-left at negative coordinates.
        let monitors = [
            PhysRect::new(0, 0, 2560, 1440),
            PhysRect::new(-2160, -2398, 2160, 3840),
        ];
        assert!(restorable(1040, 596, &monitors));
        assert!(restorable(-1500, -2000, &monitors));
        // The second monitor unplugged: its old spot is off every monitor.
        assert!(!restorable(-1500, -2000, &monitors[..1]));
        // Toolbar off the right edge.
        assert!(!restorable(2550, 100, &monitors[..1]));
    }

    #[test]
    fn interior_excludes_frame_and_toolbar() {
        let i = LAYOUT.interior(LogicalRect::new(100.0, 50.0, 480.0, 200.0));
        assert_eq!(i, LogicalRect::new(106.0, 80.0, 468.0, 164.0));
    }

    #[test]
    fn lens03_minimum_size_still_has_an_interior() {
        let i = LAYOUT.interior(LogicalRect::new(0.0, 0.0, MIN_SIZE.0, MIN_SIZE.1));
        assert!(i.width > 0.0 && i.height > 0.0, "{i:?}");
    }

    #[test]
    fn cap04_interior_region_at_fractional_scale() {
        // 480×200 logical at 150% on a monitor whose origin is left of the primary one.
        let r = interior_from_physical(-1920, 300, 720, 300, 1.5).unwrap();
        assert_eq!(r, PhysRect::new(-1911, 345, 702, 246));
        // Covers the whole logical interior even when it lands between pixels (125%).
        let r = interior_from_physical(101, 0, 600, 250, 1.25).unwrap();
        assert_eq!(r.x, 108); // floor((101/1.25 + 6) * 1.25) = floor(108.5)
        assert!(r.right() >= 101 + 600 - 7);
    }

    #[test]
    fn no_interior_when_minimised_or_tiny() {
        assert_eq!(interior_from_physical(0, 0, 0, 0, 1.0), None);
        assert_eq!(interior_from_physical(0, 0, 10, 30, 1.0), None);
    }
}

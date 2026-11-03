//! Lens window geometry: creation and placement (LENS-01), keyboard nudging (LENS-02), size
//! limits (LENS-03), and the interior region handed to the pipeline (CAP-01, CAP-04).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use pg_core::{LogicalRect, PhysRect};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::settings::LensSettings;
use crate::state::Lens;

pub const LABEL: &str = "lens";

/// First-launch size in logical px (LENS-01).
pub const DEFAULT_SIZE: (f64, f64) = (480.0, 200.0);
/// Minimum size in logical px (LENS-03).
pub const MIN_SIZE: (f64, f64) = (120.0, 48.0);

/// Event carrying the window's [`Strip`] after the mode changed or the panel flipped (MODE-02).
pub const STRIP_EVENT: &str = "lens-strip";

/// Where the interactive chrome sits, in logical px. Rust owns these numbers and the UI reads
/// them through `lens_config`, so the click-through hit-test and the CSS can't drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    /// Frame thickness on the left, right and bottom edges (drag + resize zone).
    pub frame: f64,
    /// Toolbar height at the top; it doubles as the drag handle.
    pub toolbar: f64,
    /// Height of the Panel mode strip (MODE-02). A multiple of 8, so it is whole physical px at
    /// 100/125/150/175/200% and a flip moves the window without moving the interior.
    pub panel: f64,
}

pub const LAYOUT: Layout = Layout {
    frame: 6.0,
    toolbar: 30.0,
    panel: 120.0,
};

/// How translations are shown (MODE-01, MODE-02); saved in `settings.toml` as `[lens] mode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayMode {
    /// Each translation drawn over its source block (MODE-01).
    #[default]
    Overlay,
    /// The interior stays transparent; translations are listed in a strip attached to the lens
    /// (MODE-02).
    Panel,
}

/// Where the window has the Panel mode strip: none in Overlay mode, else below the lens, or
/// above it when there is no room below (MODE-02).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Strip {
    #[default]
    None,
    Below,
    Above,
}

impl Layout {
    /// The lens interior (what gets captured and what is click-through) for a lens whose
    /// toolbar, frame and interior take up `lens` (logical px; the window without the Panel mode
    /// strip). Empty when the lens is smaller than the chrome.
    pub fn interior(&self, lens: LogicalRect) -> LogicalRect {
        LogicalRect::new(
            lens.x + self.frame,
            lens.y + self.toolbar,
            (lens.width - 2.0 * self.frame).max(0.0),
            (lens.height - self.toolbar - self.frame).max(0.0),
        )
    }
}

/// Interior region of `window` in physical virtual-desktop pixels, or `None` while the window is
/// minimised or too small to have an interior.
pub fn interior_region(window: &WebviewWindow) -> Option<PhysRect> {
    let pos = window.inner_position().ok()?;
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    let strip = window.state::<Lens>().strip();
    interior_from_physical(pos.x, pos.y, size.width, size.height, scale, strip)
}

fn interior_from_physical(
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    scale: f64,
    strip: Strip,
) -> Option<PhysRect> {
    if w == 0 || h == 0 || scale <= 0.0 {
        return None;
    }
    // In whole px first, so the strip never shifts the interior by a rounding error.
    let lens = lens_for(PhysRect::new(x, y, w, h), strip, panel_px(scale));
    let logical = LogicalRect::new(
        f64::from(lens.x) / scale,
        f64::from(lens.y) / scale,
        f64::from(lens.width) / scale,
        f64::from(lens.height) / scale,
    );
    let region = LAYOUT.interior(logical).to_physical(scale);
    (!region.is_empty()).then_some(region)
}

/// The strip's height in physical px at `scale`.
fn panel_px(scale: f64) -> u32 {
    (LAYOUT.panel * scale).round() as u32
}

/// MODE-02: where the panel goes for a lens at `lens` on a monitor whose work area is `area`
/// (physical px), the panel being `panel` px tall. Below when it fits there, else above when it
/// fits there, else on the side with more room.
pub fn panel_side(lens: PhysRect, area: PhysRect, panel: u32) -> Strip {
    let below = area.bottom() - lens.bottom();
    let above = i64::from(lens.y) - i64::from(area.y);
    let panel = i64::from(panel);
    if below >= panel || (above < panel && below >= above) {
        Strip::Below
    } else {
        Strip::Above
    }
}

/// The window's outer rect for a lens at `lens` with `strip` (physical px).
fn window_for(lens: PhysRect, strip: Strip, panel: u32) -> PhysRect {
    match strip {
        Strip::None => lens,
        Strip::Below => PhysRect::new(lens.x, lens.y, lens.width, lens.height + panel),
        Strip::Above => PhysRect::new(
            lens.x,
            lens.y - panel as i32,
            lens.width,
            lens.height + panel,
        ),
    }
}

/// The lens part of a window at `window` with `strip` (physical px).
fn lens_for(window: PhysRect, strip: Strip, panel: u32) -> PhysRect {
    match strip {
        Strip::None => window,
        Strip::Below => PhysRect::new(
            window.x,
            window.y,
            window.width,
            window.height.saturating_sub(panel),
        ),
        Strip::Above => PhysRect::new(
            window.x,
            window.y + panel as i32,
            window.width,
            window.height.saturating_sub(panel),
        ),
    }
}

/// The window's outer position and client size (physical px; the lens has no decorations, so
/// they describe the same rect) and its scale.
fn window_rect(window: &WebviewWindow) -> Option<(PhysRect, f64)> {
    let pos = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    let scale = window.scale_factor().ok()?;
    Some((PhysRect::new(pos.x, pos.y, size.width, size.height), scale))
}

/// Lay the window out for `mode` (MODE-02): add the strip below the lens, or above it when there
/// is no room below on the lens's monitor, or remove it, keeping the lens where it is. In Panel
/// mode it also flips the strip when the lens moved. Call on the main thread: then both geometry
/// calls take effect before the window events they cause are handled, so those events never see
/// a half-moved window.
pub fn apply_mode(window: &WebviewWindow, mode: DisplayMode) -> tauri::Result<()> {
    let state = window.state::<Lens>();
    let current = state.strip();
    let Some((win, scale)) = window_rect(window) else {
        return Ok(());
    };
    let panel = panel_px(scale);
    let lens = lens_for(win, current, panel);
    let next = match mode {
        DisplayMode::Overlay => Strip::None,
        DisplayMode::Panel => work_area(window.app_handle(), lens)
            .map_or(Strip::Below, |area| panel_side(lens, area, panel)),
    };
    if next == current {
        return Ok(());
    }
    set_strip(window, next)?;
    let target = window_for(lens, next, panel);
    if target.height != win.height {
        window.set_size(PhysicalSize::new(target.width, target.height))?;
    }
    if target.y != win.y {
        window.set_position(PhysicalPosition::new(target.x, target.y))?;
    }
    Ok(())
}

/// Record `strip`, tell the UI and keep the minimum size a whole lens plus the strip.
fn set_strip(window: &WebviewWindow, strip: Strip) -> tauri::Result<()> {
    window.state::<Lens>().set_strip(strip);
    let _ = window.emit_to(LABEL, STRIP_EVENT, strip);
    window.set_min_size(Some(LogicalSize::new(
        MIN_SIZE.0,
        MIN_SIZE.1 + strip_height(strip),
    )))
}

/// The strip's height in logical px (0 in Overlay mode).
fn strip_height(strip: Strip) -> f64 {
    if strip == Strip::None {
        0.0
    } else {
        LAYOUT.panel
    }
}

/// Work area of the monitor under the lens's toolbar (physical px).
fn work_area(app: &AppHandle, lens: PhysRect) -> Option<PhysRect> {
    let m = app
        .monitor_from_point(f64::from(lens.x) + 16.0, f64::from(lens.y) + 8.0)
        .ok()
        .flatten()?;
    let area = m.work_area();
    Some(PhysRect::new(
        area.position.x,
        area.position.y,
        area.size.width,
        area.size.height,
    ))
}

/// Whether a mouse button is down, i.e. the lens may be in the middle of a drag or resize.
#[cfg(windows)]
fn mouse_button_down() -> bool {
    crate::hold::mouse_button_down()
}

#[cfg(not(windows))]
fn mouse_button_down() -> bool {
    false
}

/// The lens moved or was resized: in Panel mode, flip the strip if it no longer fits on its
/// side (MODE-02). During a drag the flip waits for the mouse button to come up, since moving
/// the window inside the OS move loop would make it jump under the cursor. Main thread (window
/// events).
pub fn follow_panel(window: &WebviewWindow, waiting: &Arc<AtomicBool>) {
    if window.state::<Lens>().strip() == Strip::None {
        return;
    }
    if !mouse_button_down() {
        flip_panel(window);
        return;
    }
    if waiting.swap(true, Ordering::SeqCst) {
        return;
    }
    let (w, waiting) = (window.clone(), waiting.clone());
    std::thread::spawn(move || {
        while mouse_button_down() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        waiting.store(false, Ordering::SeqCst);
        let app = w.app_handle().clone();
        let _ = app.run_on_main_thread(move || flip_panel(&w));
    });
}

fn flip_panel(window: &WebviewWindow) {
    if window.state::<Lens>().strip() != Strip::None
        && let Err(e) = apply_mode(window, DisplayMode::Panel)
    {
        tracing::warn!(error = %e, "could not move the panel");
    }
}

/// WebView2 arguments for every window. All windows share one WebView2 environment, so they
/// must be the same everywhere. Wry's default `--disable-features` list, plus switches that
/// stop the runtime's own background traffic (variations and component updates, pings, metrics
/// and domain-reliability uploads, sync): without them WebView2 connected to Microsoft servers
/// while the app was in on-device mode (PRIV-01, `scripts/lens-net.ps1`). `msOneAuthWAM` keeps
/// the runtime from loading its sign-in library (`oneauth.dll`), which otherwise fetched the
/// Windows account's profile from `substrate.office.com` over WinHTTP (outside Chromium's
/// network stack, so `--disable-background-networking` doesn't cover it). Ignored off Windows.
pub const WEBVIEW_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,\
    msOneAuthWAM --disable-background-networking --disable-component-update \
    --disable-domain-reliability --disable-sync --metrics-recording-only --no-pings";

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
        .additional_browser_args(WEBVIEW_ARGS)
        .build()
}

/// Where the lens was last (outer position, physical px) and its size (logical px), without the
/// Panel mode strip.
pub fn geometry(window: &WebviewWindow) -> Option<(i32, i32, f64, f64)> {
    let (win, scale) = window_rect(window)?;
    let lens = lens_for(win, window.state::<Lens>().strip(), panel_px(scale));
    let size: LogicalSize<f64> = PhysicalSize::new(lens.width, lens.height).to_logical(scale);
    Some((lens.x, lens.y, size.width, size.height))
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
/// add the strip in Panel mode (MODE-02), then show it.
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
        let (area_w, area_h) = max_size(&m);
        let w = size.0.min(area_w);
        let h = size.1.min(area_h);
        let (x, y) = restore.unwrap_or_else(|| {
            let x = f64::from(area.position.x) + (area_w - w) / 2.0 * scale;
            let y = f64::from(area.position.y) + (area_h - h) / 2.0 * scale;
            (x.round() as i32, y.round() as i32)
        });
        // MODE-02: decided from the target monitor's numbers; the window only gets them once
        // the move below has changed its scale.
        let panel = panel_px(scale);
        let lens = PhysRect::new(x, y, (w * scale).round() as u32, (h * scale).round() as u32);
        let strip = match saved.mode {
            DisplayMode::Overlay => Strip::None,
            DisplayMode::Panel => {
                let area = PhysRect::new(
                    area.position.x,
                    area.position.y,
                    area.size.width,
                    area.size.height,
                );
                panel_side(lens, area, panel)
            }
        };
        set_strip(window, strip)?;
        window.set_max_size(Some(LogicalSize::new(area_w, area_h)))?;
        // Logical, so a move onto a monitor with another scale keeps it.
        window.set_size(LogicalSize::new(w, h + strip_height(strip)))?;
        let at = window_for(lens, strip, panel);
        window.set_position(PhysicalPosition::new(at.x, at.y))?;
    } else {
        window.center()?;
    }
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// The largest lens on monitor `m`: its work area in logical px (LENS-03).
fn max_size(m: &tauri::Monitor) -> (f64, f64) {
    let (scale, area) = (m.scale_factor(), m.work_area());
    (
        f64::from(area.size.width) / scale,
        f64::from(area.size.height) / scale,
    )
}

/// Re-cap the lens to the work area of the monitor it is on now (LENS-03) when that cap differs
/// from `cap`, the one last set: it may have been dragged onto another monitor, or the monitor's
/// scale or taskbar changed. A lens larger than the new cap shrinks to it.
pub fn follow_monitor_cap(window: &WebviewWindow, cap: &Mutex<Option<(f64, f64)>>) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let size = max_size(&monitor);
    let changed = {
        let mut cap = cap.lock().unwrap_or_else(PoisonError::into_inner);
        cap.replace(size) != Some(size)
    };
    // Not under the lock: both calls resize the window, which comes back here.
    if changed && let Err(e) = window.set_max_size(Some(LogicalSize::new(size.0, size.1))) {
        tracing::warn!(error = %e, "could not cap the lens to its monitor");
    }
    // Windows applies the cap to resizing by hand only, so shrink a lens that is too large now:
    // once the window has the monitor's scale, else the DPI change would rescale it after us.
    let (Ok(inner), Ok(scale)) = (window.inner_size(), window.scale_factor()) else {
        return;
    };
    if (scale - monitor.scale_factor()).abs() > 1e-6 {
        return;
    }
    let now: LogicalSize<f64> = inner.to_logical(scale);
    if now.width > size.0 + 0.5 || now.height > size.1 + 0.5 {
        let fitted = LogicalSize::new(now.width.min(size.0), now.height.min(size.1));
        if let Err(e) = window.set_size(fitted) {
            tracing::warn!(error = %e, "could not fit the lens to its monitor");
        }
    }
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
    // attribute and fails cleanly on older Windows. The desktop shell's only other `unsafe` calls
    // read the key and mouse-button state (`hold.rs`).
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
        // Panel mode's minimum is the same lens plus the strip.
        let (w, h) = (MIN_SIZE.0 as u32, (MIN_SIZE.1 + LAYOUT.panel) as u32);
        for strip in [Strip::Below, Strip::Above] {
            let p = interior_from_physical(0, 0, w, h, 1.0, strip).unwrap();
            assert_eq!(
                (f64::from(p.width), f64::from(p.height)),
                (i.width, i.height)
            );
        }
    }

    #[test]
    fn cap04_interior_region_at_fractional_scale() {
        // 480×200 logical at 150% on a monitor whose origin is left of the primary one.
        let r = interior_from_physical(-1920, 300, 720, 300, 1.5, Strip::None).unwrap();
        assert_eq!(r, PhysRect::new(-1911, 345, 702, 246));
        // Covers the whole logical interior even when it lands between pixels (125%).
        let r = interior_from_physical(101, 0, 600, 250, 1.25, Strip::None).unwrap();
        assert_eq!(r.x, 108); // floor((101/1.25 + 6) * 1.25) = floor(108.5)
        assert!(r.right() >= 101 + 600 - 7);
    }

    #[test]
    fn no_interior_when_minimised_or_tiny() {
        assert_eq!(interior_from_physical(0, 0, 0, 0, 1.0, Strip::None), None);
        assert_eq!(interior_from_physical(0, 0, 10, 30, 1.0, Strip::None), None);
        // A Panel mode window no taller than its strip.
        assert_eq!(
            interior_from_physical(0, 0, 480, 150, 1.0, Strip::Below),
            None
        );
    }

    /// MODE-02: the strip is outside the interior, so it is neither captured nor click-through,
    /// and adding it, removing it or flipping it leaves the interior (and the pipeline) alone, at
    /// every scale this machine and SPEC's fixtures use.
    #[test]
    fn mode02_the_strip_never_moves_the_interior() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let lens = PhysRect::new(-1500, 400, (480.0 * scale) as u32, (200.0 * scale) as u32);
            let panel = panel_px(scale);
            assert_eq!(
                f64::from(panel),
                LAYOUT.panel * scale,
                "whole px at {scale}"
            );
            let interior = |strip| {
                let w = window_for(lens, strip, panel);
                interior_from_physical(w.x, w.y, w.width, w.height, scale, strip)
            };
            let overlay = interior(Strip::None).unwrap();
            assert_eq!(interior(Strip::Below), Some(overlay), "{scale}");
            assert_eq!(interior(Strip::Above), Some(overlay), "{scale}");
            for strip in [Strip::None, Strip::Below, Strip::Above] {
                assert_eq!(lens_for(window_for(lens, strip, panel), strip, panel), lens);
            }
        }
    }

    #[test]
    fn mode02_panel_flips_above_without_room_below() {
        // 2560×1440 primary with a 48 px taskbar: work area 2560×1392. Lens 480×200, panel 120.
        let area = PhysRect::new(0, 0, 2560, 1392);
        let at = |y| PhysRect::new(1000, y, 480, 200);
        assert_eq!(panel_side(at(600), area, 120), Strip::Below);
        // Exactly room below: still below.
        assert_eq!(panel_side(at(1072), area, 120), Strip::Below);
        assert_eq!(panel_side(at(1073), area, 120), Strip::Above);
        // Dragged partly off the bottom edge.
        assert_eq!(panel_side(at(1300), area, 120), Strip::Above);
        // Room on neither side (a lens almost as tall as the work area): the larger side.
        let tall = |y| PhysRect::new(0, y, 480, 1300);
        assert_eq!(panel_side(tall(30), area, 120), Strip::Below);
        assert_eq!(panel_side(tall(62), area, 120), Strip::Above);
        // A monitor at negative coordinates (the 150% one here, above-left).
        let upper = PhysRect::new(-2160, -2398, 2160, 3780);
        assert_eq!(
            panel_side(PhysRect::new(-1500, -2398, 720, 300), upper, 180),
            Strip::Below
        );
        assert_eq!(
            panel_side(PhysRect::new(-1500, 1200, 720, 300), upper, 180),
            Strip::Above
        );
    }

    #[test]
    fn mode02_mode_is_saved_as_a_word() {
        assert_eq!(
            serde_json::to_string(&DisplayMode::Panel).unwrap(),
            "\"panel\""
        );
        assert_eq!(serde_json::to_string(&Strip::Above).unwrap(), "\"above\"");
    }
}

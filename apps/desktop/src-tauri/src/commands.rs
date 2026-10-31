//! The only commands the lens webview can call (SEC-01). Each is also listed in `build.rs` and
//! allowed per window in `capabilities/lens.json`. All input is validated here, in Rust.

use serde::Serialize;
use tauri::{State, WebviewWindow};

use crate::lens::{self, LAYOUT, Layout};
use crate::state::Lens;

type CmdResult<T> = Result<T, String>;

/// Arrow-key steps in logical px (LENS-02: 1 px, Shift: 10 px).
const NUDGE_STEPS: [i32; 5] = [-10, -1, 0, 1, 10];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LensConfig {
    layout: Layout,
    demo: bool,
    click_through: bool,
    /// False until a real capturer exists (M1-CAP-1) unless `--demo` is on.
    pipeline: bool,
}

#[tauri::command]
pub fn lens_config(window: WebviewWindow, state: State<'_, Lens>) -> CmdResult<LensConfig> {
    ensure_lens(&window)?;
    Ok(LensConfig {
        layout: LAYOUT,
        demo: state.demo,
        click_through: state.click.is_enabled(),
        pipeline: state.has_pipeline() || state.demo,
    })
}

#[tauri::command]
pub fn nudge_lens(window: WebviewWindow, dx: i32, dy: i32) -> CmdResult<()> {
    ensure_lens(&window)?;
    validate_nudge(dx, dy)?;
    lens::nudge(&window, dx, dy).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_click_through(
    window: WebviewWindow,
    state: State<'_, Lens>,
    enabled: bool,
) -> CmdResult<()> {
    ensure_lens(&window)?;
    state.click.set_enabled(enabled);
    Ok(())
}

#[tauri::command]
pub fn set_paused(window: WebviewWindow, state: State<'_, Lens>, paused: bool) -> CmdResult<()> {
    ensure_lens(&window)?;
    state.set_paused(paused);
    Ok(())
}

/// Hide to the tray (the toolbar's close button; quitting is in the tray menu).
#[tauri::command]
pub fn hide_lens(window: WebviewWindow, state: State<'_, Lens>) -> CmdResult<()> {
    ensure_lens(&window)?;
    window.hide().map_err(|e| e.to_string())?;
    state.set_visible(false);
    Ok(())
}

fn ensure_lens(window: &WebviewWindow) -> CmdResult<()> {
    if window.label() == lens::LABEL {
        Ok(())
    } else {
        Err("command is only available to the lens window".into())
    }
}

fn validate_nudge(dx: i32, dy: i32) -> CmdResult<()> {
    if !NUDGE_STEPS.contains(&dx) || !NUDGE_STEPS.contains(&dy) || (dx == 0 && dy == 0) {
        return Err(format!("invalid nudge ({dx}, {dy})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec01_nudge_accepts_only_arrow_key_steps() {
        for (dx, dy) in [(1, 0), (-1, 0), (0, 10), (0, -10), (10, -1)] {
            assert!(validate_nudge(dx, dy).is_ok(), "({dx}, {dy})");
        }
        for (dx, dy) in [(0, 0), (2, 0), (0, -11), (i32::MAX, 0), (i32::MIN, 1)] {
            assert!(validate_nudge(dx, dy).is_err(), "({dx}, {dy})");
        }
    }
}

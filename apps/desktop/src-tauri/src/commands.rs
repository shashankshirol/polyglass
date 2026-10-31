//! The only commands the lens webview can call (SEC-01). Each is also listed in `build.rs` and
//! allowed per window in `capabilities/lens.json`. All input is validated here, in Rust.

use serde::Serialize;
use tauri::{State, WebviewWindow};

use crate::lens::{self, LAYOUT, Layout};
use crate::state::{Lens, Mode};

type CmdResult<T> = Result<T, String>;

/// Arrow-key steps in logical px (LENS-02: 1 px, Shift: 10 px).
const NUDGE_STEPS: [i32; 5] = [-10, -1, 0, 1, 10];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LensConfig {
    layout: Layout,
    mode: Mode,
    click_through: bool,
    /// Whether a pipeline runs (or is starting) behind the lens.
    pipeline: bool,
    /// Fluent message ID if the pipeline failed to start.
    pipeline_error: Option<&'static str>,
}

#[tauri::command]
pub fn lens_config(window: WebviewWindow, state: State<'_, Lens>) -> CmdResult<LensConfig> {
    ensure_lens(&window)?;
    Ok(LensConfig {
        layout: LAYOUT,
        mode: state.mode,
        click_through: state.click.is_enabled(),
        pipeline: state.has_pipeline() || state.mode != Mode::None,
        pipeline_error: state.pipeline_error(),
    })
}

/// Open the Windows settings page where the Swedish OCR feature is added (SV-02). The target is
/// fixed; nothing from the webview reaches the process arguments.
#[tauri::command]
pub fn open_ocr_language_settings(window: WebviewWindow) -> CmdResult<()> {
    ensure_lens(&window)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg("ms-settings:regionlanguage")
            .spawn()
            .map(drop)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        Err("only available on Windows".into())
    }
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

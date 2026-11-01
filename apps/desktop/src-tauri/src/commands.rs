//! The only commands the webviews can call (SEC-01). Each is also listed in `build.rs` and
//! allowed per window in `capabilities/lens.json` or `capabilities/models.json`, and checks the
//! calling window itself. All input is validated here, in Rust.

use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::clipboard;
use crate::hotkeys;
use crate::lens::{self, LAYOUT, Layout};
use crate::models::{self, Models};
use crate::settings;
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
    /// Bytes of Swedish ↔ English models still to download (0 = installed).
    models_missing_bytes: u64,
    /// `false` if a missing model is import-only (no download URLs in the manifest).
    models_downloadable: bool,
    /// The language translations are written in: `en` or `sv` (SET-01).
    target: String,
    /// `--trace-timings`: report each drawn update with `lens_drawn`.
    trace_timings: bool,
    /// `--debug-dump-frames`: where frames are saved; the lens shows a red banner (PRIV-02).
    dump_frames: Option<String>,
}

#[tauri::command]
pub fn lens_config(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Lens>,
    models: State<'_, Models>,
) -> CmdResult<LensConfig> {
    ensure_lens(&window)?;
    Ok(LensConfig {
        layout: LAYOUT,
        mode: state.mode,
        click_through: state.click.is_enabled(),
        pipeline: state.has_pipeline() || state.mode != Mode::None,
        pipeline_error: state.pipeline_error(),
        models_missing_bytes: models.missing_bytes(),
        models_downloadable: models.missing_downloadable(),
        target: app
            .state::<settings::Store>()
            .get()
            .translation
            .target_lang()
            .to_string(),
        trace_timings: state.trace.is_some(),
        dump_frames: app
            .state::<crate::DumpDir>()
            .0
            .as_ref()
            .map(|d| d.display().to_string()),
    })
}

/// `--trace-timings`: the lens has painted the update of `generation`. Ignored otherwise.
#[tauri::command]
pub fn lens_drawn(window: WebviewWindow, state: State<'_, Lens>, generation: u64) -> CmdResult<()> {
    ensure_lens(&window)?;
    if let Some(t) = &state.trace {
        t.drawn(generation, std::time::Instant::now());
    }
    Ok(())
}

/// Open the model manager (the toolbar's ⬇ and ⚙). `async` because a window built inside a
/// synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_model_manager(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    ensure_lens(&window)?;
    models::open_window(&app).map_err(|e| e.to_string())
}

/// The catalog, what is installed, disk usage and any running download (MDL-01, MDL-05).
#[tauri::command]
pub fn models_state(
    window: WebviewWindow,
    models: State<'_, Models>,
) -> CmdResult<models::ModelsView> {
    ensure_window(&window, models::WINDOW)?;
    Ok(models.view())
}

/// Download the listed models from the manifest URLs (MDL-03). `ids` must name manifest
/// entries (`Models::resolve`); progress arrives as `models-changed` events.
#[tauri::command]
pub fn download_models(window: WebviewWindow, app: AppHandle, ids: Vec<String>) -> CmdResult<()> {
    ensure_window(&window, models::WINDOW)?;
    models::start_download(&app, &ids)
}

/// Stop the running download; it resumes from where it stopped next time (MDL-03). `false`
/// when nothing was running.
#[tauri::command]
pub fn cancel_download(window: WebviewWindow, models: State<'_, Models>) -> CmdResult<bool> {
    ensure_window(&window, models::WINDOW)?;
    Ok(models.cancel())
}

/// Delete one installed or partly downloaded model (MDL-05). `id` must name a manifest entry
/// that isn't being downloaded.
#[tauri::command]
pub fn delete_model(window: WebviewWindow, app: AppHandle, id: String) -> CmdResult<()> {
    ensure_window(&window, models::WINDOW)?;
    models::delete(&app, &id)
}

/// The wizard's "Not now" and "Done": don't offer it again, and close the window (SV-01).
#[tauri::command]
pub fn finish_first_run(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    ensure_window(&window, models::WINDOW)?;
    models::finish_first_run(&app);
    window.close().map_err(|e| e.to_string())
}

/// Copy the text the lens shows, in reading order, to the clipboard (REN-05). Takes no input
/// from the webview; `false` when there is nothing to copy.
#[tauri::command]
pub fn copy_translation(window: WebviewWindow, state: State<'_, Lens>) -> CmdResult<bool> {
    ensure_lens(&window)?;
    let text = state.copy_text();
    if text.is_empty() {
        return Ok(false);
    }
    clipboard::copy(&text)?;
    Ok(true)
}

/// Swap the translation direction (HK-01's Ctrl+Alt+S, the toolbar ⇄). Returns the new target.
#[tauri::command]
pub fn swap_languages(window: WebviewWindow, app: AppHandle) -> CmdResult<String> {
    ensure_lens(&window)?;
    Ok(hotkeys::swap(&app).to_string())
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
    app: AppHandle,
    state: State<'_, Lens>,
    enabled: bool,
) -> CmdResult<()> {
    ensure_lens(&window)?;
    state.click.set_enabled(enabled);
    app.state::<settings::Store>()
        .update(|s| s.lens.click_through = enabled);
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
    settings::remember_lens(window.app_handle(), &window);
    window.hide().map_err(|e| e.to_string())?;
    state.set_visible(false);
    Ok(())
}

fn ensure_lens(window: &WebviewWindow) -> CmdResult<()> {
    ensure_window(window, lens::LABEL)
}

/// The capability files already limit each command to its window; this checks it again here.
fn ensure_window(window: &WebviewWindow, label: &str) -> CmdResult<()> {
    if window.label() == label {
        Ok(())
    } else {
        Err(format!("command is only available to the {label} window"))
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

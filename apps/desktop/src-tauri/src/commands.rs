//! The only commands the webviews can call (SEC-01). Each is also listed in `build.rs` and
//! allowed per window in `capabilities/lens.json`, `models.json` or `freeze.json`, and checks the
//! calling window itself. All input is validated here, in Rust.

use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::appearance::Appearance;
use crate::clipboard;
use crate::freeze;
use crate::hotkeys::{self, Action, Binding, HotkeysView, Problem};
use crate::langs::{self, LanguagesView};
use crate::lens::{self, DisplayMode, LAYOUT, Layout, Strip};
use crate::models::{self, Models};
use crate::prefs::{self, SettingsView};
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
    /// The Panel mode strip, if any (MODE-02); `lens-strip` events carry later changes.
    strip: Strip,
    click_through: bool,
    /// Whether a pipeline runs (or is starting) behind the lens.
    pipeline: bool,
    /// Message ID if the pipeline failed to start.
    pipeline_error: Option<&'static str>,
    /// Bytes of the selected pair's models still to download (0 = installed).
    models_missing_bytes: u64,
    /// `false` if a missing model is import-only (no download URLs in the manifest).
    models_downloadable: bool,
    /// How to install the models when they can't be downloaded (MDL-04).
    install_command: String,
    /// The language pair and what the pickers offer (MDL-07, SET-01).
    languages: LanguagesView,
    /// `--trace-timings`: report each drawn update with `lens_drawn`.
    trace_timings: bool,
    /// `--debug-dump-frames`: where frames are saved; the lens shows a red banner (PRIV-02).
    dump_frames: Option<String>,
    /// Frame colour, opacity, font (LENS-07); `lens-appearance` events carry later changes.
    appearance: Appearance,
    /// The hotkeys for the toolbar's tooltips (HK-02); `hotkeys-changed` events carry changes.
    hotkeys: HotkeysView,
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
        strip: state.strip(),
        click_through: state.click.is_enabled(),
        pipeline: state.has_pipeline() || state.mode != Mode::None,
        pipeline_error: state.pipeline_error(),
        models_missing_bytes: models.missing_bytes(),
        models_downloadable: models.missing_downloadable(),
        install_command: models.install_command(),
        languages: langs::view(&app),
        trace_timings: state.trace.is_some(),
        dump_frames: app
            .state::<crate::DumpDir>()
            .0
            .as_ref()
            .map(|d| d.display().to_string()),
        appearance: app.state::<settings::Store>().get().appearance,
        hotkeys: app.state::<hotkeys::Hotkeys>().view(),
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

/// Open the model manager (the toolbar's ⬇, the Settings window's "Translation models…").
/// `async` because a window built inside a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_model_manager(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    if window.label() != prefs::WINDOW {
        ensure_lens(&window)?;
    }
    models::open_window(&app).map_err(|e| e.to_string())
}

/// Open the Settings window (the toolbar's ⚙). `async` like `open_model_manager`.
#[tauri::command]
pub async fn open_settings(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    ensure_lens(&window)?;
    prefs::open_window(&app).map_err(|e| e.to_string())
}

/// What the Settings window shows.
#[tauri::command]
pub fn settings_state(window: WebviewWindow, app: AppHandle) -> CmdResult<SettingsView> {
    ensure_window(&window, prefs::WINDOW)?;
    Ok(prefs::view(&app))
}

/// Change the lens's look (LENS-07). Values out of range are rejected; the lens redraws at once.
#[tauri::command]
pub fn set_appearance(
    window: WebviewWindow,
    app: AppHandle,
    appearance: Appearance,
) -> CmdResult<Appearance> {
    ensure_window(&window, prefs::WINDOW)?;
    prefs::set_appearance(&app, appearance)
}

#[derive(Debug, Serialize)]
pub struct HotkeyOutcome {
    hotkeys: HotkeysView,
    /// Why the binding was refused; the old one stays.
    problem: Option<Problem>,
}

/// Rebind one hotkey (HK-02): keys the recorder saw, the default, or off. Refused with the
/// reason when the keys are another action's, type an AltGr character, or are held elsewhere.
/// `async`: registering runs on the main thread and waits for it.
#[tauri::command]
pub async fn set_hotkey(
    window: WebviewWindow,
    app: AppHandle,
    action: Action,
    binding: Binding,
) -> CmdResult<HotkeyOutcome> {
    ensure_window(&window, prefs::WINDOW)?;
    Ok(match hotkeys::set(&app, action, &binding) {
        Ok(view) => HotkeyOutcome {
            hotkeys: view,
            problem: None,
        },
        Err(problem) => HotkeyOutcome {
            hotkeys: app.state::<hotkeys::Hotkeys>().view(),
            problem: Some(problem),
        },
    })
}

/// The Settings window records keys: unregister the hotkeys so they reach it, or register them
/// again. `async` like `set_hotkey`.
#[tauri::command]
pub async fn record_hotkeys(
    window: WebviewWindow,
    app: AppHandle,
    on: bool,
) -> CmdResult<HotkeysView> {
    ensure_window(&window, prefs::WINDOW)?;
    Ok(hotkeys::set_recording(&app, on))
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

/// "Import from folder" (MDL-04): install the models found in `path` (an absolute folder, or a
/// file in one), verified against the manifest like a download, and load them. `async`: copying
/// and hashing runs off the main thread.
#[tauri::command]
pub async fn import_models(
    window: WebviewWindow,
    app: AppHandle,
    path: String,
) -> CmdResult<models::ImportOutcome> {
    ensure_window(&window, models::WINDOW)?;
    models::import(&app, &path).await
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

/// Swap the translation direction (HK-01's Ctrl+Alt+S, the toolbar ⇄). Returns what the
/// pickers show; an error when the catalog has no model for the other direction.
#[tauri::command]
pub fn swap_languages(window: WebviewWindow, app: AppHandle) -> CmdResult<LanguagesView> {
    ensure_lens(&window)?;
    langs::swap(&app).ok_or_else(|| "no model for the other direction".into())
}

/// Use another language pair (the toolbar's pickers, MDL-07). `source` → `target` must be a pair
/// the model catalog lists; `auto` identifies each block as one of the two (SV-04).
#[tauri::command]
pub fn set_languages(
    window: WebviewWindow,
    app: AppHandle,
    models: State<'_, Models>,
    source: String,
    target: String,
    auto: bool,
) -> CmdResult<LanguagesView> {
    ensure_lens(&window)?;
    let choice = models.languages().validate(&source, &target, auto)?;
    Ok(langs::apply(&app, &choice))
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

/// Switch between Overlay and Panel mode (the toolbar's mode button, MODE-01/02) and save it
/// (LENS-01). The lens learns the new layout from a `lens-strip` event.
#[tauri::command]
pub fn set_mode(window: WebviewWindow, app: AppHandle, mode: DisplayMode) -> CmdResult<()> {
    ensure_lens(&window)?;
    let w = window.clone();
    // `apply_mode` needs the main thread (see there).
    app.run_on_main_thread(move || {
        if let Err(e) = lens::apply_mode(&w, mode) {
            tracing::warn!(error = %e, ?mode, "could not change the lens mode");
        }
    })
    .map_err(|e| e.to_string())?;
    app.state::<settings::Store>()
        .update(|s| s.lens.mode = mode);
    Ok(())
}

#[tauri::command]
pub fn set_paused(window: WebviewWindow, state: State<'_, Lens>, paused: bool) -> CmdResult<()> {
    ensure_lens(&window)?;
    state.set_paused(paused);
    Ok(())
}

/// Enter or leave Freeze mode (MODE-03): the lens's ❄ and Esc, and Esc in the snapshot window.
/// `async`, so leaving doesn't destroy the calling window from inside its own request.
#[tauri::command]
pub async fn set_frozen(window: WebviewWindow, app: AppHandle, frozen: bool) -> CmdResult<()> {
    if window.label() != freeze::WINDOW {
        ensure_lens(&window)?;
    }
    if frozen {
        freeze::enter(&app);
    } else {
        freeze::exit(&app);
    }
    Ok(())
}

/// The snapshot for the freeze window's canvas (`freeze::rgba`), as raw bytes. Memory only:
/// the reply goes straight to that window (PRIV-02).
#[tauri::command]
pub fn freeze_snapshot(window: WebviewWindow, state: State<'_, Lens>) -> CmdResult<Response> {
    ensure_window(&window, freeze::WINDOW)?;
    let snapshot = state.freeze.snapshot().ok_or("not frozen")?;
    Ok(Response::new(freeze::rgba(&snapshot)))
}

/// The freeze window has drawn the snapshot: show it under the lens.
#[tauri::command]
pub fn freeze_ready(window: WebviewWindow, app: AppHandle) -> CmdResult<()> {
    ensure_window(&window, freeze::WINDOW)?;
    freeze::shown(&app, &window).map_err(|e| e.to_string())
}

/// Hide to the tray (the toolbar's close button; quitting is in the tray menu).
#[tauri::command]
pub fn hide_lens(window: WebviewWindow, state: State<'_, Lens>) -> CmdResult<()> {
    ensure_lens(&window)?;
    freeze::exit(window.app_handle());
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

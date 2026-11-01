//! Polyglass desktop shell (PLAN §3). A thin Tauri layer over the headless `pg-*` crates: it
//! owns the lens window, turns window events into pipeline commands, and forwards
//! `LensUpdate`s to the webview.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clickthrough;
mod clipboard;
mod commands;
mod demo;
mod devflags;
mod hold;
mod hotkeys;
mod lens;
mod live;
mod models;
mod settings;
mod state;
mod tray;

use std::sync::Arc;

use pg_core::PhysRect;
use pg_models::ModelStore;
use pg_pipeline::{LensUpdate, Pipeline, PipelineConfig, SchedulerConfig, driver};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewWindow, WindowEvent};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::devflags::{DevFlags, Trace};
use crate::models::Models;
use crate::state::{Lens, Mode};

/// Event the lens UI listens to for pipeline results.
const UPDATE_EVENT: &str = "lens-update";
/// Event carrying a Fluent message ID when the pipeline can't start.
const ERROR_EVENT: &str = "lens-error";

/// Where `--debug-dump-frames` saves frames, resolved once the app's folders are known.
#[derive(Debug, Default)]
pub struct DumpDir(pub Option<std::path::PathBuf>);

fn main() {
    let flags = DevFlags::parse(std::env::args());
    let mut filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if flags.trace_timings {
        filter = filter.add_directive("pg::trace=info".parse().expect("valid directive"));
    }
    tracing_subscriber::fmt().with_env_filter(filter).init();
    let trace = flags.trace_timings.then(|| {
        Arc::new(Trace::new(
            &SchedulerConfig::default(),
            flags.trace_uncached,
        ))
    });

    let demo = std::env::args().any(|a| a == "--demo")
        || std::env::var("POLYGLASS_DEMO").is_ok_and(|v| v == "1");
    let mode = if demo {
        Mode::Demo
    } else if cfg!(windows) {
        Mode::Live
    } else {
        Mode::None
    };

    let app = tauri::Builder::default()
        .manage(Lens::new(mode).with_trace(trace))
        .plugin(hotkeys::plugin())
        .invoke_handler(tauri::generate_handler![
            commands::lens_config,
            commands::nudge_lens,
            commands::set_click_through,
            commands::set_paused,
            commands::hide_lens,
            commands::open_ocr_language_settings,
            commands::open_model_manager,
            commands::copy_translation,
            commands::swap_languages,
            commands::models_state,
            commands::download_models,
            commands::cancel_download,
            commands::delete_model,
            commands::finish_first_run,
            commands::lens_drawn,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let data = handle.path().app_local_data_dir()?;
            // PRIV-02: the one way frames reach the disk; the lens shows a red banner meanwhile.
            let dump = flags.dump_frames.clone().map(|dir| {
                dir.unwrap_or_else(|| data.join("debug-frames"))
                    .join(devflags::utc_stamp(std::time::SystemTime::now()))
            });
            handle.manage(DumpDir(dump.clone()));
            let store = ModelStore::new(data.join("models"));
            handle.manage(Models::new(store.clone()));
            let settings =
                settings::Store::open(handle.path().app_config_dir()?.join(settings::FILE));
            let saved = settings.get();
            handle.manage(settings);
            let window = lens::create(&handle)?;
            // Before the lens is first shown, so no capture ever contains it (CAP-02).
            let self_excluded = lens::exclude_from_capture(&window);
            watch_window(&window);
            tray::create(&handle)?;
            lens::place_and_show(&handle, &window, &saved.lens)?;
            let state = handle.state::<Lens>();
            state.click.set_enabled(saved.lens.click_through);
            state.set_region(lens::interior_region(&window));
            state.set_visible(true);
            state.click.spawn(handle.clone(), window.clone());
            state.hold.spawn(handle.clone());
            hotkeys::register(&handle);
            // MDL-06, SV-01: offer the Swedish models once; nothing downloads without a click.
            let models = handle.state::<Models>();
            if models::offer_first_run(mode, saved.models.first_run_done, models.missing_bytes()) {
                models.set_first_run(true);
                models::open_window(&handle)?;
            }
            match mode {
                Mode::Demo => {
                    tracing::warn!("demo mode: mock capture, OCR and translation");
                    let scale = window.scale_factor().unwrap_or(1.0);
                    let bounds = desktop_bounds(&handle).unwrap_or(PhysRect::new(0, 0, 1920, 1080));
                    start_pipeline(handle, move || demo::pipeline(bounds, scale, dump));
                }
                // Without exclusion the lens would OCR its own translations (CAP-02).
                Mode::Live if !self_excluded => {
                    report_pipeline_error(&handle, live::SELF_EXCLUSION_ERROR);
                }
                Mode::Live => {
                    tracing::info!("live capture, OCR and on-device translation");
                    start_pipeline(handle, move || live::pipeline(&store, dump));
                }
                Mode::None => tracing::warn!("no capture backend on this platform yet"),
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri app");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(w) = lens::get(app)
                && w.is_visible().unwrap_or(false)
            {
                settings::remember_lens(app, &w);
            }
            let state = app.state::<Lens>();
            if let Some(t) = &state.trace {
                t.log_summary();
            }
            tracing::info!(clickthrough_toggles = state.click.toggles(), "exiting");
            state.shutdown();
        }
    });
}

/// Feed move/resize/show/hide into the shared state (and from there into the driver).
fn watch_window(window: &WebviewWindow) {
    let w = window.clone();
    window.on_window_event(move |event| {
        let state = w.state::<Lens>();
        match event {
            WindowEvent::Moved(_)
            | WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. } => {
                state.set_region(lens::interior_region(&w));
            }
            WindowEvent::CloseRequested { api, .. } => {
                // Alt+F4 hides to the tray; Quit lives in the tray menu.
                api.prevent_close();
                settings::remember_lens(w.app_handle(), &w);
                if w.hide().is_ok() {
                    state.set_visible(false);
                }
            }
            _ => {}
        }
    });
}

/// Show or hide the lens from the tray or the hotkey (LENS-06, HK-01).
pub fn toggle_lens(app: &AppHandle) {
    let Some(window) = lens::get(app) else { return };
    let state = app.state::<Lens>();
    if state.is_visible() {
        settings::remember_lens(app, &window);
        if window.hide().is_ok() {
            state.set_visible(false);
        }
    } else if window.show().is_ok() {
        let _ = window.set_focus();
        state.set_region(lens::interior_region(&window));
        state.set_visible(true);
    }
}

/// Build the pipeline off the UI thread (painting the demo desktop and starting WGC/OCR take a
/// moment), then run the driver and forward its updates to the lens.
fn start_pipeline(
    app: AppHandle,
    build: impl FnOnce() -> pg_core::Result<Pipeline> + Send + 'static,
) {
    tauri::async_runtime::spawn(async move {
        let pipeline = match tokio::task::spawn_blocking(build).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                // Technical context only, never recognized text (PRIV-07).
                tracing::error!(error = %e, "pipeline failed to start");
                report_pipeline_error(&app, "cap-error");
                return;
            }
            Err(_) => {
                tracing::error!("pipeline setup panicked");
                return;
            }
        };
        // SET-01: translate into the saved target language.
        let target = app
            .state::<settings::Store>()
            .get()
            .translation
            .target_lang();
        pipeline.set_config(PipelineConfig {
            target,
            ..pipeline.config()
        });
        let pipeline = Arc::new(pipeline);
        app.state::<Lens>().set_pipeline(pipeline.clone());
        let (tx, mut rx) = mpsc::unbounded_channel::<LensUpdate>();
        let (handle, _task) = driver::spawn(pipeline, SchedulerConfig::default(), tx);
        app.state::<Lens>().set_driver(handle);
        while let Some(update) = rx.recv().await {
            app.state::<Lens>().set_last_update(&update);
            if let Some(t) = &app.state::<Lens>().trace {
                t.sent(&update, std::time::Instant::now());
            }
            // LensUpdate's Debug is text-free, but don't log it at all (PRIV-07).
            if app.emit_to(lens::LABEL, UPDATE_EVENT, &update).is_err() {
                break;
            }
        }
    });
}

/// Tell the lens why nothing runs behind it. `lens_config` carries the same ID in case the UI
/// subscribes after this event.
fn report_pipeline_error(app: &AppHandle, message_id: &'static str) {
    tracing::error!(message_id, "no pipeline behind the lens");
    app.state::<Lens>().set_pipeline_error(message_id);
    let _ = app.emit_to(lens::LABEL, ERROR_EVENT, message_id);
}

/// Bounding box of all monitors in physical px.
fn desktop_bounds(app: &AppHandle) -> Option<PhysRect> {
    let monitors = app.available_monitors().ok()?;
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for m in &monitors {
        let (p, s) = (m.position(), m.size());
        x0 = x0.min(i64::from(p.x));
        y0 = y0.min(i64::from(p.y));
        x1 = x1.max(i64::from(p.x) + i64::from(s.width));
        y1 = y1.max(i64::from(p.y) + i64::from(s.height));
    }
    (x1 > x0 && y1 > y0)
        .then(|| PhysRect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32))
}

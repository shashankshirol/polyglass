//! Polyglass desktop shell (PLAN §3). A thin Tauri layer over the headless `pg-*` crates: it
//! owns the lens window, turns window events into pipeline commands, and forwards
//! `LensUpdate`s to the webview.

// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clickthrough;
mod commands;
mod demo;
mod lens;
mod state;
mod tray;

use std::sync::Arc;

use pg_core::PhysRect;
use pg_pipeline::{LensUpdate, SchedulerConfig, driver};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewWindow, WindowEvent};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::state::Lens;

/// Event the lens UI listens to for pipeline results.
const UPDATE_EVENT: &str = "lens-update";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let demo = std::env::args().any(|a| a == "--demo")
        || std::env::var("POLYGLASS_DEMO").is_ok_and(|v| v == "1");

    let app = tauri::Builder::default()
        .manage(Lens::new(demo))
        .invoke_handler(tauri::generate_handler![
            commands::lens_config,
            commands::nudge_lens,
            commands::set_click_through,
            commands::set_paused,
            commands::hide_lens,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let window = lens::create(&handle)?;
            watch_window(&window);
            tray::create(&handle)?;
            lens::place_and_show(&handle, &window)?;
            let state = handle.state::<Lens>();
            state.set_region(lens::interior_region(&window));
            state.set_visible(true);
            state.click.spawn(handle.clone(), window.clone());
            if demo {
                tracing::warn!("demo mode: mock capture, OCR and translation");
                start_demo_pipeline(handle, &window);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Tauri app");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let state = app.state::<Lens>();
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
                if w.hide().is_ok() {
                    state.set_visible(false);
                }
            }
            _ => {}
        }
    });
}

/// Show or hide the lens from the tray (LENS-06; the global hotkey is M1-APP-4/M1-APP-9).
pub fn toggle_lens(app: &AppHandle) {
    let Some(window) = lens::get(app) else { return };
    let state = app.state::<Lens>();
    if state.is_visible() {
        if window.hide().is_ok() {
            state.set_visible(false);
        }
    } else if window.show().is_ok() {
        let _ = window.set_focus();
        state.set_region(lens::interior_region(&window));
        state.set_visible(true);
    }
}

fn start_demo_pipeline(app: AppHandle, window: &WebviewWindow) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let bounds = desktop_bounds(&app).unwrap_or(PhysRect::new(0, 0, 1920, 1080));
    tauri::async_runtime::spawn(async move {
        // Painting the synthetic desktop takes a moment; keep it off the UI thread.
        let Ok(pipeline) = tokio::task::spawn_blocking(move || demo::pipeline(bounds, scale)).await
        else {
            tracing::error!("demo pipeline setup failed");
            return;
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<LensUpdate>();
        let (handle, _task) = driver::spawn(Arc::new(pipeline), SchedulerConfig::default(), tx);
        app.state::<Lens>().set_driver(handle);
        while let Some(update) = rx.recv().await {
            // LensUpdate's Debug is text-free, but don't log it at all (PRIV-07).
            if app.emit_to(lens::LABEL, UPDATE_EVENT, &update).is_err() {
                break;
            }
        }
    });
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

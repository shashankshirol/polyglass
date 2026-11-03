//! Minimal tray icon: show/hide the lens, Settings, the model manager, crash reports if any,
//! and quit (LENS-01, LENS-06, MDL-05, PRIV-06). The lens has no taskbar entry, so this is the
//! way back after hiding it.

use std::path::PathBuf;

use tauri::AppHandle;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

const TOGGLE: &str = "toggle";
const SETTINGS: &str = "settings";
const MODELS: &str = "models";
const CRASHES: &str = "crashes";
const QUIT: &str = "quit";

/// `crashes`: the crash-report folder, when it holds reports from earlier runs.
pub fn create(app: &AppHandle, crashes: Option<PathBuf>) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, TOGGLE, "Show/hide lens", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, SETTINGS, "Settings…", true, None::<&str>)?;
    let models = MenuItem::with_id(app, MODELS, "Translation models…", true, None::<&str>)?;
    let reports = MenuItem::with_id(app, CRASHES, "Crash reports…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit Polyglass", true, None::<&str>)?;
    let menu = if crashes.is_some() {
        Menu::with_items(app, &[&toggle, &settings, &models, &reports, &quit])?
    } else {
        Menu::with_items(app, &[&toggle, &settings, &models, &quit])?
    };

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Polyglass")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            TOGGLE => crate::toggle_lens(app),
            SETTINGS => {
                if let Err(e) = crate::prefs::open_window(app) {
                    tracing::error!(error = %e, "can't open the settings");
                }
            }
            MODELS => {
                if let Err(e) = crate::models::open_window(app) {
                    tracing::error!(error = %e, "can't open the model manager");
                }
            }
            CRASHES => {
                if let Some(dir) = &crashes
                    && let Err(e) = crate::crash::open_folder(dir)
                {
                    tracing::error!(error = %e, "can't open the crash reports folder");
                }
            }
            QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::toggle_lens(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

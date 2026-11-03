//! The Settings window (`ui/settings.html`): appearance (LENS-07) and a way to the model
//! manager. Opened by the toolbar's ⚙ and "Settings…" in the tray. Its commands are limited to
//! it (`capabilities/settings.json`, SEC-01) and every value is checked here before it is saved.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::appearance::{self, Appearance};
use crate::{lens, settings};

pub const WINDOW: &str = "settings";

/// What the Settings window shows.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub appearance: Appearance,
    /// What "Reset" goes back to.
    pub appearance_defaults: Appearance,
}

pub fn view(app: &AppHandle) -> SettingsView {
    SettingsView {
        appearance: app.state::<settings::Store>().get().appearance,
        appearance_defaults: Appearance::default(),
    }
}

/// Save a new appearance and show it in the lens at once.
pub fn set_appearance(app: &AppHandle, a: Appearance) -> Result<Appearance, String> {
    a.validate()?;
    let a = a.clamped();
    app.state::<settings::Store>()
        .update(|s| s.appearance = a.clone());
    let _ = app.emit_to(lens::LABEL, appearance::EVENT, &a);
    Ok(a)
}

/// Open the Settings window, or bring it to the front. Not from a synchronous command (see
/// `models::open_window`).
pub fn open_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(WINDOW) {
        w.show()?;
        w.unminimize()?;
        return w.set_focus();
    }
    let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("settings.html".into()))
        .title("Polyglass – Settings")
        .inner_size(560.0, 640.0)
        .min_inner_size(420.0, 360.0)
        .center()
        .additional_browser_args(lens::WEBVIEW_ARGS)
        .build()?;
    window.set_focus()
}

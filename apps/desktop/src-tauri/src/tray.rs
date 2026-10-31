//! Minimal tray icon: show/hide the lens and quit (LENS-01, LENS-06). The lens has no taskbar
//! entry, so this is the way back after hiding it.

use tauri::AppHandle;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

const TOGGLE: &str = "toggle";
const QUIT: &str = "quit";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, TOGGLE, "Show/hide lens", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, "Quit Polyglass", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Polyglass")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            TOGGLE => crate::toggle_lens(app),
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

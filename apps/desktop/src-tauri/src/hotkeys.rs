//! Default global hotkeys (HK-01) and the actions behind them and the matching toolbar buttons.
//!
//! Registered from Rust through `tauri-plugin-global-shortcut`; the webview gets no permission
//! for the plugin. Ctrl+Alt with L, F, C, S and T produce no character with AltGr on
//! the Swedish layout, where AltGr arrives as Ctrl+Alt, so registering them takes nothing away
//! from typing (SV-09, `sv09_hotkeys_type_nothing_with_altgr_on_the_swedish_layout`). A hotkey
//! another app already holds is logged and skipped; reporting it in the UI is HK-02 (M4-2).

use tauri::{AppHandle, Emitter, Manager, Wry, plugin::TauriPlugin};
use tauri_plugin_global_shortcut::{
    Builder, Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

use crate::settings;
use crate::state::Lens;
use crate::{clipboard, freeze, langs, lens};
/// Event carrying the click-through state after the hotkey toggled it.
pub const CLICK_THROUGH_EVENT: &str = "lens-click-through";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    ShowHide,
    Freeze,
    Copy,
    Swap,
    ClickThrough,
}

/// HK-01 keys, each with the platform's modifiers (`modifiers`).
pub const DEFAULTS: [(Action, Code); 5] = [
    (Action::ShowHide, Code::KeyL),
    (Action::Freeze, Code::KeyF),
    (Action::Copy, Code::KeyC),
    (Action::Swap, Code::KeyS),
    (Action::ClickThrough, Code::KeyT),
];

pub fn modifiers() -> Modifiers {
    Modifiers::CONTROL | Modifiers::ALT
}

fn action_for(shortcut: &Shortcut) -> Option<Action> {
    DEFAULTS
        .iter()
        .find(|(_, code)| shortcut.matches(modifiers(), code))
        .map(|(a, _)| *a)
}

/// The plugin, with one handler for all hotkeys.
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state == ShortcutState::Pressed
                && let Some(action) = action_for(shortcut)
            {
                run(app, action);
            }
        })
        .build()
}

/// Register the defaults. A key held by another app is skipped with a warning.
pub fn register(app: &AppHandle) {
    for (action, code) in DEFAULTS {
        if let Err(e) = app
            .global_shortcut()
            .register(Shortcut::new(Some(modifiers()), code))
        {
            tracing::warn!(?action, error = %e, "hotkey unavailable");
        }
    }
}

fn run(app: &AppHandle, action: Action) {
    match action {
        Action::ShowHide => crate::toggle_lens(app),
        Action::Freeze => freeze::toggle(app),
        Action::Copy => {
            let text = app.state::<Lens>().copy_text();
            if !text.is_empty()
                && let Err(e) = clipboard::copy(&text)
            {
                tracing::warn!(error = %e, "copy failed");
            }
        }
        Action::Swap => {
            if langs::swap(app).is_none() {
                tracing::info!("swap: the catalog has no model for the other direction");
            }
        }
        Action::ClickThrough => {
            let state = app.state::<Lens>();
            let on = !state.click.is_enabled();
            state.click.set_enabled(on);
            app.state::<settings::Store>()
                .update(|s| s.lens.click_through = on);
            let _ = app.emit_to(lens::LABEL, CLICK_THROUGH_EVENT, on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hk01_defaults_map_back_to_their_actions() {
        for (action, code) in DEFAULTS {
            assert_eq!(
                action_for(&Shortcut::new(Some(modifiers()), code)),
                Some(action)
            );
        }
        assert_eq!(
            action_for(&Shortcut::new(Some(Modifiers::CONTROL), Code::KeyL)),
            None
        );
    }

    /// SV-09: on the Swedish layout, AltGr (= Ctrl+Alt) with a hotkey's letter types nothing, so
    /// the hotkeys never swallow a character. A control (AltGr+2 = @, AltGr+E = €) shows that the
    /// check sees AltGr characters. Skips when the Swedish layout isn't installed; it never
    /// installs or activates one.
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code)]
    fn sv09_hotkeys_type_nothing_with_altgr_on_the_swedish_layout() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            GetKeyboardLayoutList, HKL, MAPVK_VK_TO_VSC_EX, MapVirtualKeyExW, ToUnicodeEx,
            VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU,
        };
        // SAFETY (whole test): plain Win32 reads with buffers sized for them. ToUnicodeEx with
        // flag 4 leaves the thread's keyboard state alone (Windows 10 1607+).
        let mut layouts = [HKL::default(); 64];
        let n = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) } as usize;
        // The low word is the language, the high word the layout: 0x041D Swedish.
        let Some(&swedish) = layouts[..n]
            .iter()
            .find(|h| (h.0 as usize & 0xFFFF_FFFF) == 0x041D_041D)
        else {
            eprintln!("skipped: the Swedish keyboard layout isn't installed");
            return;
        };
        let mut keys = [0u8; 256];
        for vk in [VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU] {
            keys[usize::from(vk.0)] = 0x80;
        }
        let altgr = |vk: u16| -> String {
            let scan =
                unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, Some(swedish)) };
            let mut buf = [0u16; 8];
            let n = unsafe { ToUnicodeEx(u32::from(vk), scan, &keys, &mut buf, 4, Some(swedish)) };
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        };
        assert_eq!(altgr(u16::from(b'2')), "@");
        assert_eq!(altgr(u16::from(b'E')), "€");
        for (action, code) in DEFAULTS {
            let letter = match code {
                Code::KeyL => b'L',
                Code::KeyF => b'F',
                Code::KeyC => b'C',
                Code::KeyS => b'S',
                Code::KeyT => b'T',
                _ => unreachable!("a letter hotkey"),
            };
            assert_eq!(altgr(u16::from(letter)), "", "{action:?} types a character");
        }
    }
}

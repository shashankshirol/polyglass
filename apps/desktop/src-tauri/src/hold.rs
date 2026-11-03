//! Hold left Alt to see the original text in place of the translation (REN-04).
//!
//! The lens is click-through and rarely has the keyboard focus, so it can't wait for key events.
//! A background thread reads the key state at about 30 Hz while the lens is visible and tells the
//! webview when the answer changes (`lens-original`). It only reads the state, so other apps get
//! every key as usual. Right Alt is AltGr on the Swedish layout, where it types @ £ $ € { [ ] }
//! and arrives as Ctrl + right Alt, so neither it nor any Ctrl combination counts (SV-09); the
//! Ctrl+Alt hotkeys (HK-01) don't flash the original either.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, Thread};

use tauri::{AppHandle, Emitter};

use crate::clickthrough::POLL_INTERVAL;
use crate::lens;

/// Event carrying `true` while the original should be shown.
pub const EVENT: &str = "lens-original";

/// Modifier keys held right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keys {
    pub left_alt: bool,
    pub right_alt: bool,
    pub ctrl: bool,
    pub win: bool,
}

/// Whether `keys` mean "show the original": left Alt alone (Shift is allowed).
pub fn shows_original(keys: Keys) -> bool {
    keys.left_alt && !keys.right_alt && !keys.ctrl && !keys.win
}

#[derive(Debug)]
pub struct Hold {
    visible: AtomicBool,
    stop: AtomicBool,
    thread: OnceLock<Thread>,
}

impl Hold {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            visible: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            thread: OnceLock::new(),
        })
    }

    pub fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::SeqCst);
        self.wake();
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake();
    }

    fn wake(&self) {
        if let Some(t) = self.thread.get() {
            t.unpark();
        }
    }

    /// Start the poller. Call once. Without a key-state source (not Windows yet) nothing runs.
    pub fn spawn(self: &Arc<Self>, app: AppHandle) {
        if !cfg!(windows) {
            return;
        }
        let state = self.clone();
        let handle = thread::Builder::new()
            .name("pg-hold".into())
            .spawn(move || state.run(&app))
            .expect("spawn hold thread");
        let _ = self.thread.set(handle.thread().clone());
    }

    fn run(&self, app: &AppHandle) {
        let mut showing = false;
        while !self.stop.load(Ordering::SeqCst) {
            let visible = self.visible.load(Ordering::SeqCst);
            let want = visible && shows_original(keys());
            if want != showing && app.emit_to(lens::LABEL, EVENT, want).is_ok() {
                showing = want;
            }
            if visible {
                thread::sleep(POLL_INTERVAL);
            } else {
                // Hidden: no polling until shown again (LENS-06).
                thread::park();
            }
        }
    }
}

#[cfg(windows)]
fn keys() -> Keys {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LWIN, VK_RMENU, VK_RWIN,
    };
    let down = |vk: VIRTUAL_KEY| {
        // SAFETY: GetAsyncKeyState only reads the global key state; any key code is valid.
        #[allow(unsafe_code)]
        let state = unsafe { GetAsyncKeyState(i32::from(vk.0)) };
        state < 0
    };
    Keys {
        left_alt: down(VK_LMENU),
        right_alt: down(VK_RMENU),
        ctrl: down(VK_CONTROL),
        win: down(VK_LWIN) || down(VK_RWIN),
    }
}

/// Whether any mouse button is down (the physical buttons, so left-handed setups count too).
/// The Panel mode strip waits for this before it flips (MODE-02, `lens::follow_panel`).
#[cfg(windows)]
pub fn mouse_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    };
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].into_iter().any(|vk| {
        // SAFETY: as in `keys`: a read of the global key state.
        #[allow(unsafe_code)]
        let state = unsafe { GetAsyncKeyState(i32::from(vk.0)) };
        state < 0
    })
}

#[cfg(not(windows))]
fn keys() -> Keys {
    Keys::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ren04_left_alt_alone_shows_the_original() {
        let alt = Keys {
            left_alt: true,
            ..Keys::default()
        };
        assert!(shows_original(alt));
        assert!(!shows_original(Keys::default()));
    }

    #[test]
    fn sv09_altgr_and_hotkeys_do_not_show_the_original() {
        // AltGr on the Swedish layout: Ctrl + right Alt (typing @ { [ ] } \ and so on).
        let altgr = Keys {
            right_alt: true,
            ctrl: true,
            ..Keys::default()
        };
        assert!(!shows_original(altgr));
        // Ctrl+Alt+L/C/S/T hotkeys (HK-01).
        let hotkey = Keys {
            left_alt: true,
            ctrl: true,
            ..Keys::default()
        };
        assert!(!shows_original(hotkey));
        let win_alt = Keys {
            left_alt: true,
            win: true,
            ..Keys::default()
        };
        assert!(!shows_original(win_alt));
        let right_alt = Keys {
            right_alt: true,
            ..Keys::default()
        };
        assert!(!shows_original(right_alt));
    }
}

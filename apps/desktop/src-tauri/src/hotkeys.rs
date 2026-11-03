//! Global hotkeys: the HK-01 defaults, rebinding them (HK-02) and the actions behind them and
//! the matching toolbar buttons.
//!
//! Registered from Rust through `tauri-plugin-global-shortcut`; the webview gets no permission
//! for the plugin. Ctrl+Alt with L, F, C, S and T produce no character with AltGr on
//! the Swedish layout, where AltGr arrives as Ctrl+Alt, so registering them takes nothing away
//! from typing (SV-09, `sv09_hotkeys_type_nothing_with_altgr_on_the_swedish_layout`).
//!
//! HK-02: the Settings window rebinds each action (`[hotkeys]` in `settings.toml`, "" = off).
//! A new binding is refused with the reason when it uses another action's keys, when it would
//! swallow an AltGr character on an installed keyboard layout (`altgr_character`, SV-09), or when
//! another app or Windows already holds it (registering fails); the old binding then stays. A
//! saved hotkey that can't be registered at start is reported as inactive in Settings.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Wry, plugin::TauriPlugin};
use tauri_plugin_global_shortcut::{
    Builder, Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
};

use crate::settings::{self, HotkeySettings};
use crate::state::Lens;
use crate::{clipboard, freeze, langs, lens};
/// Event carrying the click-through state after the hotkey toggled it.
pub const CLICK_THROUGH_EVENT: &str = "lens-click-through";
/// Event carrying the [`HotkeysView`] after a binding changed (lens tooltips, Settings).
pub const EVENT: &str = "hotkeys-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    ShowHide,
    Freeze,
    Copy,
    Swap,
    ClickThrough,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::ShowHide,
        Action::Freeze,
        Action::Copy,
        Action::Swap,
        Action::ClickThrough,
    ];
}

/// HK-01 keys, each with Ctrl+Alt.
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

pub fn default_keys(action: Action) -> Keys {
    let code = DEFAULTS
        .iter()
        .find(|(a, _)| *a == action)
        .map(|(_, c)| *c)
        .expect("every action has a default");
    Keys {
        mods: modifiers(),
        code,
    }
}

/// A key with its modifiers, written like `Ctrl+Alt+L` in `settings.toml` and the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    pub mods: Modifiers,
    pub code: Code,
}

const MODIFIER_NAMES: [(Modifiers, &str); 4] = [
    (Modifiers::CONTROL, "Ctrl"),
    (Modifiers::ALT, "Alt"),
    (Modifiers::SHIFT, "Shift"),
    (Modifiers::SUPER, "Win"),
];

impl Keys {
    /// From the Settings window's recorder or the file. Only letters, digits and F1–F24, with
    /// Ctrl, Alt or Win (Shift alone would take a capital letter from every app).
    pub fn new(mods: Modifiers, code: Code) -> Result<Self, Problem> {
        if key_name(code).is_none() {
            return Err(Problem::Key);
        }
        if !mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
            return Err(Problem::NoModifier);
        }
        Ok(Self { mods, code })
    }

    pub fn parse(text: &str) -> Result<Self, Problem> {
        let mut mods = Modifiers::empty();
        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let key = parts.pop().ok_or(Problem::Key)?;
        for p in parts {
            let (m, _) = MODIFIER_NAMES
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(p))
                .ok_or(Problem::Key)?;
            mods |= *m;
        }
        let code = ALLOWED_KEYS
            .iter()
            .copied()
            .find(|c| key_name(*c).is_some_and(|n| n.eq_ignore_ascii_case(key)))
            .ok_or(Problem::Key)?;
        Self::new(mods, code)
    }

    pub fn shortcut(self) -> Shortcut {
        Shortcut::new(Some(self.mods), self.code)
    }
}

impl fmt::Display for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (m, name) in MODIFIER_NAMES {
            if self.mods.contains(m) {
                write!(f, "{name}+")?;
            }
        }
        f.write_str(&key_name(self.code).unwrap_or_default())
    }
}

/// Keys a hotkey can use: A–Z, 0–9 (the main row), F1–F24.
const ALLOWED_KEYS: [Code; 60] = {
    use Code::*;
    [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ, Digit0, Digit1, Digit2,
        Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, F1, F2, F3, F4, F5, F6, F7, F8, F9,
        F10, F11, F12, F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    ]
};

/// `L`, `1`, `F5`; `None` for keys a hotkey can't use.
fn key_name(code: Code) -> Option<String> {
    if !ALLOWED_KEYS.contains(&code) {
        return None;
    }
    let s = code.to_string();
    Some(
        s.strip_prefix("Key")
            .or_else(|| s.strip_prefix("Digit"))
            .unwrap_or(&s)
            .to_owned(),
    )
}

/// The Windows virtual-key code of a letter or digit (the same as its ASCII capital).
fn character_vk(code: Code) -> Option<u16> {
    let name = key_name(code)?;
    let b = name.as_bytes();
    (b.len() == 1).then(|| u16::from(b[0]))
}

/// What the Settings window's recorder saw: `KeyboardEvent.code` and the modifier flags.
#[derive(Debug, Clone, Deserialize)]
pub struct Pressed {
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Pressed {
    pub fn keys(&self) -> Result<Keys, Problem> {
        let code: Code = self.code.parse().map_err(|_| Problem::Key)?;
        let mut mods = Modifiers::empty();
        for (on, m) in [
            (self.ctrl, Modifiers::CONTROL),
            (self.alt, Modifiers::ALT),
            (self.shift, Modifiers::SHIFT),
            (self.meta, Modifiers::SUPER),
        ] {
            if on {
                mods |= m;
            }
        }
        Keys::new(mods, code)
    }
}

/// A new binding from the Settings window.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Binding {
    Pressed(Pressed),
    Default,
    Off,
}

/// Why a binding was refused; the UI words it (M4-5 localizes it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    /// Not a letter, digit or F-key.
    Key,
    /// No Ctrl, Alt or Win.
    NoModifier,
    /// Another Polyglass action has these keys.
    Duplicate { action: Action },
    /// Ctrl+Alt (= AltGr) with this key types `character` on the keyboard layout of `layout`
    /// (a BCP 47 tag), so the hotkey would take it away from every app (SV-09).
    AltGr { layout: String, character: String },
    /// Registering failed: another app or Windows holds these keys.
    InUse { keys: String },
}

/// Registers and unregisters hotkeys; the plugin in the app, a fake in tests.
pub trait Registrar {
    fn register(&self, keys: Keys) -> bool;
    fn unregister(&self, keys: Keys);
}

impl Registrar for AppHandle {
    fn register(&self, keys: Keys) -> bool {
        match self.global_shortcut().register(keys.shortcut()) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(%keys, error = %e, "hotkey unavailable");
                false
            }
        }
    }
    fn unregister(&self, keys: Keys) {
        if let Err(e) = self.global_shortcut().unregister(keys.shortcut()) {
            tracing::warn!(%keys, error = %e, "hotkey unregister failed");
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HotkeyRow {
    pub action: Action,
    /// `None`: turned off.
    pub keys: Option<String>,
    pub default: String,
    /// `false` when registering it failed (another app holds it).
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HotkeysView {
    pub rows: Vec<HotkeyRow>,
}

#[derive(Debug, Default)]
struct Inner {
    bound: HashMap<Action, Option<Keys>>,
    /// Registered with the OS now (not when it failed, nor while the recorder runs).
    registered: HashMap<Action, Keys>,
    /// Saved but not registered at the last attempt.
    inactive: Vec<Action>,
    recording: bool,
}

/// The bindings the app runs with.
#[derive(Debug, Default)]
pub struct Hotkeys {
    inner: Mutex<Inner>,
}

impl Hotkeys {
    /// From `[hotkeys]`; a value that doesn't parse falls back to the default.
    pub fn from_settings(s: &HotkeySettings) -> Self {
        let bound = Action::ALL
            .into_iter()
            .map(|a| {
                let text = s.get(a);
                let keys = if text.trim().is_empty() {
                    None
                } else {
                    Some(Keys::parse(text).unwrap_or_else(|_| {
                        tracing::warn!(action = ?a, "hotkey in settings.toml not understood; default used");
                        default_keys(a)
                    }))
                };
                (a, keys)
            })
            .collect();
        Self {
            inner: Mutex::new(Inner {
                bound,
                ..Inner::default()
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("hotkeys lock")
    }

    pub fn keys(&self, action: Action) -> Option<Keys> {
        self.lock().bound.get(&action).copied().flatten()
    }

    pub fn action_for(&self, shortcut: &Shortcut) -> Option<Action> {
        let inner = self.lock();
        inner
            .registered
            .iter()
            .find(|(_, k)| shortcut.matches(k.mods, k.code))
            .map(|(a, _)| *a)
    }

    pub fn view(&self) -> HotkeysView {
        let inner = self.lock();
        HotkeysView {
            rows: Action::ALL
                .into_iter()
                .map(|a| HotkeyRow {
                    action: a,
                    keys: inner
                        .bound
                        .get(&a)
                        .copied()
                        .flatten()
                        .map(|k| k.to_string()),
                    default: default_keys(a).to_string(),
                    active: !inner.inactive.contains(&a),
                })
                .collect(),
        }
    }

    /// The bindings as saved in `settings.toml`.
    pub fn to_settings(&self) -> HotkeySettings {
        let mut s = HotkeySettings::default();
        for a in Action::ALL {
            s.set(a, self.keys(a).map(|k| k.to_string()).unwrap_or_default());
        }
        s
    }

    /// Register every binding (at start, and when the recorder stops).
    pub fn register_all(&self, reg: &impl Registrar) {
        let mut inner = self.lock();
        inner.inactive.clear();
        for a in Action::ALL {
            let Some(k) = inner.bound.get(&a).copied().flatten() else {
                continue;
            };
            if inner.registered.get(&a) == Some(&k) {
                continue;
            }
            if reg.register(k) {
                inner.registered.insert(a, k);
            } else {
                inner.inactive.push(a);
            }
        }
    }

    /// The Settings window records new keys: let every key combination reach it, then put the
    /// hotkeys back (HK-02).
    pub fn set_recording(&self, reg: &impl Registrar, on: bool) {
        if on {
            let mut inner = self.lock();
            inner.recording = true;
            for (_, k) in inner.registered.drain() {
                reg.unregister(k);
            }
        } else {
            self.lock().recording = false;
            self.register_all(reg);
        }
    }

    /// Bind `action` to `new` (`None` = off). Checked first (see [`Problem`]); when registering
    /// the new keys fails, the old binding is registered again and kept.
    pub fn rebind(
        &self,
        reg: &impl Registrar,
        action: Action,
        new: Option<Keys>,
        altgr: impl Fn(Keys) -> Option<(String, String)>,
    ) -> Result<(), Problem> {
        let mut inner = self.lock();
        let old = inner.bound.get(&action).copied().flatten();
        if let Some(k) = new {
            if let Some((&other, _)) = inner
                .bound
                .iter()
                .find(|(a, b)| **a != action && **b == Some(k))
            {
                return Err(Problem::Duplicate { action: other });
            }
            if let Some((layout, character)) = altgr(k) {
                return Err(Problem::AltGr { layout, character });
            }
        }
        if new == old && !inner.inactive.contains(&action) {
            return Ok(());
        }
        let live = !inner.recording;
        if let Some(k) = inner.registered.remove(&action) {
            reg.unregister(k);
        }
        if let Some(k) = new
            && live
        {
            if !reg.register(k) {
                if let Some(o) = old
                    && reg.register(o)
                {
                    inner.registered.insert(action, o);
                }
                return Err(Problem::InUse {
                    keys: k.to_string(),
                });
            }
            inner.registered.insert(action, k);
        }
        inner.bound.insert(action, new);
        inner.inactive.retain(|a| *a != action);
        Ok(())
    }
}

/// The plugin, with one handler for all hotkeys.
pub fn plugin() -> TauriPlugin<Wry> {
    Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state == ShortcutState::Pressed
                && let Some(action) = app.state::<Hotkeys>().action_for(shortcut)
            {
                run(app, action);
            }
        })
        .build()
}

/// Register the saved bindings. A key held by another app is skipped and shown as inactive.
pub fn register(app: &AppHandle) {
    app.state::<Hotkeys>().register_all(app);
}

/// HK-02: rebind from the Settings window, save, and tell the windows.
pub fn set(app: &AppHandle, action: Action, binding: &Binding) -> Result<HotkeysView, Problem> {
    let keys = match binding {
        Binding::Pressed(p) => Some(p.keys()?),
        Binding::Default => Some(default_keys(action)),
        Binding::Off => None,
    };
    let hotkeys = app.state::<Hotkeys>();
    // A binding is made while the hotkeys are registered, so "in use" is found now.
    hotkeys.set_recording(app, false);
    hotkeys.rebind(app, action, keys, altgr_character)?;
    let saved = hotkeys.to_settings();
    app.state::<settings::Store>().update(|s| s.hotkeys = saved);
    let view = hotkeys.view();
    let _ = app.emit(EVENT, &view);
    Ok(view)
}

pub fn set_recording(app: &AppHandle, on: bool) -> HotkeysView {
    let hotkeys = app.state::<Hotkeys>();
    hotkeys.set_recording(app, on);
    let view = hotkeys.view();
    let _ = app.emit(EVENT, &view);
    view
}

/// SV-09 for a new binding: the character Ctrl+Alt(+Shift) with this key types on an installed
/// keyboard layout, and that layout's language. Ctrl+Alt is how AltGr arrives, so such a hotkey
/// would swallow the character in every app. Only reads the layouts (`ToUnicodeEx` with flag 4
/// leaves the keyboard state alone).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn altgr_character(keys: Keys) -> Option<(String, String)> {
    use windows::Win32::Globalization::LCIDToLocaleName;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayoutList, HKL, MAPVK_VK_TO_VSC_EX, MapVirtualKeyExW, ToUnicodeEx, VK_CONTROL,
        VK_LCONTROL, VK_LSHIFT, VK_MENU, VK_RMENU, VK_SHIFT,
    };
    if !keys.mods.contains(Modifiers::CONTROL | Modifiers::ALT)
        || keys.mods.contains(Modifiers::SUPER)
    {
        return None;
    }
    let vk = character_vk(keys.code)?;
    let mut state = [0u8; 256];
    let mut held = vec![VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RMENU];
    if keys.mods.contains(Modifiers::SHIFT) {
        held.extend([VK_SHIFT, VK_LSHIFT]);
    }
    for k in held {
        state[usize::from(k.0)] = 0x80;
    }
    // SAFETY: plain Win32 reads into buffers sized for them.
    let mut layouts = [HKL::default(); 64];
    let n = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) } as usize;
    for &layout in &layouts[..n.min(layouts.len())] {
        // SAFETY: as above; flag 4 keeps the thread's keyboard (dead-key) state unchanged.
        let scan = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, Some(layout)) };
        let mut buf = [0u16; 8];
        let len = unsafe { ToUnicodeEx(u32::from(vk), scan, &state, &mut buf, 4, Some(layout)) };
        // < 0: a dead key (e.g. AltGr+¨ → ~), which types its character with the next key.
        let len = len.unsigned_abs() as usize;
        let text: String = String::from_utf16_lossy(&buf[..len.min(buf.len())])
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        if text.is_empty() {
            continue;
        }
        let lcid = layout_lcid(layout.0 as usize);
        let mut name = [0u16; 85];
        // SAFETY: the buffer is LOCALE_NAME_MAX_LENGTH long.
        let len = unsafe { LCIDToLocaleName(lcid, Some(&mut name), 0) };
        let tag = String::from_utf16_lossy(&name[..(len.max(1) as usize - 1)]);
        return Some((tag, text));
    }
    None
}

/// The locale that names a keyboard layout handle (HKL): its high word is the layout (0x041D
/// Swedish, also when it is used under an English language); 0xF… there is a handle to a layout
/// variant, and then the low word (the language) has to do.
#[cfg(windows)]
fn layout_lcid(hkl: usize) -> u32 {
    let layout = ((hkl >> 16) & 0xFFFF) as u32;
    if layout & 0xF000 == 0xF000 {
        (hkl & 0xFFFF) as u32
    } else {
        layout
    }
}

#[cfg(not(windows))]
pub fn altgr_character(_keys: Keys) -> Option<(String, String)> {
    None
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
    use std::cell::RefCell;
    use std::collections::HashSet;

    use super::*;

    /// Holds the keys "other apps" registered, and what Polyglass has registered.
    #[derive(Default)]
    struct Fake {
        taken: HashSet<String>,
        ours: RefCell<Vec<String>>,
    }

    impl Registrar for Fake {
        fn register(&self, keys: Keys) -> bool {
            let k = keys.to_string();
            if self.taken.contains(&k) || self.ours.borrow().contains(&k) {
                return false;
            }
            self.ours.borrow_mut().push(k);
            true
        }
        fn unregister(&self, keys: Keys) {
            self.ours.borrow_mut().retain(|k| *k != keys.to_string());
        }
    }

    fn keys(s: &str) -> Keys {
        Keys::parse(s).unwrap()
    }

    fn no_altgr(_: Keys) -> Option<(String, String)> {
        None
    }

    fn defaults() -> Hotkeys {
        Hotkeys::from_settings(&HotkeySettings::default())
    }

    #[test]
    fn hk01_defaults_map_back_to_their_actions() {
        let h = defaults();
        let fake = Fake::default();
        h.register_all(&fake);
        for (action, code) in DEFAULTS {
            assert_eq!(
                h.action_for(&Shortcut::new(Some(modifiers()), code)),
                Some(action)
            );
        }
        assert_eq!(
            h.action_for(&Shortcut::new(Some(Modifiers::CONTROL), Code::KeyL)),
            None
        );
        assert_eq!(fake.ours.borrow().len(), 5);
        assert_eq!(
            HotkeySettings::default().get(Action::ShowHide),
            "Ctrl+Alt+L"
        );
    }

    #[test]
    fn hk02_keys_are_written_and_read_back() {
        for text in [
            "Ctrl+Alt+L",
            "Ctrl+Shift+1",
            "Alt+Win+F5",
            "Ctrl+Alt+Shift+Win+Z",
        ] {
            assert_eq!(keys(text).to_string(), text);
        }
        assert_eq!(keys(" ctrl + alt + l ").to_string(), "Ctrl+Alt+L");
        let pressed = |code: &str, ctrl, alt, shift, meta| {
            Pressed {
                code: code.into(),
                ctrl,
                alt,
                shift,
                meta,
            }
            .keys()
        };
        assert_eq!(
            pressed("KeyK", true, true, false, false)
                .unwrap()
                .to_string(),
            "Ctrl+Alt+K"
        );
        assert_eq!(
            pressed("F9", false, false, true, true).unwrap().to_string(),
            "Shift+Win+F9"
        );
        assert_eq!(
            pressed("KeyK", false, false, true, false),
            Err(Problem::NoModifier)
        );
        assert_eq!(
            pressed("F9", false, false, false, false),
            Err(Problem::NoModifier)
        );
        for bad in [
            "Space",
            "Escape",
            "ArrowUp",
            "Numpad1",
            "Semicolon",
            "KeyL; x",
            "",
        ] {
            assert_eq!(
                pressed(bad, true, true, false, false),
                Err(Problem::Key),
                "{bad}"
            );
        }
        for bad in ["", "Ctrl+Alt", "Ctrl+Alt+Space", "Hyper+L", "Ctrl+Alt+L+K"] {
            assert!(Keys::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn hk02_rebinding_saves_and_moves_the_registration() {
        let h = defaults();
        let fake = Fake::default();
        h.register_all(&fake);
        h.rebind(&fake, Action::Swap, Some(keys("Ctrl+Shift+F9")), no_altgr)
            .unwrap();
        assert!(fake.ours.borrow().contains(&"Ctrl+Shift+F9".to_string()));
        assert!(!fake.ours.borrow().contains(&"Ctrl+Alt+S".to_string()));
        assert_eq!(
            h.action_for(&keys("Ctrl+Shift+F9").shortcut()),
            Some(Action::Swap)
        );
        assert_eq!(h.action_for(&keys("Ctrl+Alt+S").shortcut()), None);
        let saved = h.to_settings();
        assert_eq!(saved.get(Action::Swap), "Ctrl+Shift+F9");
        // Off: unregistered and saved as "".
        h.rebind(&fake, Action::Copy, None, no_altgr).unwrap();
        assert!(!fake.ours.borrow().contains(&"Ctrl+Alt+C".to_string()));
        assert_eq!(h.to_settings().get(Action::Copy), "");
        assert_eq!(
            Hotkeys::from_settings(&h.to_settings()).keys(Action::Copy),
            None
        );
        assert_eq!(
            Hotkeys::from_settings(&h.to_settings()).keys(Action::Swap),
            Some(keys("Ctrl+Shift+F9"))
        );
    }

    #[test]
    fn hk02_conflicts_are_reported_and_the_old_binding_stays() {
        let h = defaults();
        let fake = Fake {
            taken: ["Ctrl+Alt+K".to_string()].into(),
            ..Fake::default()
        };
        h.register_all(&fake);
        // Another Polyglass action.
        assert_eq!(
            h.rebind(&fake, Action::Swap, Some(keys("Ctrl+Alt+L")), no_altgr),
            Err(Problem::Duplicate {
                action: Action::ShowHide
            })
        );
        // Another app holds it: refused, Ctrl+Alt+S still works.
        assert_eq!(
            h.rebind(&fake, Action::Swap, Some(keys("Ctrl+Alt+K")), no_altgr),
            Err(Problem::InUse {
                keys: "Ctrl+Alt+K".into()
            })
        );
        assert_eq!(h.keys(Action::Swap), Some(keys("Ctrl+Alt+S")));
        assert_eq!(
            h.action_for(&keys("Ctrl+Alt+S").shortcut()),
            Some(Action::Swap)
        );
        // An AltGr character on an installed layout.
        let altgr = |k: Keys| (k == keys("Ctrl+Alt+E")).then(|| ("sv-SE".into(), "€".into()));
        assert_eq!(
            h.rebind(&fake, Action::Swap, Some(keys("Ctrl+Alt+E")), altgr),
            Err(Problem::AltGr {
                layout: "sv-SE".into(),
                character: "€".into()
            })
        );
        assert_eq!(h.keys(Action::Swap), Some(keys("Ctrl+Alt+S")));
    }

    #[test]
    fn hk02_saved_hotkey_another_app_holds_is_reported_inactive() {
        let h = defaults();
        let fake = Fake {
            taken: ["Ctrl+Alt+F".to_string()].into(),
            ..Fake::default()
        };
        h.register_all(&fake);
        let row = |a| h.view().rows.into_iter().find(|r| r.action == a).unwrap();
        assert!(!row(Action::Freeze).active);
        assert_eq!(row(Action::Freeze).keys.as_deref(), Some("Ctrl+Alt+F"));
        assert!(row(Action::Copy).active);
        // Binding other keys makes it active again.
        h.rebind(&fake, Action::Freeze, Some(keys("Ctrl+Alt+G")), no_altgr)
            .unwrap();
        assert!(row(Action::Freeze).active);
    }

    #[test]
    fn hk02_recording_lets_every_key_through_then_registers_again() {
        let h = defaults();
        let fake = Fake::default();
        h.register_all(&fake);
        h.set_recording(&fake, true);
        assert!(fake.ours.borrow().is_empty());
        assert_eq!(h.action_for(&keys("Ctrl+Alt+L").shortcut()), None);
        // A rebind while recording is saved and registered when recording stops.
        h.rebind(&fake, Action::Freeze, Some(keys("Ctrl+Alt+G")), no_altgr)
            .unwrap();
        assert!(fake.ours.borrow().is_empty());
        h.set_recording(&fake, false);
        assert_eq!(fake.ours.borrow().len(), 5);
        assert!(fake.ours.borrow().contains(&"Ctrl+Alt+G".to_string()));
    }

    #[test]
    fn hk02_unreadable_saved_hotkey_falls_back_to_its_default() {
        let mut s = HotkeySettings::default();
        s.set(Action::Copy, "Ctrl+Alt+Space".into());
        s.set(Action::Swap, "".into());
        let h = Hotkeys::from_settings(&s);
        assert_eq!(h.keys(Action::Copy), Some(default_keys(Action::Copy)));
        assert_eq!(h.keys(Action::Swap), None);
    }

    /// SV-09 / HK-02: with the Swedish layout installed, Ctrl+Alt bindings that type an AltGr
    /// character are reported with the character; the HK-01 keys aren't. Skips without the
    /// layout (it never installs one).
    #[cfg(windows)]
    #[test]
    fn hk02_altgr_characters_on_the_swedish_layout_are_reported() {
        if !swedish_layout_installed() {
            eprintln!("skipped: the Swedish keyboard layout isn't installed");
            return;
        }
        let found = |s: &str| altgr_character(keys(s)).map(|(_, c)| c);
        assert_eq!(found("Ctrl+Alt+2").as_deref(), Some("@"));
        assert_eq!(found("Ctrl+Alt+E").as_deref(), Some("€"));
        assert_eq!(found("Ctrl+Alt+7").as_deref(), Some("{"));
        let (layout, _) = altgr_character(keys("Ctrl+Alt+4")).unwrap();
        assert!(!layout.is_empty());
        for (_, code) in DEFAULTS {
            let k = Keys {
                mods: modifiers(),
                code,
            };
            assert_eq!(altgr_character(k), None, "{k}");
        }
        // Win, or no Ctrl+Alt: not AltGr.
        assert_eq!(found("Ctrl+Alt+Win+2"), None);
        assert_eq!(found("Ctrl+Shift+2"), None);
        assert_eq!(found("Ctrl+Alt+F5"), None);
    }

    /// The message names the keyboard layout, not the language it is used under: the dev
    /// machine has the Swedish layout under English (UK), HKL 0x041D0809.
    #[cfg(windows)]
    #[test]
    fn hk02_altgr_report_names_the_layout() {
        assert_eq!(layout_lcid(0x041D_0809), 0x041D);
        assert_eq!(layout_lcid(0x041D_041D), 0x041D);
        assert_eq!(layout_lcid(0x0409_0409), 0x0409);
        assert_eq!(layout_lcid(0xF002_0409), 0x0409);
    }

    #[cfg(windows)]
    #[allow(unsafe_code)]
    fn swedish_layout_installed() -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL};
        let mut layouts = [HKL::default(); 64];
        // SAFETY: a buffer sized for the call.
        let n = unsafe { GetKeyboardLayoutList(Some(&mut layouts)) } as usize;
        layouts[..n]
            .iter()
            .any(|h| (h.0 as usize & 0xFFFF_FFFF) == 0x041D_041D)
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

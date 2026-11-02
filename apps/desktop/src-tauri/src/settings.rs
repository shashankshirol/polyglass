//! Settings in a human-readable TOML file in the platform config directory (SET-01).
//!
//! The file is `settings.toml` in Tauri's app config directory (Windows:
//! `%APPDATA%\org.polyglass.desktop`). It is read once at start and written atomically (a
//! temporary file, then a rename) when a setting changes and when the app exits. Missing keys take
//! their defaults and unknown keys are ignored, so older and newer versions can share a file. A
//! file that doesn't parse is kept as `settings.toml.bad` and the defaults are used, so a typo
//! never stops the app.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};

pub const FILE: &str = "settings.toml";

const HEADER: &str = "# Polyglass settings (SET-01). Edit while Polyglass isn't running; unknown \
                      keys are ignored.\n\n";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lens: LensSettings,
    pub translation: TranslationSettings,
    pub models: ModelSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSettings {
    /// Set once the first-run wizard was closed, declined or finished (MDL-06, SV-01), so it is
    /// offered only once. The model manager stays available from the lens and the tray.
    pub first_run_done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensSettings {
    /// Last outer position in physical virtual-desktop px; unset until the lens was placed.
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Last size in logical px.
    pub width: f64,
    pub height: f64,
    /// Whether the interior lets clicks through (LENS-04).
    pub click_through: bool,
}

impl Default for LensSettings {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: crate::lens::DEFAULT_SIZE.0,
            height: crate::lens::DEFAULT_SIZE.1,
            click_through: true,
        }
    }
}

/// The language pair (MDL-07). Any pair the model catalog lists; a pair it doesn't list is
/// replaced when the app starts (`langs::Languages::choice`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationSettings {
    /// The language of the text under the lens (default `sv`, SV-01).
    pub source: String,
    /// The language translations are written in (default `en`).
    pub target: String,
    /// Identify each block as `source` or `target`; text already in the target passes through
    /// (SV-04, TR-07). Off: every block is read as `source`.
    pub auto: bool,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            source: "sv".into(),
            target: "en".into(),
            auto: true,
        }
    }
}

impl Settings {
    /// Read `path`, falling back to the defaults when it is missing or broken (a broken file is
    /// renamed to `settings.toml.bad`).
    pub fn load(path: &Path) -> Self {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                tracing::warn!(error = %e, "can't read settings; using defaults");
                return Self::default();
            }
        };
        // Windows editors may save UTF-8 with a byte order mark, which TOML doesn't allow.
        match toml::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)) {
            Ok(s) => s,
            Err(e) => {
                // Only the position: the message quotes the file.
                tracing::warn!(
                    span = ?e.span(),
                    "settings file doesn't parse; kept as .bad, using defaults"
                );
                let _ = fs::rename(path, path.with_extension("toml.bad"));
                Self::default()
            }
        }
    }

    /// Write to `path` atomically, creating its directory.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let body = toml::to_string(self).map_err(io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, format!("{HEADER}{body}"))?;
        fs::rename(&tmp, path)
    }
}

/// The settings the app runs with, and where they are saved.
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl Store {
    pub fn open(path: PathBuf) -> Self {
        let current = Mutex::new(Settings::load(&path));
        Self { path, current }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().expect("settings lock").clone()
    }

    /// Change the settings and save them if anything changed.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) {
        let mut cur = self.current.lock().expect("settings lock");
        let before = cur.clone();
        f(&mut cur);
        if *cur != before
            && let Err(e) = cur.save(&self.path)
        {
            tracing::warn!(error = %e, "can't save settings");
        }
    }
}

/// Record where the lens is and how big it is (saved when it hides and at exit).
pub fn remember_lens(app: &AppHandle, window: &WebviewWindow) {
    let Some((x, y, width, height)) = crate::lens::geometry(window) else {
        return;
    };
    app.state::<Store>().update(|s| {
        s.lens.x = Some(x);
        s.lens.y = Some(y);
        s.lens.width = width;
        s.lens.height = height;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set01_round_trips_through_readable_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cfg").join(FILE);
        let mut s = Settings::default();
        s.lens.x = Some(-1200);
        s.lens.y = Some(300);
        s.lens.width = 600.0;
        s.lens.click_through = false;
        s.translation.source = "de".into();
        s.translation.auto = false;
        s.models.first_run_done = true;
        s.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# Polyglass settings"), "{text}");
        assert!(
            text.contains("[lens]") && text.contains("x = -1200"),
            "{text}"
        );
        assert!(
            text.contains("[translation]") && text.contains("source = \"de\""),
            "{text}"
        );
        assert!(
            text.contains("[models]") && text.contains("first_run_done = true"),
            "{text}"
        );
        assert_eq!(Settings::load(&path), s);
    }

    #[test]
    fn set01_missing_file_and_keys_take_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        assert_eq!(Settings::load(&path), Settings::default());
        fs::write(
            &path,
            "[lens]\nwidth = 300.0\nfuture_key = 1\n[new_section]\na = 2\n",
        )
        .unwrap();
        let s = Settings::load(&path);
        assert_eq!(s.lens.width, 300.0);
        assert_eq!(s.lens.height, crate::lens::DEFAULT_SIZE.1);
        assert!(s.lens.click_through);
        assert_eq!(s.translation, TranslationSettings::default());
        assert!(!s.models.first_run_done, "the wizard is offered");
    }

    #[test]
    fn set01_reads_a_file_saved_with_a_byte_order_mark() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        fs::write(&path, "\u{feff}[lens]\nclick_through = false\n").unwrap();
        assert!(!Settings::load(&path).lens.click_through);
        assert!(path.exists());
    }

    #[test]
    fn set01_broken_file_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        fs::write(&path, "[lens\nwidth = ").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(!path.exists());
        assert!(dir.path().join("settings.toml.bad").exists());
    }

    #[test]
    fn store_saves_only_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        let store = Store::open(path.clone());
        store.update(|_| {});
        assert!(!path.exists(), "nothing changed, nothing written");
        store.update(|s| s.translation.target = "sv".into());
        assert_eq!(Settings::load(&path).translation.target, "sv");
    }
}

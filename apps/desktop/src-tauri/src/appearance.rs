//! How the lens looks (LENS-07): frame colour, overlay opacity, font family and font scale.
//!
//! Saved as `[appearance]` in `settings.toml` (SET-01) and sent to the lens with `lens_config`
//! and the [`EVENT`] after every change. The Settings window changes it through
//! `set_appearance`, which takes only values this module accepts (SEC-01); a hand-edited file
//! with values out of range is brought back into range when it is read. Reduced motion needs no
//! setting: the stylesheets turn animations off while the OS asks for it (LENS-07, second
//! clause).

use serde::{Deserialize, Serialize};

/// Event carrying the new [`Appearance`] to the lens.
pub const EVENT: &str = "lens-appearance";

/// Overlay opacity range in percent (LENS-07).
pub const OPACITY: std::ops::RangeInclusive<u8> = 20..=100;
/// Font scale range in percent (LENS-07).
pub const FONT_SCALE: std::ops::RangeInclusive<u16> = 75..=200;

/// The toolbar's and frame's navy (`style.css` `--chrome`).
pub const DEFAULT_FRAME: &str = "#182c42";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Frame, toolbar and Panel strip colour, `#rrggbb`. The lens picks light or dark text on
    /// it so the toolbar stays readable.
    pub frame_color: String,
    /// How opaque the translated blocks' backgrounds are, 20–100 %. The text stays opaque.
    pub opacity: u8,
    /// Font family for translations (overlay blocks and the Panel strip).
    pub font: Font,
    /// Font scale for translations, 75–200 %: it scales the starting and the smallest size of
    /// REN-02's fitting, and the Panel strip's text.
    pub font_scale: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            frame_color: DEFAULT_FRAME.into(),
            opacity: 100,
            font: Font::System,
            font_scale: 100,
        }
    }
}

/// Fonts that ship with Windows 10 and 11, so every choice draws as named. The UI maps each to a
/// CSS font stack (`appearance.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Font {
    /// The system UI font (Segoe UI).
    System,
    Arial,
    Calibri,
    Verdana,
    Tahoma,
    Georgia,
    TimesNewRoman,
    Consolas,
}

impl Appearance {
    /// Whether every value is in range (`set_appearance` rejects anything else).
    pub fn validate(&self) -> Result<(), String> {
        if !is_hex_color(&self.frame_color) {
            return Err("frame colour must be #rrggbb".into());
        }
        if !OPACITY.contains(&self.opacity) {
            return Err(format!(
                "opacity must be {}–{} %",
                OPACITY.start(),
                OPACITY.end()
            ));
        }
        if !FONT_SCALE.contains(&self.font_scale) {
            return Err(format!(
                "font scale must be {}–{} %",
                FONT_SCALE.start(),
                FONT_SCALE.end()
            ));
        }
        Ok(())
    }

    /// The same appearance with every value in range: used on values read from the file.
    pub fn clamped(mut self) -> Self {
        if !is_hex_color(&self.frame_color) {
            self.frame_color = DEFAULT_FRAME.into();
        }
        self.frame_color.make_ascii_lowercase();
        self.opacity = self.opacity.clamp(*OPACITY.start(), *OPACITY.end());
        self.font_scale = self
            .font_scale
            .clamp(*FONT_SCALE.start(), *FONT_SCALE.end());
        self
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens07_defaults_are_valid_and_match_the_stylesheet() {
        let a = Appearance::default();
        assert!(a.validate().is_ok());
        let css = include_str!("../../ui/src/style.css");
        // `--chrome` is the default frame colour with the frame's alpha.
        assert!(
            css.contains("--chrome: rgba(24, 44, 66, 0.92)"),
            "style.css changed"
        );
        assert_eq!(a.frame_color, "#182c42");
    }

    #[test]
    fn lens07_set_appearance_takes_only_values_in_range() {
        let ok = |f: &dyn Fn(&mut Appearance)| {
            let mut a = Appearance::default();
            f(&mut a);
            a.validate()
        };
        assert!(ok(&|a| a.opacity = 20).is_ok());
        assert!(ok(&|a| a.opacity = 100).is_ok());
        assert!(ok(&|a| a.font_scale = 75).is_ok());
        assert!(ok(&|a| a.font_scale = 200).is_ok());
        assert!(ok(&|a| a.frame_color = "#A0b1C2".into()).is_ok());
        assert!(ok(&|a| a.opacity = 19).is_err());
        assert!(ok(&|a| a.opacity = 101).is_err());
        assert!(ok(&|a| a.font_scale = 74).is_err());
        assert!(ok(&|a| a.font_scale = 201).is_err());
        for bad in [
            "", "red", "#fff", "#1234567", "182c42#", "#12345g", "url(x)",
        ] {
            assert!(ok(&|a| a.frame_color = bad.into()).is_err(), "{bad}");
        }
    }

    #[test]
    fn lens07_unknown_font_is_rejected_and_names_are_kebab_case() {
        let json = |font: &str| {
            serde_json::from_str::<Appearance>(&format!(
                r##"{{"frame_color":"#182c42","opacity":80,"font":"{font}","font_scale":125}}"##
            ))
        };
        assert_eq!(json("times-new-roman").unwrap().font, Font::TimesNewRoman);
        assert_eq!(json("system").unwrap().font_scale, 125);
        assert!(json("Comic Sans MS").is_err());
        assert!(json("system-ui; color: red").is_err());
    }

    #[test]
    fn lens07_file_values_out_of_range_are_brought_into_range() {
        let a = Appearance {
            frame_color: "blue".into(),
            opacity: 5,
            font: Font::Georgia,
            font_scale: 900,
        }
        .clamped();
        assert_eq!(a.frame_color, DEFAULT_FRAME);
        assert_eq!(a.opacity, 20);
        assert_eq!(a.font_scale, 200);
        assert_eq!(a.font, Font::Georgia);
        assert!(a.validate().is_ok());
        let upper = Appearance {
            frame_color: "#AABBCC".into(),
            ..Appearance::default()
        };
        assert_eq!(upper.clamped().frame_color, "#aabbcc");
    }
}

//! Fixture images rendered from the ground truth in `tests/fixtures/*/truth.json` (M0-FIX-2).
//!
//! Layout of one image ([`FixtureImage::render`]):
//! - a page [`PAGE_WIDTH`] logical px wide (the SPEC §5 lens width) and as tall as its text, with
//!   a [`MARGIN`] on every side;
//! - block `i` is set in UI font size `FONT_SIZES[i % 5]` (12–16 logical px), word-wrapped to
//!   the page, with a gap of 0.8 line heights after each block;
//! - every length is logical px × scale; line tops and the left edge are whole physical pixels.
//!
//! The images are rendered when a test or `pg-bench` needs them, with the system UI font, and are
//! not committed (`tests/fixtures/README.md` explains why). For a given font file the output is
//! byte-for-byte deterministic; [`TextRenderer::font_sha256`] names the font, and the
//! `render-fixtures` binary writes the PNGs out for inspection.

use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use image::ExtendedColorType;
use image::ImageEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use pg_core::{Frame, LangTag, PhysRect, Rect};
use serde::{Deserialize, Serialize};

use crate::TextRenderer;

/// Monitor scales every fixture is rendered at (CAP-04, OCR-04, SV-03).
pub const SCALES: [f64; 4] = [1.0, 1.25, 1.5, 2.0];
/// UI font sizes (em, like CSS `font-size`) in logical px, used block by block in this order.
pub const FONT_SIZES: [f32; 5] = [12.0, 13.0, 14.0, 15.0, 16.0];
/// Page width in logical px.
pub const PAGE_WIDTH: f32 = 600.0;
/// Page margin in logical px.
pub const MARGIN: f32 = 12.0;
/// Gap after each block, in line heights of that block.
const BLOCK_GAP: f32 = 0.8;

/// Colour scheme of a fixture image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Light, Theme::Dark];

    pub fn name(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// Background and text colour: the Windows 11 defaults (window background `#F3F3F3` with
    /// near-black text, `#202020` with white text).
    pub fn colors(self) -> ([u8; 3], [u8; 3]) {
        match self {
            Theme::Light => ([243, 243, 243], [27, 27, 27]),
            Theme::Dark => ([32, 32, 32], [255, 255, 255]),
        }
    }
}

/// One `truth.json` (format in `tests/fixtures/README.md`). Fields the generator and scorers
/// don't use are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Truth {
    pub fixture: String,
    /// Language of the whole fixture; `None` for mixed fixtures, whose blocks carry their own.
    #[serde(default)]
    pub lang: Option<LangTag>,
    /// A committed image next to `truth.json`, if any (none so far).
    #[serde(default)]
    pub image: Option<String>,
    pub blocks: Vec<TruthBlock>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TruthBlock {
    pub text: String,
    #[serde(default)]
    pub lang: Option<LangTag>,
    /// Physical px at 100% in a committed image.
    #[serde(default)]
    pub bbox: Option<Rect>,
}

impl Truth {
    /// Read `dir/truth.json`.
    pub fn load(dir: &Path) -> io::Result<Truth> {
        let path = dir.join("truth.json");
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str(&raw).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        })
    }

    /// The OCR language for this fixture: its own, or Swedish for mixed Swedish/English text
    /// (Auto mode prefers the Swedish recognizer, ARCHITECTURE "One cycle").
    pub fn ocr_lang(&self) -> LangTag {
        self.lang.clone().unwrap_or_else(LangTag::sv)
    }

    /// Whether the fixture counts toward the Swedish accuracy set (SV-03) rather than the
    /// English one (OCR-04).
    pub fn is_swedish(&self) -> bool {
        self.ocr_lang().is_swedish()
    }
}

/// `tests/fixtures` in this workspace.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Every fixture under [`fixtures_dir`], sorted by directory name.
pub fn all() -> io::Result<Vec<Truth>> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("truth.json").is_file())
        .collect();
    dirs.sort();
    dirs.iter().map(|d| Truth::load(d)).collect()
}

/// One fixture by directory name.
pub fn load(name: &str) -> io::Result<Truth> {
    Truth::load(&fixtures_dir().join(name))
}

/// One rendered line of ground truth.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedLine {
    /// Index into `Truth::blocks`.
    pub block: usize,
    pub text: String,
    /// Font size in logical px.
    pub font_px: f32,
    /// Advance width × (ascent − descent), in physical image px.
    pub bbox: Rect,
}

/// A horizontal strip of an image holding whole lines (see [`FixtureImage::bands`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Band {
    pub y: u32,
    pub height: u32,
    /// Indices into [`FixtureImage::lines`].
    pub lines: Range<usize>,
}

/// A fixture rendered at one scale and theme.
#[derive(Debug, Clone)]
pub struct FixtureImage {
    pub fixture: String,
    pub scale: f64,
    pub theme: Theme,
    /// RGBA, origin (0, 0), `scale_factor` = `scale`.
    pub frame: Frame,
    /// Every line in top-to-bottom order.
    pub lines: Vec<RenderedLine>,
}

impl FixtureImage {
    /// Render `truth` at `scale` (1.0 = 100%) in `theme`.
    pub fn render(r: &TextRenderer, truth: &Truth, scale: f64, theme: Theme) -> FixtureImage {
        let s = scale as f32;
        let left = (MARGIN * s).round();
        let max_width = (PAGE_WIDTH - 2.0 * MARGIN) * s;
        let mut top = (MARGIN * s).round();
        let mut bottom = top;
        // Lay out first so the page height is known: (block, text, physical px, top).
        let mut layout = Vec::new();
        for (i, block) in truth.blocks.iter().enumerate() {
            let font_px = FONT_SIZES[i % FONT_SIZES.len()];
            let px = font_px * s;
            let line_h = r.line_height(px);
            for text in r.wrap(px, &block.text, max_width) {
                layout.push((i, text, font_px, top));
                bottom = top + line_h;
                top = (top + line_h).round();
            }
            top = (top + line_h * BLOCK_GAP).round();
        }
        let width = (PAGE_WIDTH * s).round() as u32;
        let height = (bottom + MARGIN * s).ceil() as u32;
        let (bg, fg) = theme.colors();
        let mut frame =
            Frame::solid(width, height, bg).with_origin(PhysRect::new(0, 0, width, height), scale);
        let lines = layout
            .into_iter()
            .map(|(block, text, font_px, top)| {
                let bbox = r.draw(&mut frame, left, top, font_px * s, &text, fg);
                RenderedLine {
                    block,
                    text,
                    font_px,
                    bbox,
                }
            })
            .collect();
        FixtureImage {
            fixture: truth.fixture.clone(),
            scale,
            theme,
            frame,
            lines,
        }
    }

    /// File-name stem, e.g. `ui-dialog-sv@125-dark`.
    pub fn stem(&self) -> String {
        format!(
            "{}@{}-{}",
            self.fixture,
            (self.scale * 100.0).round(),
            self.theme.name()
        )
    }

    /// The image as PNG (RGBA, deterministic for a given font file).
    pub fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        PngEncoder::new_with_quality(&mut out, CompressionType::Best, FilterType::Adaptive)
            .write_image(
                self.frame.data(),
                self.frame.width(),
                self.frame.height(),
                ExtendedColorType::Rgba8,
            )
            .expect("encoding an RGBA frame to PNG in memory cannot fail");
        out
    }

    /// The text of `lines`, one per line.
    pub fn text_of(&self, lines: Range<usize>) -> String {
        self.lines[lines]
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Split the image into strips no taller than `max_height` px (unless one line is), each
    /// holding whole lines, cut halfway between lines. Strips the size of the lens are what the
    /// lens would see when moved down the page.
    pub fn bands(&self, max_height: u32) -> Vec<Band> {
        let mut out = Vec::new();
        let mut start = 0;
        let mut y = 0u32;
        while start < self.lines.len() {
            let limit = y as f32 + max_height as f32;
            let mut end = start + 1;
            while end < self.lines.len() && self.lines[end].bbox.bottom() <= limit {
                end += 1;
            }
            let cut = match self.lines.get(end) {
                Some(next) => {
                    let gap_mid = (self.lines[end - 1].bbox.bottom() + next.bbox.y) / 2.0;
                    (gap_mid.round() as u32).min(limit as u32).max(y + 1)
                }
                None => {
                    let last = self.lines[end - 1].bbox.bottom().ceil() as u32;
                    self.frame.height().min(limit as u32).max(last)
                }
            };
            out.push(Band {
                y,
                height: cut - y,
                lines: start..end,
            });
            start = end;
            y = cut;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> Option<TextRenderer> {
        // Skips where no system font is installed (minimal CI containers).
        TextRenderer::system()
    }

    #[test]
    fn every_fixture_parses() {
        let all = all().unwrap();
        assert_eq!(all.len(), 8);
        for t in &all {
            assert!(!t.blocks.is_empty(), "{}", t.fixture);
        }
        assert!(load("mixed-sv-en").unwrap().is_swedish());
        assert!(!load("ui-dialog-en").unwrap().is_swedish());
    }

    #[test]
    fn rendering_is_deterministic() {
        let Some(r) = renderer() else { return };
        let truth = load("ui-dialog-sv").unwrap();
        for scale in SCALES {
            let a = FixtureImage::render(&r, &truth, scale, Theme::Dark);
            let b = FixtureImage::render(&r, &truth, scale, Theme::Dark);
            assert_eq!(a.png(), b.png(), "{}", a.stem());
            assert_eq!(a.lines, b.lines);
        }
    }

    #[test]
    fn layout_scales_with_the_monitor() {
        let Some(r) = renderer() else { return };
        let truth = load("paragraphs-sv").unwrap();
        let base = FixtureImage::render(&r, &truth, 1.0, Theme::Light);
        assert_eq!(base.frame.width(), 600);
        // Every word of the truth is rendered, in order.
        let words: Vec<&str> = truth
            .blocks
            .iter()
            .flat_map(|b| b.text.split_whitespace())
            .collect();
        let rendered = base.text_of(0..base.lines.len());
        assert_eq!(rendered.split_whitespace().collect::<Vec<_>>(), words);
        for scale in SCALES {
            let img = FixtureImage::render(&r, &truth, scale, Theme::Light);
            assert_eq!(img.stem(), format!("paragraphs-sv@{}-light", scale * 100.0));
            assert_eq!(img.frame.width(), (600.0 * scale).round() as u32);
            assert_eq!(img.frame.scale_factor, scale);
            // Same wrapping at every scale; boxes grow with it (rounding aside).
            assert_eq!(img.lines.len(), base.lines.len(), "{}", img.stem());
            for (l, b) in img.lines.iter().zip(&base.lines) {
                assert_eq!(l.text, b.text);
                assert!((l.bbox.w - b.bbox.w * scale as f32).abs() <= 2.0);
                assert!((l.bbox.y - b.bbox.y * scale as f32).abs() <= 2.0 * scale as f32 + 2.0);
            }
            assert!(
                img.lines
                    .iter()
                    .all(|l| l.bbox.right() <= img.frame.width() as f32)
            );
            assert!(
                img.lines
                    .iter()
                    .all(|l| l.bbox.bottom() <= img.frame.height() as f32)
            );
            assert!(img.lines.iter().all(|l| (12.0..=16.0).contains(&l.font_px)));
        }
    }

    #[test]
    fn themes_swap_text_and_background() {
        let Some(r) = renderer() else { return };
        let truth = load("ui-dialog-en").unwrap();
        for theme in Theme::ALL {
            let img = FixtureImage::render(&r, &truth, 1.0, theme);
            let (bg, fg) = theme.colors();
            assert_eq!(img.frame.rgb(0, 0), bg);
            // Some pixel of the 16 px line has (nearly) the full text colour.
            let near_fg = |p: [u8; 3]| p.iter().zip(fg).all(|(a, b)| a.abs_diff(b) <= 16);
            let b = img.lines.iter().find(|l| l.font_px == 16.0).unwrap().bbox;
            let found = (b.y as u32..b.bottom() as u32)
                .any(|y| (b.x as u32..b.right() as u32).any(|x| near_fg(img.frame.rgb(x, y))));
            assert!(found, "{theme:?}");
        }
    }

    #[test]
    fn bands_hold_whole_lines() {
        let Some(r) = renderer() else { return };
        let truth = load("mixed-sv-en").unwrap();
        let img = FixtureImage::render(&r, &truth, 1.5, Theme::Light);
        let bands = img.bands(450);
        assert!(bands.len() > 1);
        let mut next_line = 0;
        let mut next_y = 0;
        for band in &bands {
            assert_eq!(band.y, next_y);
            assert_eq!(band.lines.start, next_line);
            assert!(band.height <= 450);
            for l in &img.lines[band.lines.clone()] {
                assert!(
                    l.bbox.y >= band.y as f32 && l.bbox.bottom() <= (band.y + band.height) as f32
                );
            }
            next_line = band.lines.end;
            next_y = band.y + band.height;
        }
        assert_eq!(next_line, img.lines.len());
        assert!(next_y <= img.frame.height());
    }
}

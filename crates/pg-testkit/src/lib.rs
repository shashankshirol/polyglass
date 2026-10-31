//! Test and benchmark helpers for Polyglass.
//!
//! [`TextRenderer`] draws text into a [`Frame`] with an installed system font, so OCR engines can
//! be tested and benchmarked before the fixture images exist (CHECKLIST M0-FIX-2). Rendering is
//! grayscale anti-aliased (no ClearType/subpixel), so real screen captures are harder than this.
//! The font is read from the OS at run time and never copied into the repository.

use std::fmt;
use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use pg_core::{Frame, PixelFormat, Rect};

/// Renders text into frames with one font.
pub struct TextRenderer {
    font: FontVec,
    path: PathBuf,
}

impl fmt::Debug for TextRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextRenderer")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl TextRenderer {
    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let font = FontVec::try_from_vec(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(Self {
            font,
            path: path.to_owned(),
        })
    }

    /// A common sans-serif UI font installed with the OS (Segoe UI on Windows), if found.
    pub fn system() -> Option<Self> {
        system_font_candidates()
            .into_iter()
            .find_map(|p| Self::from_file(&p).ok())
    }

    /// Distance between the tops of consecutive lines at `px`.
    pub fn line_height(&self, px: f32) -> f32 {
        let f = self.font.as_scaled(PxScale::from(px));
        f.ascent() - f.descent() + f.line_gap()
    }

    /// Advance width of `text` at `px`.
    pub fn text_width(&self, px: f32, text: &str) -> f32 {
        let f = self.font.as_scaled(PxScale::from(px));
        let mut width = 0.0;
        let mut prev = None;
        for c in text.chars() {
            let id = f.glyph_id(c);
            if let Some(p) = prev {
                width += f.kern(p, id);
            }
            width += f.h_advance(id);
            prev = Some(id);
        }
        width
    }

    /// Draw one line of `text` with its top-left at (`x`, `top`) in `rgb`, alpha-blended over the
    /// frame (RGBA or BGRA). Returns the line's box: advance width × (ascent − descent).
    pub fn draw(
        &self,
        frame: &mut Frame,
        x: f32,
        top: f32,
        px: f32,
        text: &str,
        rgb: [u8; 3],
    ) -> Rect {
        let scaled = self.font.as_scaled(PxScale::from(px));
        let baseline = top + scaled.ascent();
        let (w, h, stride) = (frame.width(), frame.height(), frame.stride());
        let color = match frame.format() {
            PixelFormat::Bgra8 => [rgb[2], rgb[1], rgb[0]],
            PixelFormat::Rgba8 => rgb,
            PixelFormat::Gray8 => panic!("TextRenderer draws into RGBA/BGRA frames only"),
        };
        let data = frame.data_mut();
        let mut pen = x;
        let mut prev = None;
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                pen += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(px), point(pen, baseline));
            pen += scaled.h_advance(id);
            prev = Some(id);
            let Some(outline) = self.font.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                let px_x = bounds.min.x as i64 + i64::from(gx);
                let px_y = bounds.min.y as i64 + i64::from(gy);
                if px_x < 0 || px_y < 0 || px_x >= i64::from(w) || px_y >= i64::from(h) {
                    return;
                }
                let i = px_y as usize * stride + px_x as usize * 4;
                let a = coverage.clamp(0.0, 1.0);
                for (dst, src) in data[i..i + 3].iter_mut().zip(color) {
                    *dst = (f32::from(*dst) * (1.0 - a) + f32::from(src) * a).round() as u8;
                }
            });
        }
        Rect::new(x, top, pen - x, scaled.ascent() - scaled.descent())
    }

    /// Greedy word-wrap of `text` into lines no wider than `max_width` at `px`. A single word
    /// wider than `max_width` gets a line of its own.
    pub fn wrap(&self, px: f32, text: &str, max_width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!("{current} {word}")
            };
            if !current.is_empty() && self.text_width(px, &candidate) > max_width {
                lines.push(std::mem::replace(&mut current, word.to_owned()));
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }
}

fn system_font_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        let fonts = PathBuf::from(windir).join("Fonts");
        out.push(fonts.join("segoeui.ttf"));
        out.push(fonts.join("arial.ttf"));
    } else if cfg!(target_os = "macos") {
        out.push("/System/Library/Fonts/Supplemental/Arial.ttf".into());
        out.push("/Library/Fonts/Arial.ttf".into());
    } else {
        out.push("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into());
        out.push("/usr/share/fonts/dejavu/DejaVuSans.ttf".into());
        out.push("/usr/share/fonts/TTF/DejaVuSans.ttf".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_dark_pixels_inside_the_returned_box() {
        // Skips where no system font is installed (minimal CI containers).
        let Some(r) = TextRenderer::system() else {
            return;
        };
        let mut f = Frame::solid(200, 40, [255, 255, 255]);
        let bbox = r.draw(&mut f, 10.0, 5.0, 16.0, "Hello", [0, 0, 0]);
        assert!(bbox.w > 20.0 && bbox.w < 80.0, "{bbox:?}");
        let (mut inside, mut outside) = (0, 0);
        for y in 0..f.height() {
            for x in 0..f.width() {
                if f.luma(x, y) >= 128 {
                    continue;
                }
                let (fx, fy) = (x as f32, y as f32);
                if fx >= bbox.x - 1.0
                    && fx <= bbox.right() + 1.0
                    && fy >= bbox.y - 1.0
                    && fy <= bbox.bottom() + 1.0
                {
                    inside += 1;
                } else {
                    outside += 1;
                }
            }
        }
        assert!(inside > 50);
        assert_eq!(outside, 0);
    }

    #[test]
    fn wraps_at_width() {
        let Some(r) = TextRenderer::system() else {
            return;
        };
        let text = "one two three four five six seven eight nine ten";
        let lines = r.wrap(16.0, text, 100.0);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| r.text_width(16.0, l) <= 100.0));
        assert_eq!(lines.join(" "), text);
    }
}

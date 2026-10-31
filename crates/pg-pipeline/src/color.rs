//! Background sampling and text colour for Overlay mode (REN-01).

use pg_core::{Frame, Rect};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    /// WCAG 2.1 relative luminance.
    pub fn relative_luminance(self) -> f64 {
        fn channel(c: u8) -> f64 {
            let c = f64::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// WCAG contrast ratio (1–21).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (a.relative_luminance(), b.relative_luminance());
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Black or white, whichever contrasts more with `bg`. Always ≥ 4.58:1 (REN-01 needs 4.5).
pub fn foreground_for(bg: Rgb) -> Rgb {
    if contrast_ratio(bg, Rgb::BLACK) >= contrast_ratio(bg, Rgb::WHITE) {
        Rgb::BLACK
    } else {
        Rgb::WHITE
    }
}

/// Sample the background around `bbox`: the per-channel median of pixels on a ring `pad`
/// pixels outside the box (clamped to the frame). Falls back to the pixels inside the box when
/// the ring is empty (box covers the whole frame).
pub fn sample_background(frame: &Frame, bbox: Rect, pad: u32) -> Rgb {
    let (fw, fh) = (frame.width() as i64, frame.height() as i64);
    if fw == 0 || fh == 0 {
        return Rgb::WHITE;
    }
    let x0 = (bbox.x.floor() as i64 - i64::from(pad)).clamp(0, fw - 1);
    let y0 = (bbox.y.floor() as i64 - i64::from(pad)).clamp(0, fh - 1);
    let x1 = (bbox.right().ceil() as i64 + i64::from(pad)).clamp(0, fw - 1);
    let y1 = (bbox.bottom().ceil() as i64 + i64::from(pad)).clamp(0, fh - 1);

    let mut samples: Vec<[u8; 3]> = Vec::new();
    // Sample every other pixel along the ring; plenty for a median.
    for x in (x0..=x1).step_by(2) {
        for y in [y0, y1] {
            samples.push(frame.rgb(x as u32, y as u32));
        }
    }
    for y in (y0..=y1).step_by(2) {
        for x in [x0, x1] {
            samples.push(frame.rgb(x as u32, y as u32));
        }
    }
    if samples.is_empty() {
        samples.push(frame.rgb(x0 as u32, y0 as u32));
    }
    let median = |ch: usize| {
        let mut v: Vec<u8> = samples.iter().map(|p| p[ch]).collect();
        v.sort_unstable();
        v[v.len() / 2]
    };
    Rgb(median(0), median(1), median(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_contrast_values() {
        assert!((contrast_ratio(Rgb::BLACK, Rgb::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(Rgb(119, 119, 119), Rgb::WHITE) - 4.48).abs() < 0.01);
    }

    #[test]
    fn ren01_foreground_always_meets_aa() {
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let bg = Rgb(r as u8, g as u8, b as u8);
                    let ratio = contrast_ratio(bg, foreground_for(bg));
                    assert!(ratio >= 4.5, "{bg:?} → {ratio}");
                }
            }
        }
    }

    #[test]
    fn samples_surrounding_colour_not_text() {
        // Dark-theme panel with a white "text" box inside.
        let mut f = Frame::solid(100, 40, [30, 30, 30]);
        let stride = f.stride();
        let data = f.data_mut();
        for y in 10..30 {
            for x in 20..80 {
                let i = y * stride + x * 4;
                data[i..i + 3].copy_from_slice(&[250, 250, 250]);
            }
        }
        let bg = sample_background(&f, Rect::new(20.0, 10.0, 60.0, 20.0), 2);
        assert_eq!(bg, Rgb(30, 30, 30));
        assert_eq!(foreground_for(bg), Rgb::WHITE);
    }
}

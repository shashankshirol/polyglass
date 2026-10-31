//! Change detection (CAP-03) and blank-region detection (CAP-06).
//!
//! A frame is reduced to a grid of mean-luma cells. Two frames differ when more than a
//! fraction of cells changed by more than a tolerance. This costs one pass over the pixels
//! (≈ 0.2 ms for 600×300) and ignores sub-cell noise.

use pg_core::{Frame, PixelFormat};

/// Cell edge length in physical pixels.
const CELL: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeConfig {
    /// Per-cell luma difference (0–255) below which a cell counts as unchanged.
    pub cell_tolerance: u8,
    /// Fraction of cells (0–1) that must change for the frame to count as changed.
    /// Tune against real captures: a blinking caret touches ~2 cells (see CHECKLIST M1-VER-2).
    pub changed_fraction: f32,
    /// Max − min cell luma at or below which a region is considered blank (CAP-06).
    pub blank_range: u8,
}

impl Default for ChangeConfig {
    fn default() -> Self {
        Self {
            cell_tolerance: 6,
            changed_fraction: 0.0015,
            blank_range: 3,
        }
    }
}

/// Downsampled luma grid of a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameSignature {
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    cells: Vec<u8>,
}

impl FrameSignature {
    pub fn of(frame: &Frame) -> Self {
        let (w, h) = (frame.width(), frame.height());
        let cols = w.div_ceil(CELL).max(1);
        let rows = h.div_ceil(CELL).max(1);
        let mut sums = vec![0u32; (cols * rows) as usize];
        let mut counts = vec![0u32; (cols * rows) as usize];
        let bpp = frame.format().bytes_per_pixel();
        let data = frame.data();
        for y in 0..h {
            let row = &data[y as usize * frame.stride()..][..w as usize * bpp];
            let cell_row = (y / CELL) * cols;
            for x in 0..w {
                let p = &row[x as usize * bpp..];
                let luma = match frame.format() {
                    PixelFormat::Bgra8 => luma(p[2], p[1], p[0]),
                    PixelFormat::Rgba8 => luma(p[0], p[1], p[2]),
                    PixelFormat::Gray8 => u32::from(p[0]),
                };
                let i = (cell_row + x / CELL) as usize;
                sums[i] += luma;
                counts[i] += 1;
            }
        }
        let cells = sums
            .iter()
            .zip(&counts)
            .map(|(s, c)| if *c == 0 { 0 } else { (s / c) as u8 })
            .collect();
        Self {
            width: w,
            height: h,
            cols,
            rows,
            cells,
        }
    }

    /// Fraction of cells (0–1) that differ by more than `tolerance`. Different frame sizes
    /// count as fully changed.
    pub fn changed_fraction(&self, other: &FrameSignature, tolerance: u8) -> f32 {
        if self.width != other.width || self.height != other.height {
            return 1.0;
        }
        let changed = self
            .cells
            .iter()
            .zip(&other.cells)
            .filter(|(a, b)| a.abs_diff(**b) > tolerance)
            .count();
        changed as f32 / self.cells.len() as f32
    }

    pub fn differs(&self, other: &FrameSignature, cfg: &ChangeConfig) -> bool {
        let f = self.changed_fraction(other, cfg.cell_tolerance);
        f > cfg.changed_fraction || (f > 0.0 && self.cells.len() < 100)
    }

    /// True if the region is uniform (e.g. DRM-protected video shows as black) (CAP-06).
    pub fn is_blank(&self, cfg: &ChangeConfig) -> bool {
        let (min, max) = self
            .cells
            .iter()
            .fold((u8::MAX, u8::MIN), |(lo, hi), &c| (lo.min(c), hi.max(c)));
        max.saturating_sub(min) <= cfg.blank_range
    }

    pub fn grid(&self) -> (u32, u32) {
        (self.cols, self.rows)
    }
}

fn luma(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint(frame: &mut Frame, x0: u32, y0: u32, w: u32, h: u32, rgb: [u8; 3]) {
        let stride = frame.stride();
        let data = frame.data_mut();
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let i = y as usize * stride + x as usize * 4;
                data[i..i + 3].copy_from_slice(&rgb);
            }
        }
    }

    #[test]
    fn identical_frames_do_not_differ() {
        let cfg = ChangeConfig::default();
        let a = FrameSignature::of(&Frame::solid(600, 300, [240, 240, 240]));
        let b = FrameSignature::of(&Frame::solid(600, 300, [240, 240, 240]));
        assert!(!a.differs(&b, &cfg));
        assert_eq!(a.grid(), (75, 38));
    }

    #[test]
    fn a_changed_word_is_detected() {
        let cfg = ChangeConfig::default();
        let base = Frame::solid(600, 300, [255, 255, 255]);
        let mut changed = base.clone();
        // A dark ~40×14 px word: covers ≥ 5 cells of 8×8.
        paint(&mut changed, 100, 100, 40, 14, [20, 20, 20]);
        let a = FrameSignature::of(&base);
        let b = FrameSignature::of(&changed);
        assert!(a.differs(&b, &cfg));
    }

    #[test]
    fn a_one_pixel_caret_is_ignored() {
        let cfg = ChangeConfig::default();
        let base = Frame::solid(600, 300, [255, 255, 255]);
        let mut caret = base.clone();
        paint(&mut caret, 101, 100, 1, 16, [0, 0, 0]);
        let a = FrameSignature::of(&base);
        let b = FrameSignature::of(&caret);
        assert!(!a.differs(&b, &cfg));
    }

    #[test]
    fn size_change_is_full_change() {
        let a = FrameSignature::of(&Frame::solid(100, 100, [0, 0, 0]));
        let b = FrameSignature::of(&Frame::solid(100, 101, [0, 0, 0]));
        assert_eq!(a.changed_fraction(&b, 0), 1.0);
    }

    #[test]
    fn cap06_blank_detection() {
        let cfg = ChangeConfig::default();
        assert!(FrameSignature::of(&Frame::solid(320, 200, [0, 0, 0])).is_blank(&cfg));
        let mut text = Frame::solid(320, 200, [0, 0, 0]);
        paint(&mut text, 10, 10, 30, 12, [255, 255, 255]);
        assert!(!FrameSignature::of(&text).is_blank(&cfg));
    }
}

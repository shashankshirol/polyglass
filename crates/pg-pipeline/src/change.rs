//! Change detection (CAP-03) and blank-region detection (CAP-06).
//!
//! A frame is reduced to its luma plane and a grid of mean-luma cells. It differs from the
//! reference when any pixel moved by more than a tolerance, unless the only such pixels form a
//! blinking text caret (one thin vertical bar), or when a cell's mean moved by more than a smaller
//! tolerance (a fade). A signature plus a comparison takes ≈ 0.5 ms for 600×300 and 2 ms for
//! 1200×600 (release build).
//!
//! Tuned on real WGC captures of GDI ClearType text (`tests/change_wgc.rs`, ADR-0003): static
//! content captures bit-exactly, while a caret and a changed digit or typed letter both touch
//! 2–12 cells of 8 px. No cell count or fraction separates them, so the caret is recognised by
//! shape, and a changed digit can move every cell's mean by as little as 2 levels, so drawn
//! changes are found per pixel.

use pg_core::{Frame, PixelFormat};

/// Cell edge length in physical pixels.
const CELL: u32 = 8;
/// Shortest caret in logical px: the line height of 9 px text is about 12 px.
const MIN_CARET_HEIGHT: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChangeConfig {
    /// Per-cell mean-luma difference (0–255) above which a cell counts as changed (fades).
    pub cell_tolerance: u8,
    /// Per-pixel luma difference (0–255) above which a pixel counts as changed (drawn changes).
    pub pixel_tolerance: u8,
    /// Widest change, in logical px, that still counts as a caret. Windows and browsers draw the
    /// caret 1–2 px wide at 100% and anti-alias it at fractional positions.
    pub caret_width: f32,
    /// Max − min cell luma at or below which a region is considered blank (CAP-06).
    pub blank_range: u8,
}

impl Default for ChangeConfig {
    fn default() -> Self {
        Self {
            cell_tolerance: 3,
            pixel_tolerance: 24,
            caret_width: 3.0,
            blank_range: 3,
        }
    }
}

/// The luma plane of a frame and its downsampled grid of mean-luma cells.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSignature {
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    cells: Vec<u8>,
    luma: Vec<u8>,
    scale: f64,
}

impl FrameSignature {
    pub fn of(frame: &Frame) -> Self {
        let (w, h) = (frame.width(), frame.height());
        let cols = w.div_ceil(CELL).max(1);
        let rows = h.div_ceil(CELL).max(1);
        let mut sums = vec![0u32; (cols * rows) as usize];
        let mut counts = vec![0u32; (cols * rows) as usize];
        let mut lumas = Vec::with_capacity(w as usize * h as usize);
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
                lumas.push(luma as u8);
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
            luma: lumas,
            scale: frame.scale_factor,
        }
    }

    /// Fraction of cells (0–1) whose mean luma differs by more than `tolerance`. Different
    /// frame sizes count as fully changed.
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

    /// True if the frame changed against `other` (the last frame that was processed), ignoring
    /// a caret blink.
    ///
    /// A drawn change (text, caret, scroll, video) moves pixels by more than `pixel_tolerance`.
    /// It counts unless those pixels form a caret: one bar at most `caret_width` wide, at least
    /// twice as tall as wide and at least `MIN_CARET_HEIGHT` tall. A faint change (a fade) moves
    /// no pixel that far and counts when a cell's mean moves by more than `cell_tolerance`.
    pub fn differs(&self, other: &FrameSignature, cfg: &ChangeConfig) -> bool {
        if self.width != other.width || self.height != other.height {
            return true;
        }
        let scale = self.scale.max(1.0);
        let max_width = (f64::from(cfg.caret_width) * scale).round() as u32;
        let mut bar: Option<(u32, u32, u32, u32)> = None;
        for (i, (a, b)) in self.luma.iter().zip(&other.luma).enumerate() {
            if a.abs_diff(*b) > cfg.pixel_tolerance {
                let (x, y) = (i as u32 % self.width, i as u32 / self.width);
                let (x0, x1, y0, y1) = bar.unwrap_or((x, x, y, y));
                let grown = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
                if grown.1 - grown.0 + 1 > max_width {
                    return true;
                }
                bar = Some(grown);
            }
        }
        let Some((x0, x1, y0, y1)) = bar else {
            return self.cells_changed_outside(other, cfg.cell_tolerance, None);
        };
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        let caret = h >= 2 * w && f64::from(h) >= MIN_CARET_HEIGHT * scale;
        !caret || self.cells_changed_outside(other, cfg.cell_tolerance, Some((x0, x1)))
    }

    /// True if a cell's mean luma moved by more than `tolerance`, not counting the cells that
    /// overlap the pixel columns `skip` (a caret).
    fn cells_changed_outside(
        &self,
        other: &FrameSignature,
        tolerance: u8,
        skip: Option<(u32, u32)>,
    ) -> bool {
        self.cells
            .iter()
            .zip(&other.cells)
            .enumerate()
            .any(|(i, (a, b))| {
                let x = i as u32 % self.cols * CELL;
                let skipped = skip.is_some_and(|(x0, x1)| x <= x1 && x + CELL > x0);
                !skipped && a.abs_diff(*b) > tolerance
            })
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

    fn differs(a: &Frame, b: &Frame) -> bool {
        FrameSignature::of(a).differs(&FrameSignature::of(b), &ChangeConfig::default())
    }

    #[test]
    fn identical_frames_do_not_differ() {
        let a = Frame::solid(600, 300, [240, 240, 240]);
        assert!(!differs(&a, &a.clone()));
        assert_eq!(FrameSignature::of(&a).grid(), (75, 38));
    }

    #[test]
    fn a_changed_word_is_detected() {
        let base = Frame::solid(600, 300, [255, 255, 255]);
        let mut changed = base.clone();
        // A dark ~40×14 px word: covers ≥ 5 cells of 8×8.
        paint(&mut changed, 100, 100, 40, 14, [20, 20, 20]);
        assert!(differs(&base, &changed));
    }

    #[test]
    fn a_caret_is_ignored_at_every_alignment() {
        let base = Frame::solid(600, 300, [255, 255, 255]);
        for (w, h, scale) in [(1, 19, 1.0), (2, 19, 1.0), (2, 28, 1.5), (3, 38, 2.0)] {
            for off in 0..8 {
                let mut caret = base.clone();
                paint(&mut caret, 100 + off, 100 + off, w, h, [0, 0, 0]);
                let (a, b) = (
                    base.clone().with_origin(base.origin, scale),
                    caret.with_origin(base.origin, scale),
                );
                assert!(!differs(&a, &b), "{w}×{h} px at {scale}, offset {off}");
            }
        }
    }

    #[test]
    fn a_narrow_glyph_change_is_detected() {
        // A digit-sized change (5 px wide) in a lens of any size: no fraction hides it.
        for (w, h) in [(120, 48), (600, 300), (2400, 1200)] {
            let base = Frame::solid(w, h, [255, 255, 255]);
            let mut digit = base.clone();
            paint(&mut digit, 20, 20, 5, 2, [0, 0, 0]);
            paint(&mut digit, 20, 22, 2, 8, [0, 0, 0]);
            assert!(differs(&base, &digit), "{w}×{h}");
        }
    }

    #[test]
    fn a_caret_next_to_a_typed_letter_is_detected() {
        let base = Frame::solid(600, 300, [255, 255, 255]);
        let mut typed = base.clone();
        paint(&mut typed, 100, 104, 6, 10, [0, 0, 0]);
        paint(&mut typed, 108, 100, 1, 19, [0, 0, 0]);
        assert!(differs(&base, &typed));
    }

    #[test]
    fn a_short_vertical_change_is_not_a_caret() {
        // The dot of an `i` or a colon appearing: thin, but shorter than any caret.
        let base = Frame::solid(600, 300, [255, 255, 255]);
        let mut dots = base.clone();
        paint(&mut dots, 100, 100, 2, 6, [0, 0, 0]);
        assert!(differs(&base, &dots));
    }

    #[test]
    fn a_horizontal_line_is_not_a_caret() {
        let base = Frame::solid(600, 300, [255, 255, 255]);
        let mut underline = base.clone();
        paint(&mut underline, 100, 100, 12, 1, [0, 0, 0]);
        assert!(differs(&base, &underline));
    }

    #[test]
    fn a_fade_is_detected() {
        // Every pixel moves by less than the pixel tolerance, the cell means by more than theirs.
        let a = Frame::solid(320, 200, [200, 200, 200]);
        let b = Frame::solid(320, 200, [190, 190, 190]);
        assert!(differs(&a, &b));
    }

    #[test]
    fn size_change_is_full_change() {
        let a = FrameSignature::of(&Frame::solid(100, 100, [0, 0, 0]));
        let b = FrameSignature::of(&Frame::solid(100, 101, [0, 0, 0]));
        assert_eq!(a.changed_fraction(&b, 0), 1.0);
        assert!(a.differs(&b, &ChangeConfig::default()));
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

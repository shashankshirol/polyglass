use std::fmt;

use crate::{Error, PhysRect, Result};

/// Pixel layout of a [`Frame`]. All formats are 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// Blue, green, red, alpha (Windows Graphics Capture).
    Bgra8,
    /// Red, green, blue, alpha (image files).
    Rgba8,
    /// Single luminance channel.
    Gray8,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Bgra8 | PixelFormat::Rgba8 => 4,
            PixelFormat::Gray8 => 1,
        }
    }
}

/// One captured image of the lens region.
///
/// Frames live in memory only and are never written to disk (PRIV-02). The `Debug` impl
/// deliberately omits pixel data so frames can't leak into logs.
#[derive(Clone)]
pub struct Frame {
    width: u32,
    height: u32,
    stride: usize,
    format: PixelFormat,
    data: Vec<u8>,
    /// Where the frame was captured, in physical virtual-desktop pixels.
    pub origin: PhysRect,
    /// Scale factor of the monitor the frame came from (1.0 = 100%).
    pub scale_factor: f64,
}

impl Frame {
    /// Build a frame from raw pixel data. `stride` is the number of bytes per row.
    pub fn new(
        width: u32,
        height: u32,
        stride: usize,
        format: PixelFormat,
        data: Vec<u8>,
    ) -> Result<Self> {
        let min_stride = width as usize * format.bytes_per_pixel();
        if stride < min_stride {
            return Err(Error::InvalidInput(format!(
                "stride {stride} smaller than row size {min_stride}"
            )));
        }
        let needed = stride
            .checked_mul(height as usize)
            .ok_or_else(|| Error::InvalidInput("frame too large".into()))?;
        if data.len() < needed {
            return Err(Error::InvalidInput(format!(
                "frame buffer has {} bytes, need {needed}",
                data.len()
            )));
        }
        Ok(Self {
            width,
            height,
            stride,
            format,
            data,
            origin: PhysRect::new(0, 0, width, height),
            scale_factor: 1.0,
        })
    }

    /// A frame filled with one RGB colour (handy in tests).
    pub fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Self {
        let mut data = Vec::with_capacity(width as usize * height as usize * 4);
        for _ in 0..(width as usize * height as usize) {
            data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Self::new(width, height, width as usize * 4, PixelFormat::Rgba8, data)
            .expect("solid frame dimensions are consistent")
    }

    pub fn with_origin(mut self, origin: PhysRect, scale_factor: f64) -> Self {
        self.origin = origin;
        self.scale_factor = scale_factor;
        self
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Mutable pixel access (used by tests and by renderers that paint into frames).
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// RGB value at (`x`, `y`). Panics if out of bounds.
    pub fn rgb(&self, x: u32, y: u32) -> [u8; 3] {
        assert!(x < self.width && y < self.height, "pixel out of bounds");
        let bpp = self.format.bytes_per_pixel();
        let i = y as usize * self.stride + x as usize * bpp;
        let p = &self.data[i..i + bpp];
        match self.format {
            PixelFormat::Bgra8 => [p[2], p[1], p[0]],
            PixelFormat::Rgba8 => [p[0], p[1], p[2]],
            PixelFormat::Gray8 => [p[0], p[0], p[0]],
        }
    }

    /// Approximate luminance (0–255) at (`x`, `y`), using integer BT.601 weights.
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        let [r, g, b] = self.rgb(x, y);
        ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
    }

    /// Copy out the sub-rectangle (`x`, `y`, `w`, `h`), clamped to the frame.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Frame {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width - x);
        let h = h.min(self.height - y);
        let bpp = self.format.bytes_per_pixel();
        let row = w as usize * bpp;
        let mut data = Vec::with_capacity(row * h as usize);
        for r in 0..h as usize {
            let start = (y as usize + r) * self.stride + x as usize * bpp;
            data.extend_from_slice(&self.data[start..start + row]);
        }
        let mut out = Frame::new(w, h, row, self.format, data).expect("crop is in bounds");
        out.origin = PhysRect::new(self.origin.x + x as i32, self.origin.y + y as i32, w, h);
        out.scale_factor = self.scale_factor;
        out
    }
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .field("origin", &self.origin)
            .field("scale_factor", &self.scale_factor)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_buffer() {
        assert!(Frame::new(10, 10, 40, PixelFormat::Rgba8, vec![0; 399]).is_err());
        assert!(Frame::new(10, 10, 30, PixelFormat::Rgba8, vec![0; 400]).is_err());
    }

    #[test]
    fn bgra_and_rgba_channels() {
        let bgra = Frame::new(1, 1, 4, PixelFormat::Bgra8, vec![1, 2, 3, 255]).unwrap();
        assert_eq!(bgra.rgb(0, 0), [3, 2, 1]);
        let rgba = Frame::new(1, 1, 4, PixelFormat::Rgba8, vec![1, 2, 3, 255]).unwrap();
        assert_eq!(rgba.rgb(0, 0), [1, 2, 3]);
    }

    #[test]
    fn luma_extremes() {
        assert_eq!(Frame::solid(1, 1, [255, 255, 255]).luma(0, 0), 255);
        assert_eq!(Frame::solid(1, 1, [0, 0, 0]).luma(0, 0), 0);
    }

    #[test]
    fn crop_with_padding_stride() {
        // 2x2 RGBA with 4 bytes of row padding.
        let data = vec![
            1, 1, 1, 255, 2, 2, 2, 255, 0, 0, 0, 0, //
            3, 3, 3, 255, 4, 4, 4, 255, 0, 0, 0, 0,
        ];
        let f = Frame::new(2, 2, 12, PixelFormat::Rgba8, data).unwrap();
        let c = f.crop(1, 1, 5, 5);
        assert_eq!((c.width(), c.height()), (1, 1));
        assert_eq!(c.rgb(0, 0), [4, 4, 4]);
        assert_eq!(c.origin, PhysRect::new(1, 1, 1, 1));
    }

    #[test]
    fn debug_omits_pixels() {
        let f = Frame::solid(2, 2, [9, 9, 9]);
        let s = format!("{f:?}");
        assert!(!s.contains("data"));
    }
}

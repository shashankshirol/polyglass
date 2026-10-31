use std::path::Path;

use pg_core::{Error, Frame, PhysRect, PixelFormat, Result};

use crate::{ScreenCapturer, StaticCapturer};

/// Serves captures from a fixture image placed at the virtual-desktop origin.
#[derive(Debug, Clone)]
pub struct FixtureCapturer {
    inner: StaticCapturer,
}

impl FixtureCapturer {
    /// Load a PNG fixture file. `scale_factor` is the simulated monitor scale (CAP-04 tests).
    pub fn open(path: &Path, scale_factor: f64) -> Result<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| Error::InvalidInput(format!("cannot read fixture: {e}")))?;
        Self::from_png(&bytes, scale_factor)
    }

    /// Load a PNG fixture from memory, e.g. one rendered by `pg-testkit` (M0-FIX-2).
    pub fn from_png(png: &[u8], scale_factor: f64) -> Result<Self> {
        let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .map_err(|e| Error::InvalidInput(format!("cannot load fixture: {e}")))?
            .into_rgba8();
        let (w, h) = img.dimensions();
        let frame = Frame::new(w, h, w as usize * 4, PixelFormat::Rgba8, img.into_raw())?
            .with_origin(PhysRect::new(0, 0, w, h), scale_factor);
        Ok(Self {
            inner: StaticCapturer::new(frame),
        })
    }

    /// The full fixture area, e.g. to place the lens over the whole image.
    pub fn bounds(&self) -> PhysRect {
        self.inner.bounds()
    }
}

impl ScreenCapturer for FixtureCapturer {
    fn capture(&mut self, region: PhysRect) -> Result<Frame> {
        self.inner.capture(region)
    }

    fn excludes_self(&self) -> bool {
        true
    }
}

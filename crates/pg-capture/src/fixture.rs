use std::path::Path;

use pg_core::{Error, Frame, PhysRect, PixelFormat, Result};

use crate::{ScreenCapturer, StaticCapturer};

/// Serves captures from an image file placed at the virtual-desktop origin.
#[derive(Debug, Clone)]
pub struct FixtureCapturer {
    inner: StaticCapturer,
}

impl FixtureCapturer {
    /// Load a PNG fixture. `scale_factor` is the simulated monitor scale (CAP-04 tests).
    pub fn open(path: &Path, scale_factor: f64) -> Result<Self> {
        let img = image::open(path)
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

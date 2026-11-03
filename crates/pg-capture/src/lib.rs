//! Screen capture for Polyglass.
//!
//! Only the lens interior is captured (CAP-01). Platform implementations: [`WgcCapturer`]
//! (Windows Graphics Capture). Another platform's capturer is one more `#[cfg]` module
//! implementing [`ScreenCapturer`] (v1 is Windows only).

use pg_core::{Frame, PhysRect, Result};

#[cfg(feature = "fixture")]
mod fixture;
#[cfg(feature = "fixture")]
pub use fixture::FixtureCapturer;

// Direct3D 11 / DXGI interop and the monitor APIs are `unsafe fn` in windows-rs; there is no
// safe way to read WGC frames. The allowance is limited to this module.
#[cfg(windows)]
#[allow(unsafe_code)]
mod wgc;
#[cfg(windows)]
pub use wgc::{MonitorInfo, WgcCapturer, monitors};

pub trait ScreenCapturer: Send {
    /// Capture `region` (physical pixels), excluding Polyglass's own windows where the OS allows.
    fn capture(&mut self, region: PhysRect) -> Result<Frame>;

    /// Whether frames are guaranteed not to contain Polyglass windows (CAP-02).
    fn excludes_self(&self) -> bool;

    /// Free OS capture resources while the lens is hidden (LENS-06). The next `capture`
    /// acquires them again.
    fn release(&mut self) {}
}

impl<T: ScreenCapturer + ?Sized> ScreenCapturer for Box<T> {
    fn capture(&mut self, region: PhysRect) -> Result<Frame> {
        (**self).capture(region)
    }

    fn excludes_self(&self) -> bool {
        (**self).excludes_self()
    }

    fn release(&mut self) {
        (**self).release();
    }
}

/// A capturer backed by a fixed in-memory "desktop" frame. Regions outside the desktop are
/// clipped. Used by pipeline tests and as the base of [`FixtureCapturer`].
#[derive(Debug, Clone)]
pub struct StaticCapturer {
    desktop: Frame,
    captures: usize,
    releases: usize,
}

impl StaticCapturer {
    /// `desktop.origin` defines where the frame sits in virtual-desktop coordinates.
    pub fn new(desktop: Frame) -> Self {
        Self {
            desktop,
            captures: 0,
            releases: 0,
        }
    }

    /// Replace the desktop contents (simulates content changing under the lens).
    pub fn set_desktop(&mut self, desktop: Frame) {
        self.desktop = desktop;
    }

    /// The desktop area in virtual-desktop coordinates.
    pub fn bounds(&self) -> PhysRect {
        self.desktop.origin
    }

    /// Number of successful `capture` calls (used to assert parking, LENS-06).
    pub fn captures(&self) -> usize {
        self.captures
    }

    /// Number of `release` calls (used to assert that hiding frees capture resources).
    pub fn releases(&self) -> usize {
        self.releases
    }
}

impl ScreenCapturer for StaticCapturer {
    fn capture(&mut self, region: PhysRect) -> Result<Frame> {
        let bounds = self.desktop.origin;
        let Some(clip) = region.intersect(&bounds) else {
            return Err(pg_core::Error::Capture(
                "region is outside the desktop".into(),
            ));
        };
        let x = (clip.x - bounds.x) as u32;
        let y = (clip.y - bounds.y) as u32;
        self.captures += 1;
        Ok(self.desktop.crop(x, y, clip.width, clip.height))
    }

    fn excludes_self(&self) -> bool {
        true
    }

    fn release(&mut self) {
        self.releases += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_capture_crops_in_desktop_coordinates() {
        let desktop =
            Frame::solid(100, 100, [10, 20, 30]).with_origin(PhysRect::new(-50, 0, 100, 100), 1.0);
        let mut cap = StaticCapturer::new(desktop);
        let f = cap.capture(PhysRect::new(-60, 10, 30, 20)).unwrap();
        assert_eq!((f.width(), f.height()), (20, 20));
        assert_eq!(f.origin, PhysRect::new(-50, 10, 20, 20));
        assert_eq!(cap.captures(), 1);
        assert!(cap.capture(PhysRect::new(500, 500, 10, 10)).is_err());
    }
}

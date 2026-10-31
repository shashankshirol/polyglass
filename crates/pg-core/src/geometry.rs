use serde::{Deserialize, Serialize};

/// A rectangle in physical (device) pixels, in virtual-desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PhysRect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub const fn right(&self) -> i64 {
        self.x as i64 + self.width as i64
    }

    pub const fn bottom(&self) -> i64 {
        self.y as i64 + self.height as i64
    }

    /// Intersection with `other`, or `None` if they don't overlap.
    pub fn intersect(&self, other: &PhysRect) -> Option<PhysRect> {
        let x0 = i64::from(self.x.max(other.x));
        let y0 = i64::from(self.y.max(other.y));
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(PhysRect::new(
            x0 as i32,
            y0 as i32,
            (x1 - x0) as u32,
            (y1 - y0) as u32,
        ))
    }
}

/// A rectangle in logical (DPI-independent) pixels, as reported by the window system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl LogicalRect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Convert to physical pixels for a monitor with `scale` (1.0 = 100%, 1.25 = 125%, ...).
    ///
    /// The origin is floored and the far edge is ceiled so the physical rect always covers the
    /// whole logical area; this keeps text at the lens edge from being clipped at fractional
    /// scales (CAP-04).
    pub fn to_physical(&self, scale: f64) -> PhysRect {
        let x0 = (self.x * scale).floor();
        let y0 = (self.y * scale).floor();
        let x1 = ((self.x + self.width) * scale).ceil();
        let y1 = ((self.y + self.height) * scale).ceil();
        PhysRect::new(
            x0 as i32,
            y0 as i32,
            (x1 - x0).max(0.0) as u32,
            (y1 - y0).max(0.0) as u32,
        )
    }
}

/// A rectangle in frame-local pixel coordinates (used for OCR boxes).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }

    /// Smallest rect containing both.
    pub fn union(&self, other: &Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Length of the overlap of the two rects' horizontal extents (0 if disjoint).
    pub fn horizontal_overlap(&self, other: &Rect) -> f32 {
        (self.right().min(other.right()) - self.x.max(other.x)).max(0.0)
    }

    /// Length of the overlap of the two rects' vertical extents (0 if disjoint).
    pub fn vertical_overlap(&self, other: &Rect) -> f32 {
        (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0.0)
    }

    /// Scale all coordinates by `factor` (e.g. physical → logical with `1.0 / scale`).
    pub fn scaled(&self, factor: f32) -> Rect {
        Rect::new(
            self.x * factor,
            self.y * factor,
            self.w * factor,
            self.h * factor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_to_physical_at_common_scales() {
        let r = LogicalRect::new(100.0, 50.0, 480.0, 200.0);
        assert_eq!(r.to_physical(1.0), PhysRect::new(100, 50, 480, 200));
        assert_eq!(r.to_physical(1.25), PhysRect::new(125, 62, 600, 251));
        assert_eq!(r.to_physical(1.5), PhysRect::new(150, 75, 720, 300));
        assert_eq!(r.to_physical(2.0), PhysRect::new(200, 100, 960, 400));
    }

    #[test]
    fn logical_to_physical_covers_fractional_area() {
        let r = LogicalRect::new(0.3, 0.3, 10.0, 10.0);
        let p = r.to_physical(1.25);
        // Logical area spans 0.375..12.875 physical px.
        assert_eq!(p, PhysRect::new(0, 0, 13, 13));
    }

    #[test]
    fn negative_coordinates_on_left_monitor() {
        let r = LogicalRect::new(-1920.0, 0.0, 480.0, 200.0);
        assert_eq!(r.to_physical(1.5), PhysRect::new(-2880, 0, 720, 300));
    }

    #[test]
    fn phys_intersect() {
        let a = PhysRect::new(0, 0, 100, 100);
        let b = PhysRect::new(50, 60, 100, 100);
        assert_eq!(a.intersect(&b), Some(PhysRect::new(50, 60, 50, 40)));
        assert_eq!(a.intersect(&PhysRect::new(100, 0, 10, 10)), None);
    }

    #[test]
    fn rect_union_and_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 20.0, 10.0, 10.0);
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 30.0));
        assert_eq!(a.horizontal_overlap(&b), 5.0);
        assert_eq!(a.vertical_overlap(&b), 0.0);
    }
}

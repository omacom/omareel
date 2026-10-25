//! `QPointF`, `QSizeF` and `QRectF`: plain double geometry. Qt compares them fuzzily
//! (`qFuzzyCompare` per coordinate); `PartialEq` here is exact, so use `fuzzy_eq` where the
//! C++ used `==`.

use std::ops::{Add, Div, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointF {
    pub x: f64,
    pub y: f64,
}

impl PointF {
    pub const fn new(x: f64, y: f64) -> PointF {
        PointF { x, y }
    }
}

impl Add for PointF {
    type Output = PointF;
    fn add(self, o: PointF) -> PointF {
        PointF::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for PointF {
    type Output = PointF;
    fn sub(self, o: PointF) -> PointF {
        PointF::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for PointF {
    type Output = PointF;
    fn mul(self, k: f64) -> PointF {
        PointF::new(self.x * k, self.y * k)
    }
}

impl Div<f64> for PointF {
    type Output = PointF;
    fn div(self, k: f64) -> PointF {
        PointF::new(self.x / k, self.y / k)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SizeF {
    pub width: f64,
    pub height: f64,
}

impl SizeF {
    pub const fn new(width: f64, height: f64) -> SizeF {
        SizeF { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl RectF {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> RectF {
        RectF { x, y, width, height }
    }
}

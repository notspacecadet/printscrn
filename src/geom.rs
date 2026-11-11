//! Selection geometry shared by the overlay and the cropper.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub fn from_points(a: (f32, f32), b: (f32, f32)) -> Self {
        let x = a.0.min(b.0);
        let y = a.1.min(b.1);
        Self {
            x,
            y,
            w: (a.0 - b.0).abs(),
            h: (a.1 - b.1).abs(),
        }
    }

    pub fn is_empty(self) -> bool {
        self.w < 1.0 || self.h < 1.0
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersects(self, other: Self) -> bool {
        self.intersection(other).is_some()
    }

    pub fn intersection(self, other: Self) -> Option<Self> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 - x0 < 0.5 || y1 - y0 < 0.5 {
            None
        } else {
            Some(Self {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        }
    }

    pub fn area(self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }
}

/// A monitor placed in the overlay's point coordinate space.
///
/// `origin`/`size` are the overlay window's rectangle (logical points).
/// `image_w`/`image_h` are the frozen framebuffer in physical pixels.
/// Cropping uses the ratio of those two, so a Retina capture stays sharp
/// even when the reported scale factor and the window scale disagree.
#[derive(Clone, Debug)]
pub struct PlacedMonitor {
    pub name: String,
    pub origin_x: f32,
    pub origin_y: f32,
    pub size_w: f32,
    pub size_h: f32,
    pub image_w: u32,
    pub image_h: u32,
}

impl PlacedMonitor {
    pub fn rect(&self) -> RectF {
        RectF {
            x: self.origin_x,
            y: self.origin_y,
            w: self.size_w,
            h: self.size_h,
        }
    }

    pub fn scale_x(&self) -> f32 {
        if self.size_w <= 0.0 {
            1.0
        } else {
            self.image_w as f32 / self.size_w
        }
    }

    pub fn scale_y(&self) -> f32 {
        if self.size_h <= 0.0 {
            1.0
        } else {
            self.image_h as f32 / self.size_h
        }
    }
}

#[derive(Clone, Debug)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Which scale a crop should be rasterized at.
///
/// A selection that lives on one display uses that display's pixels exactly.
/// A selection that crosses displays uses the scale of the display that
/// contains most of the selection, so the result is not a blurry downsample
/// of a Retina screen.
pub fn output_scale(monitors: &[PlacedMonitor], sel: RectF) -> f32 {
    let mut best_area = -1.0;
    let mut scale = 1.0;
    for monitor in monitors {
        if let Some(inter) = sel.intersection(monitor.rect()) {
            let area = inter.area();
            if area > best_area {
                best_area = area;
                scale = monitor.scale_x().max(0.1);
            }
        }
    }
    scale
}

/// Map a global point into crop-pixel space for an already-chosen output scale.
pub fn to_crop_px(sel: RectF, scale: f32, x: f32, y: f32) -> (f32, f32) {
    ((x - sel.x) * scale, (y - sel.y) * scale)
}

pub fn constrain_line(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let adx = dx.abs();
    let ady = dy.abs();
    if adx < 0.001 && ady < 0.001 {
        return b;
    }
    if adx > ady * 2.4 {
        (b.0, a.1)
    } else if ady > adx * 2.4 {
        (a.0, b.1)
    } else {
        let len = adx.max(ady);
        (a.0 + dx.signum() * len, a.1 + dy.signum() * len)
    }
}

pub fn constrain_square(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let side = dx.abs().max(dy.abs());
    (a.0 + dx.signum() * side, a.1 + dy.signum() * side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_normalizes_drag_direction() {
        let r = RectF::from_points((40.0, 10.0), (10.0, 50.0));
        assert_eq!(
            r,
            RectF {
                x: 10.0,
                y: 10.0,
                w: 30.0,
                h: 40.0
            }
        );
    }

    #[test]
    fn intersection_rejects_a_gap() {
        let a = RectF {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let b = RectF {
            x: 12.0,
            y: 0.0,
            w: 5.0,
            h: 5.0,
        };
        assert!(a.intersection(b).is_none());
    }

    #[test]
    fn shift_constrains_to_axis_or_diagonal() {
        assert_eq!(constrain_line((0.0, 0.0), (30.0, 2.0)), (30.0, 0.0));
        assert_eq!(constrain_line((0.0, 0.0), (2.0, 30.0)), (0.0, 30.0));
        let (x, y) = constrain_line((0.0, 0.0), (10.0, 12.0));
        assert!((x - y).abs() < 0.01);
    }
}

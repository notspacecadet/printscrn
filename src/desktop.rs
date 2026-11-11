//! Frozen desktop: one physical framebuffer per monitor, cropped in the
//! overlay's point space so HiDPI stays 1:1 with the captured pixels.

use image::{Rgba, RgbaImage};

use crate::geom::{PlacedMonitor, RectF, output_scale};

#[derive(Clone, Debug)]
pub struct MonitorShot {
    pub name: String,
    /// Raw position reported by the capture backend (points on macOS, pixels on Windows).
    pub raw_x: i32,
    pub raw_y: i32,
    pub raw_w: u32,
    pub raw_h: u32,
    pub reported_scale: f32,
    pub image: RgbaImage,
    /// Overlay window origin in egui points. Updated once the window is placed.
    pub origin_x: f32,
    pub origin_y: f32,
    /// Overlay window size in egui points.
    pub point_w: f32,
    pub point_h: f32,
}

impl MonitorShot {
    pub fn placed(&self) -> PlacedMonitor {
        PlacedMonitor {
            name: self.name.clone(),
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            size_w: self.point_w,
            size_h: self.point_h,
            image_w: self.image.width(),
            image_h: self.image.height(),
        }
    }
}

/// Decide whether the backend reported logical points or physical pixels,
/// then express the monitor in egui points for the initial window placement.
///
/// macOS `CGDisplayBounds` is in points while the bitmap is in pixels, so the
/// bitmap is about `scale` times larger than the reported size.
/// Windows `DEVMODE` size matches the bitmap (physical pixels) and the scale
/// is separate.
pub fn initial_point_rect(
    raw_x: i32,
    raw_y: i32,
    raw_w: u32,
    raw_h: u32,
    reported_scale: f32,
    image_w: u32,
    image_h: u32,
) -> (f32, f32, f32, f32) {
    let scale = if reported_scale.is_finite() && reported_scale > 0.1 {
        reported_scale
    } else {
        1.0
    };
    let reported_is_logical = raw_w > 0
        && image_w > raw_w.saturating_mul(11) / 10
        && raw_h > 0
        && image_h > raw_h.saturating_mul(11) / 10;
    if reported_is_logical {
        (raw_x as f32, raw_y as f32, raw_w as f32, raw_h as f32)
    } else {
        (
            raw_x as f32 / scale,
            raw_y as f32 / scale,
            (raw_w as f32 / scale).max(1.0),
            (raw_h as f32 / scale).max(1.0),
        )
    }
}

/// Keep monitors from overlapping in the overlay's point space when they run
/// at different scale factors.
///
/// Windows reports positions in physical pixels and [`initial_point_rect`]
/// divides each monitor's position by that monitor's own scale. With one
/// scale for every monitor that is exact. With mixed scales the divided
/// rectangles can land on top of each other: a 1920-wide display at 100% next
/// to a display at 200% puts the second one at x = 960, inside the first.
/// A selection on one display would then also be cropped from the other.
///
/// Physical rectangles never overlap, and a monitor's point size is never
/// larger than its pixel size, so anchoring each monitor at its physical
/// position keeps them apart. Backends that already report points (macOS)
/// are left alone.
pub fn separate_mixed_scale_origins(shots: &mut [MonitorShot]) {
    let scale = |shot: &MonitorShot| {
        if shot.point_w > 0.0 {
            shot.image.width() as f32 / shot.point_w
        } else {
            1.0
        }
    };
    let Some(first) = shots.first().map(scale) else {
        return;
    };
    if shots.iter().all(|shot| (scale(shot) - first).abs() < 0.01) {
        return;
    }
    for shot in shots.iter_mut() {
        let reported_physical = shot.raw_w > 0 && shot.image.width() <= shot.raw_w * 11 / 10;
        if reported_physical {
            shot.origin_x = shot.raw_x as f32;
            shot.origin_y = shot.raw_y as f32;
        }
    }
}

pub fn crop(monitors: &[MonitorShot], sel: RectF) -> RgbaImage {
    let placed: Vec<PlacedMonitor> = monitors.iter().map(MonitorShot::placed).collect();
    crop_placed(monitors, &placed, sel)
}

pub fn crop_placed(monitors: &[MonitorShot], placed: &[PlacedMonitor], sel: RectF) -> RgbaImage {
    let scale = output_scale(placed, sel).max(0.1);
    let out_w = (sel.w * scale).round().max(1.0) as u32;
    let out_h = (sel.h * scale).round().max(1.0) as u32;
    let mut out = RgbaImage::from_pixel(out_w, out_h, Rgba([0, 0, 0, 255]));

    for (monitor, place) in monitors.iter().zip(placed.iter()) {
        let Some(inter) = sel.intersection(place.rect()) else {
            continue;
        };
        let sx0 = ((inter.x - place.origin_x) * place.scale_x())
            .round()
            .max(0.0) as i64;
        let sy0 = ((inter.y - place.origin_y) * place.scale_y())
            .round()
            .max(0.0) as i64;
        let sx1 = ((inter.right() - place.origin_x) * place.scale_x()).round() as i64;
        let sy1 = ((inter.bottom() - place.origin_y) * place.scale_y()).round() as i64;
        let src_x = sx0.clamp(0, monitor.image.width() as i64) as u32;
        let src_y = sy0.clamp(0, monitor.image.height() as i64) as u32;
        let src_r = sx1.clamp(0, monitor.image.width() as i64) as u32;
        let src_b = sy1.clamp(0, monitor.image.height() as i64) as u32;
        if src_r <= src_x || src_b <= src_y {
            continue;
        }
        let src_w = src_r - src_x;
        let src_h = src_b - src_y;

        let dx = ((inter.x - sel.x) * scale).round().max(0.0) as u32;
        let dy = ((inter.y - sel.y) * scale).round().max(0.0) as u32;
        let dw = (inter.w * scale).round().max(1.0) as u32;
        let dh = (inter.h * scale).round().max(1.0) as u32;
        blit(
            &monitor.image,
            src_x,
            src_y,
            src_w,
            src_h,
            &mut out,
            dx,
            dy,
            dw,
            dh,
        );
    }
    out
}

fn blit(
    src: &RgbaImage,
    sx: u32,
    sy: u32,
    sw: u32,
    sh: u32,
    dst: &mut RgbaImage,
    dx: u32,
    dy: u32,
    dw: u32,
    dh: u32,
) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return;
    }
    for row in 0..dh {
        let dst_y = dy + row;
        if dst_y >= dst.height() {
            break;
        }
        let src_y = sy + ((row as u64 * sh as u64) / dh as u64) as u32;
        if src_y >= src.height() {
            break;
        }
        for col in 0..dw {
            let dst_x = dx + col;
            if dst_x >= dst.width() {
                break;
            }
            let src_x = sx + ((col as u64 * sw as u64) / dw as u64) as u32;
            if src_x >= src.width() {
                break;
            }
            dst.put_pixel(dst_x, dst_y, *src.get_pixel(src_x, src_y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(color))
    }

    fn shot(origin: (f32, f32), points: (f32, f32), image: RgbaImage) -> MonitorShot {
        MonitorShot {
            name: "test".into(),
            raw_x: origin.0 as i32,
            raw_y: origin.1 as i32,
            raw_w: points.0 as u32,
            raw_h: points.1 as u32,
            reported_scale: image.width() as f32 / points.0,
            image,
            origin_x: origin.0,
            origin_y: origin.1,
            point_w: points.0,
            point_h: points.1,
        }
    }

    #[test]
    fn macos_geometry_stays_in_points() {
        let (x, y, w, h) = initial_point_rect(1920, 0, 1512, 982, 2.0, 3024, 1964);
        assert_eq!((x, y, w, h), (1920.0, 0.0, 1512.0, 982.0));
    }

    #[test]
    fn windows_physical_geometry_converts_with_scale() {
        let (x, y, w, h) = initial_point_rect(3840, 0, 2560, 1440, 1.25, 2560, 1440);
        assert!((x - 3072.0).abs() < 0.01, "{x}");
        assert!((w - 2048.0).abs() < 0.01, "{w}");
        assert!((h - 1152.0).abs() < 0.01, "{h}");
        assert_eq!(y, 0.0);
    }

    #[test]
    fn mixed_scale_monitors_do_not_overlap_in_point_space() {
        // 1920x1080 at 100% on the left, 3840x2160 at 200% to its right.
        let mut shots = vec![
            MonitorShot {
                name: "left".into(),
                raw_x: 0,
                raw_y: 0,
                raw_w: 1920,
                raw_h: 1080,
                reported_scale: 1.0,
                image: solid(1920, 1080, [1, 1, 1, 255]),
                origin_x: 0.0,
                origin_y: 0.0,
                point_w: 1920.0,
                point_h: 1080.0,
            },
            MonitorShot {
                name: "right".into(),
                raw_x: 1920,
                raw_y: 0,
                raw_w: 3840,
                raw_h: 2160,
                reported_scale: 2.0,
                image: solid(3840, 2160, [2, 2, 2, 255]),
                origin_x: 960.0,
                origin_y: 0.0,
                point_w: 1920.0,
                point_h: 1080.0,
            },
        ];
        assert!(shots[0].placed().rect().intersects(shots[1].placed().rect()));
        separate_mixed_scale_origins(&mut shots);
        assert!(!shots[0].placed().rect().intersects(shots[1].placed().rect()));
        assert_eq!(shots[1].origin_x, 1920.0);
        // A selection on the left display takes pixels from the left display only.
        let cropped = crop(
            &shots,
            RectF {
                x: 1000.0,
                y: 10.0,
                w: 50.0,
                h: 20.0,
            },
        );
        assert_eq!((cropped.width(), cropped.height()), (50, 20));
        assert_eq!(cropped.get_pixel(25, 10).0, [1, 1, 1, 255]);
    }

    #[test]
    fn same_scale_monitors_keep_their_contiguous_layout() {
        let mut shots = vec![
            shot((0.0, 0.0), (1280.0, 720.0), solid(2560, 1440, [1, 1, 1, 255])),
            shot((1280.0, 0.0), (1280.0, 720.0), solid(2560, 1440, [2, 2, 2, 255])),
        ];
        separate_mixed_scale_origins(&mut shots);
        assert_eq!(shots[1].origin_x, 1280.0);
    }

    #[test]
    fn linux_one_to_one_is_unchanged() {
        let rect = initial_point_rect(0, 0, 1920, 1200, 1.0, 1920, 1200);
        assert_eq!(rect, (0.0, 0.0, 1920.0, 1200.0));
    }

    #[test]
    fn hidpi_crop_is_physical_and_exact() {
        let mut image = solid(200, 100, [0, 0, 0, 255]);
        // Logical 100×50 at 2x. Paint a 2×2 physical block at logical (10, 10).
        image.put_pixel(20, 20, Rgba([10, 20, 30, 255]));
        image.put_pixel(21, 20, Rgba([11, 21, 31, 255]));
        image.put_pixel(20, 21, Rgba([12, 22, 32, 255]));
        image.put_pixel(21, 21, Rgba([13, 23, 33, 255]));
        let monitor = shot((0.0, 0.0), (100.0, 50.0), image);
        let cropped = crop(
            &[monitor],
            RectF {
                x: 10.0,
                y: 10.0,
                w: 4.0,
                h: 3.0,
            },
        );
        assert_eq!((cropped.width(), cropped.height()), (8, 6));
        assert_eq!(cropped.get_pixel(0, 0).0, [10, 20, 30, 255]);
        assert_eq!(cropped.get_pixel(1, 0).0, [11, 21, 31, 255]);
        assert_eq!(cropped.get_pixel(0, 1).0, [12, 22, 32, 255]);
    }

    #[test]
    fn crop_spans_two_monitors_in_order() {
        let left = shot((0.0, 0.0), (100.0, 40.0), solid(100, 40, [255, 0, 0, 255]));
        let right = shot((100.0, 0.0), (80.0, 40.0), solid(80, 40, [0, 0, 255, 255]));
        let cropped = crop(
            &[left, right],
            RectF {
                x: 90.0,
                y: 0.0,
                w: 20.0,
                h: 10.0,
            },
        );
        assert_eq!(cropped.width(), 20);
        assert_eq!(cropped.get_pixel(0, 0).0[0], 255);
        assert_eq!(cropped.get_pixel(10, 0).0[2], 255);
        assert_eq!(cropped.get_pixel(19, 0).0[2], 255);
    }

    #[test]
    fn crop_outside_monitors_is_blank_but_sized() {
        let monitor = shot((0.0, 0.0), (50.0, 50.0), solid(50, 50, [1, 2, 3, 255]));
        let cropped = crop(
            &[monitor],
            RectF {
                x: 200.0,
                y: 200.0,
                w: 8.0,
                h: 4.0,
            },
        );
        assert_eq!((cropped.width(), cropped.height()), (8, 4));
        assert_eq!(cropped.get_pixel(0, 0).0, [0, 0, 0, 255]);
    }
}

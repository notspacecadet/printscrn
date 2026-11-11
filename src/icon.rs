//! Tray and window icon, drawn in code so the binary does not depend on a
//! loose PNG at runtime.

use image::{Rgba, RgbaImage};

pub fn icon_image(size: u32) -> RgbaImage {
    let mut image = RgbaImage::from_pixel(size, size, Rgba([0, 0, 0, 0]));
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let inset = s * 0.08;
            if !in_round_rect(px, py, inset, inset, s - inset, s - inset, s * 0.22) {
                continue;
            }
            image.put_pixel(x, y, Rgba([23, 24, 28, 255]));
            let bracket = bracket_mask(px, py, s);
            if bracket {
                image.put_pixel(x, y, Rgba([255, 77, 46, 255]));
            }
        }
    }
    image
}

fn in_round_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> bool {
    let cx = px.clamp(x0 + radius, x1 - radius);
    let cy = py.clamp(y0 + radius, y1 - radius);
    let dx = px - cx;
    let dy = py - cy;
    dx * dx + dy * dy <= radius * radius && px >= x0 && px <= x1 && py >= y0 && py <= y1
}

fn bracket_mask(px: f32, py: f32, s: f32) -> bool {
    let m = s * 0.28;
    let t = (s * 0.075).max(1.5);
    let arm = s * 0.22;
    let corners = [(m, m), (s - m, m), (m, s - m), (s - m, s - m)];
    for (cx, cy) in corners {
        let dx = (px - cx).abs();
        let dy = (py - cy).abs();
        let on_vert = dx <= t && dy <= arm;
        let on_horz = dy <= t && dx <= arm;
        if on_vert || on_horz {
            return true;
        }
    }
    false
}

pub fn icon_rgba(size: u32) -> (Vec<u8>, u32, u32) {
    let image = icon_image(size);
    let (w, h) = image.dimensions();
    (image.into_raw(), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_transparent_corners_and_a_coral_mark() {
        let image = icon_image(64);
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        let mut coral = 0u32;
        for pixel in image.pixels() {
            if pixel.0[0] > 200 && pixel.0[1] < 120 && pixel.0[3] == 255 {
                coral += 1;
            }
        }
        assert!(
            coral > 40,
            "expected the selection brackets, counted {coral}"
        );
    }
}

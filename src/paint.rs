//! Software rasterizer used to bake annotations into the PNG.
//!
//! The overlay previews shapes with the GPU, then this path produces the
//! pixels that actually land on the clipboard, so copy matches the marks.

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use std::sync::OnceLock;

use crate::annotate::{Annotation, Color, Pt, Shape};

const FONT_BYTES: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");

fn font() -> &'static FontArc {
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| FontArc::try_from_slice(FONT_BYTES).expect("Inter font is valid"))
}

pub fn render(base: &RgbaImage, annotations: &[Annotation]) -> RgbaImage {
    let mut canvas = base.clone();
    for annotation in annotations {
        draw_annotation(&mut canvas, annotation);
    }
    canvas
}

fn draw_annotation(canvas: &mut RgbaImage, annotation: &Annotation) {
    match &annotation.shape {
        Shape::Pen { points } => {
            stroke_polyline(
                canvas,
                points,
                annotation.stroke.max(1.0) * 0.5,
                annotation.color,
                false,
            );
        }
        Shape::Marker { points } => {
            let width = (annotation.stroke.max(2.0) * 3.2).max(14.0);
            let color = annotation.color.with_alpha(120);
            stroke_polyline(canvas, points, width * 0.5, color, true);
        }
        Shape::Line { a, b } => {
            stroke_segment(
                canvas,
                *a,
                *b,
                annotation.stroke.max(1.0) * 0.5,
                annotation.color,
            );
        }
        Shape::Arrow { a, b } => {
            draw_arrow(canvas, *a, *b, annotation.stroke.max(1.0), annotation.color)
        }
        Shape::Rect { a, b } => {
            stroke_rect(
                canvas,
                *a,
                *b,
                annotation.stroke.max(1.0) * 0.5,
                annotation.color,
            );
        }
        Shape::Ellipse { a, b } => {
            stroke_ellipse(
                canvas,
                *a,
                *b,
                annotation.stroke.max(1.0) * 0.5,
                annotation.color,
            );
        }
        Shape::Text { at, text, size } => draw_text(canvas, *at, text, *size, annotation.color),
        Shape::Blur { a, b, radius } => blur_rect(canvas, *a, *b, *radius),
        Shape::Pixelate { a, b, block } => pixelate_rect(canvas, *a, *b, *block),
    }
}

fn stroke_polyline(
    canvas: &mut RgbaImage,
    points: &[Pt],
    radius: f32,
    color: Color,
    highlight: bool,
) {
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        stamp_disc(canvas, points[0], radius, color, highlight);
        return;
    }
    for pair in points.windows(2) {
        stroke_segment(canvas, pair[0], pair[1], radius, color);
        if highlight {
            // Highlighter re-stamps with the translucent color. `stroke_segment`
            // already blends, so a second pass would darken joints. Joints are
            // covered because each capsule includes its round caps.
        }
    }
    let _ = highlight;
}

fn stroke_segment(canvas: &mut RgbaImage, a: Pt, b: Pt, radius: f32, color: Color) {
    let (min_x, min_y, max_x, max_y) = bounds_2(a, b, radius + 1.0);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dist = dist_to_segment(x as f32 + 0.5, y as f32 + 0.5, a, b);
            let cover = coverage(dist, radius);
            if cover > 0.0 {
                blend(canvas, x, y, color, cover);
            }
        }
    }
}

fn stamp_disc(canvas: &mut RgbaImage, center: Pt, radius: f32, color: Color, _highlight: bool) {
    stroke_segment(canvas, center, center, radius, color);
}

fn stroke_rect(canvas: &mut RgbaImage, a: Pt, b: Pt, radius: f32, color: Color) {
    let (x0, y0, x1, y1) = norm_rect(a, b);
    let (min_x, min_y, max_x, max_y) = bounds_rect(x0, y0, x1, y1, radius + 1.0);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dist = dist_to_rect_edge(x as f32 + 0.5, y as f32 + 0.5, x0, y0, x1, y1);
            let cover = coverage(dist, radius);
            if cover > 0.0 {
                blend(canvas, x, y, color, cover);
            }
        }
    }
}

fn stroke_ellipse(canvas: &mut RgbaImage, a: Pt, b: Pt, radius: f32, color: Color) {
    let (x0, y0, x1, y1) = norm_rect(a, b);
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let rx = ((x1 - x0) * 0.5).max(0.5);
    let ry = ((y1 - y0) * 0.5).max(0.5);
    let (min_x, min_y, max_x, max_y) = bounds_rect(x0, y0, x1, y1, radius + 1.5);
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5 - cx;
            let py = y as f32 + 0.5 - cy;
            let nx = px / rx;
            let ny = py / ry;
            let f = nx * nx + ny * ny - 1.0;
            let gx = 2.0 * px / (rx * rx);
            let gy = 2.0 * py / (ry * ry);
            let g = (gx * gx + gy * gy).sqrt().max(0.0001);
            let dist = (f / g).abs();
            let cover = coverage(dist, radius);
            if cover > 0.0 {
                blend(canvas, x, y, color, cover);
            }
        }
    }
}

fn draw_arrow(canvas: &mut RgbaImage, a: Pt, b: Pt, stroke: f32, color: Color) {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let ux = dx / len;
    let uy = dy / len;
    let head = (stroke * 3.4 + 12.0).min(len * 0.5).max(8.0);
    let base = Pt {
        x: b.x - ux * head,
        y: b.y - uy * head,
    };
    stroke_segment(canvas, a, base, stroke * 0.5, color);
    let px = -uy;
    let py = ux;
    let half = head * 0.42;
    let left = Pt {
        x: base.x + px * half,
        y: base.y + py * half,
    };
    let right = Pt {
        x: base.x - px * half,
        y: base.y - py * half,
    };
    fill_triangle(canvas, b, left, right, color);
}

fn fill_triangle(canvas: &mut RgbaImage, a: Pt, b: Pt, c: Pt, color: Color) {
    let min_x = a.x.min(b.x).min(c.x).floor() as i32 - 1;
    let min_y = a.y.min(b.y).min(c.y).floor() as i32 - 1;
    let max_x = a.x.max(b.x).max(c.x).ceil() as i32 + 1;
    let max_y = a.y.max(b.y).max(c.y).ceil() as i32 + 1;
    let area = edge(a, b, c);
    if area.abs() < 0.001 {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = Pt {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            };
            let w0 = edge(b, c, p);
            let w1 = edge(c, a, p);
            let w2 = edge(a, b, p);
            if w0.signum() == area.signum()
                && w1.signum() == area.signum()
                && w2.signum() == area.signum()
                || w0 == 0.0
                || w1 == 0.0
                || w2 == 0.0
            {
                // Accept the interior. A zero barycentric coordinate is the edge.
                if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0) {
                    blend(canvas, x, y, color, 1.0);
                }
            }
        }
    }
}

fn edge(a: Pt, b: Pt, c: Pt) -> f32 {
    (c.x - a.x) * (b.y - a.y) - (c.y - a.y) * (b.x - a.x)
}

fn draw_text(canvas: &mut RgbaImage, at: Pt, text: &str, size: f32, color: Color) {
    let font = font();
    let size = size.clamp(8.0, 256.0);
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();
    let mut pen_x = at.x;
    let baseline = at.y + ascent;
    for ch in text.chars() {
        let glyph = font
            .glyph_id(ch)
            .with_scale_and_position(scale, ab_glyph::point(pen_x, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, cov| {
                let px = bounds.min.x as i32 + x as i32;
                let py = bounds.min.y as i32 + y as i32;
                if cov > 0.02 {
                    blend(canvas, px, py, color, cov);
                }
            });
        }
        pen_x += scaled.h_advance(font.glyph_id(ch));
    }
}

fn blur_rect(canvas: &mut RgbaImage, a: Pt, b: Pt, radius: f32) {
    let (x0, y0, x1, y1) = clamped_rect(canvas, a, b);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let radius = radius.round().clamp(2.0, 40.0) as i32;
    let rw = (x1 - x0) as usize;
    let rh = (y1 - y0) as usize;
    let mut src = vec![0u8; rw * rh * 4];
    for y in 0..rh {
        for x in 0..rw {
            let pixel = canvas
                .get_pixel(x0 as u32 + x as u32, y0 as u32 + y as u32)
                .0;
            let i = (y * rw + x) * 4;
            src[i..i + 4].copy_from_slice(&pixel);
        }
    }
    let mut tmp = vec![0u8; src.len()];
    for _ in 0..2 {
        box_blur(&src, &mut tmp, rw, rh, radius, true);
        box_blur(&tmp, &mut src, rw, rh, radius, false);
    }
    for y in 0..rh {
        for x in 0..rw {
            let i = (y * rw + x) * 4;
            canvas.put_pixel(
                x0 as u32 + x as u32,
                y0 as u32 + y as u32,
                Rgba([src[i], src[i + 1], src[i + 2], 255]),
            );
        }
    }
}

fn box_blur(src: &[u8], dst: &mut [u8], w: usize, h: usize, radius: i32, horizontal: bool) {
    let r = radius as isize;
    let diam = (radius * 2 + 1) as u32;
    if horizontal {
        for y in 0..h {
            let mut sum = [0u32; 4];
            let sample = |x: isize| {
                let xx = x.clamp(0, w as isize - 1) as usize;
                let i = (y * w + xx) * 4;
                [
                    src[i] as u32,
                    src[i + 1] as u32,
                    src[i + 2] as u32,
                    src[i + 3] as u32,
                ]
            };
            for k in -r..=r {
                let p = sample(k);
                for c in 0..4 {
                    sum[c] += p[c];
                }
            }
            for x in 0..w {
                let i = (y * w + x) * 4;
                for c in 0..4 {
                    dst[i + c] = (sum[c] / diam) as u8;
                }
                let remove = sample(x as isize - r);
                let add = sample(x as isize + r + 1);
                for c in 0..4 {
                    sum[c] = sum[c] + add[c] - remove[c];
                }
            }
        }
    } else {
        for x in 0..w {
            let mut sum = [0u32; 4];
            let sample = |y: isize| {
                let yy = y.clamp(0, h as isize - 1) as usize;
                let i = (yy * w + x) * 4;
                [
                    src[i] as u32,
                    src[i + 1] as u32,
                    src[i + 2] as u32,
                    src[i + 3] as u32,
                ]
            };
            for k in -r..=r {
                let p = sample(k);
                for c in 0..4 {
                    sum[c] += p[c];
                }
            }
            for y in 0..h {
                let i = (y * w + x) * 4;
                for c in 0..4 {
                    dst[i + c] = (sum[c] / diam) as u8;
                }
                let remove = sample(y as isize - r);
                let add = sample(y as isize + r + 1);
                for c in 0..4 {
                    sum[c] = sum[c] + add[c] - remove[c];
                }
            }
        }
    }
}

fn pixelate_rect(canvas: &mut RgbaImage, a: Pt, b: Pt, block: f32) {
    let (x0, y0, x1, y1) = clamped_rect(canvas, a, b);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let block = block.round().clamp(2.0, 80.0) as u32;
    let mut y = y0;
    while y < y1 {
        let bh = (y1 - y).min(block as i32) as u32;
        let mut x = x0;
        while x < x1 {
            let bw = (x1 - x).min(block as i32) as u32;
            let mut acc = [0u32; 3];
            let mut count = 0u32;
            for yy in 0..bh {
                for xx in 0..bw {
                    let p = canvas.get_pixel(x as u32 + xx, y as u32 + yy).0;
                    acc[0] += p[0] as u32;
                    acc[1] += p[1] as u32;
                    acc[2] += p[2] as u32;
                    count += 1;
                }
            }
            let color = Rgba([
                (acc[0] / count) as u8,
                (acc[1] / count) as u8,
                (acc[2] / count) as u8,
                255,
            ]);
            for yy in 0..bh {
                for xx in 0..bw {
                    canvas.put_pixel(x as u32 + xx, y as u32 + yy, color);
                }
            }
            x += block as i32;
        }
        y += block as i32;
    }
}

fn blend(canvas: &mut RgbaImage, x: i32, y: i32, color: Color, cover: f32) {
    if x < 0 || y < 0 || x >= canvas.width() as i32 || y >= canvas.height() as i32 {
        return;
    }
    let dst = canvas.get_pixel(x as u32, y as u32).0;
    let alpha = (color.a as f32 / 255.0) * cover.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let inv = 1.0 - alpha;
    let mix = |s: u8, d: u8| -> u8 { (s as f32 * alpha + d as f32 * inv).round() as u8 };
    canvas.put_pixel(
        x as u32,
        y as u32,
        Rgba([
            mix(color.r, dst[0]),
            mix(color.g, dst[1]),
            mix(color.b, dst[2]),
            255,
        ]),
    );
}

fn coverage(dist: f32, radius: f32) -> f32 {
    // Full ink inside the stroke, one pixel of antialiasing on the outer edge.
    let outer = radius + 0.5;
    let inner = (radius - 0.5).max(0.0);
    if dist <= inner {
        1.0
    } else if dist >= outer {
        0.0
    } else {
        (outer - dist) / (outer - inner).max(0.001)
    }
}

fn dist_to_segment(px: f32, py: f32, a: Pt, b: Pt) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    let t = if len2 < 0.0001 {
        0.0
    } else {
        (((px - a.x) * dx + (py - a.y) * dy) / len2).clamp(0.0, 1.0)
    };
    let cx = a.x + t * dx;
    let cy = a.y + t * dy;
    let ex = px - cx;
    let ey = py - cy;
    (ex * ex + ey * ey).sqrt()
}

fn dist_to_rect_edge(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let outside_x = if px < x0 {
        x0 - px
    } else if px > x1 {
        px - x1
    } else {
        0.0
    };
    let outside_y = if py < y0 {
        y0 - py
    } else if py > y1 {
        py - y1
    } else {
        0.0
    };
    if outside_x == 0.0 && outside_y == 0.0 {
        (px - x0).min(x1 - px).min(py - y0).min(y1 - py)
    } else if outside_x == 0.0 {
        outside_y
    } else if outside_y == 0.0 {
        outside_x
    } else {
        (outside_x * outside_x + outside_y * outside_y).sqrt()
    }
}

fn norm_rect(a: Pt, b: Pt) -> (f32, f32, f32, f32) {
    (a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

fn clamped_rect(canvas: &RgbaImage, a: Pt, b: Pt) -> (i32, i32, i32, i32) {
    let (x0, y0, x1, y1) = norm_rect(a, b);
    let x0 = x0.floor().max(0.0) as i32;
    let y0 = y0.floor().max(0.0) as i32;
    let x1 = x1.ceil().min(canvas.width() as f32) as i32;
    let y1 = y1.ceil().min(canvas.height() as f32) as i32;
    (x0, y0, x1.max(x0), y1.max(y0))
}

fn bounds_2(a: Pt, b: Pt, pad: f32) -> (i32, i32, i32, i32) {
    (
        (a.x.min(b.x) - pad).floor() as i32,
        (a.y.min(b.y) - pad).floor() as i32,
        (a.x.max(b.x) + pad).ceil() as i32,
        (a.y.max(b.y) + pad).ceil() as i32,
    )
}

fn bounds_rect(x0: f32, y0: f32, x1: f32, y1: f32, pad: f32) -> (i32, i32, i32, i32) {
    (
        (x0 - pad).floor() as i32,
        (y0 - pad).floor() as i32,
        (x1 + pad).ceil() as i32,
        (y1 + pad).ceil() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotate::{Annotation, Color, Pt, Shape};
    use image::{Rgba, RgbaImage};

    fn canvas() -> RgbaImage {
        RgbaImage::from_pixel(80, 60, Rgba([240, 240, 240, 255]))
    }

    fn differs(base: &RgbaImage, painted: &RgbaImage) -> bool {
        base.as_raw() != painted.as_raw()
    }

    #[test]
    fn pen_marks_a_pixel() {
        let base = canvas();
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Pen {
                    points: vec![Pt { x: 10.0, y: 10.0 }, Pt { x: 40.0, y: 10.0 }],
                },
                color: Color::rgb(0, 0, 0),
                stroke: 3.0,
            }],
        );
        assert!(differs(&base, &painted));
        assert_eq!(painted.get_pixel(20, 10).0[0], 0);
        assert_eq!(
            painted.get_pixel(20, 40).0[0],
            240,
            "far from the stroke stays untouched"
        );
    }

    #[test]
    fn rect_is_an_outline() {
        let base = canvas();
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Rect {
                    a: Pt { x: 10.0, y: 10.0 },
                    b: Pt { x: 50.0, y: 40.0 },
                },
                color: Color::rgb(0, 0, 0),
                stroke: 2.0,
            }],
        );
        assert!(
            painted.get_pixel(30, 10).0[0] < 80,
            "top edge should be ink"
        );
        assert!(
            painted.get_pixel(30, 25).0[0] > 200,
            "center of the box stays empty"
        );
    }

    #[test]
    fn marker_is_translucent() {
        let base = canvas();
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Marker {
                    points: vec![Pt { x: 8.0, y: 30.0 }, Pt { x: 70.0, y: 30.0 }],
                },
                color: Color::rgb(255, 214, 10),
                stroke: 8.0,
            }],
        );
        let pixel = painted.get_pixel(40, 30).0;
        assert!(
            pixel[0] > 240,
            "yellow highlighter stays light on white, got {pixel:?}"
        );
        assert!(
            pixel[2] < 230,
            "blue channel drops, so it is not the original gray"
        );
        assert!(pixel[1] > pixel[2]);
    }

    #[test]
    fn arrow_head_spreads_off_the_shaft() {
        let base = canvas();
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Arrow {
                    a: Pt { x: 8.0, y: 30.0 },
                    b: Pt { x: 60.0, y: 30.0 },
                },
                color: Color::rgb(200, 0, 0),
                stroke: 2.0,
            }],
        );
        let mut found = false;
        for y in 20..29 {
            for x in 48..60 {
                let pixel = painted.get_pixel(x, y).0;
                if pixel[0] > 150 && pixel[1] < 80 {
                    found = true;
                }
            }
        }
        assert!(found, "arrow head should paint above the shaft");
    }

    #[test]
    fn blur_changes_only_the_region() {
        let mut base = canvas();
        for x in 0..80 {
            base.put_pixel(x, 10, Rgba([0, 0, 0, 255]));
            base.put_pixel(x, 12, Rgba([255, 255, 255, 255]));
        }
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Blur {
                    a: Pt { x: 20.0, y: 4.0 },
                    b: Pt { x: 50.0, y: 20.0 },
                    radius: 3.0,
                },
                color: Color::rgb(0, 0, 0),
                stroke: 1.0,
            }],
        );
        let inside = painted.get_pixel(30, 11).0[0];
        assert!(
            inside > 20 && inside < 235,
            "blur mixes the stripe, got {inside}"
        );
        assert_eq!(
            painted.get_pixel(5, 10).0[0],
            0,
            "outside the rect is unchanged"
        );
    }

    #[test]
    fn pixelate_makes_a_uniform_block() {
        let mut base = canvas();
        base.put_pixel(10, 10, Rgba([0, 0, 0, 255]));
        base.put_pixel(14, 14, Rgba([255, 0, 0, 255]));
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Pixelate {
                    a: Pt { x: 8.0, y: 8.0 },
                    b: Pt { x: 24.0, y: 24.0 },
                    block: 8.0,
                },
                color: Color::rgb(0, 0, 0),
                stroke: 1.0,
            }],
        );
        assert_eq!(painted.get_pixel(8, 8).0, painted.get_pixel(15, 15).0);
    }

    #[test]
    fn text_paints_ink() {
        let base = canvas();
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Text {
                    at: Pt { x: 4.0, y: 8.0 },
                    text: "Hi".into(),
                    size: 28.0,
                },
                color: Color::rgb(0, 0, 0),
                stroke: 1.0,
            }],
        );
        assert!(differs(&base, &painted));
    }

    #[test]
    fn ellipse_has_an_empty_center() {
        let base = canvas();
        let painted = render(
            &base,
            &[Annotation {
                shape: Shape::Ellipse {
                    a: Pt { x: 10.0, y: 8.0 },
                    b: Pt { x: 70.0, y: 50.0 },
                },
                color: Color::rgb(0, 0, 0),
                stroke: 2.0,
            }],
        );
        assert!(
            painted.get_pixel(40, 8).0[0] < 160,
            "top of the ellipse should be ink"
        );
        assert!(painted.get_pixel(40, 29).0[0] > 200);
    }
}

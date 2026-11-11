//! Grab every monitor the moment the shortcut fires.
//!
//! The bitmaps are what the overlay paints, so the picture is frozen even if a
//! menu or a video frame disappears underneath the window.

use anyhow::{Result, anyhow};
use image::RgbaImage;
use std::time::Instant;

use crate::desktop::{MonitorShot, initial_point_rect, separate_mixed_scale_origins};
use crate::log::log;

#[derive(Clone, Debug)]
pub struct Desktop {
    pub monitors: Vec<MonitorShot>,
    pub captured_at: Instant,
    pub capture_ms: u128,
}

pub fn capture_desktop() -> Result<Desktop> {
    let started = Instant::now();
    let monitors = xcap::Monitor::all().map_err(|err| anyhow!("monitor list: {err}"))?;
    if monitors.is_empty() {
        return Err(anyhow!("no monitors reported"));
    }
    let mut shots = Vec::with_capacity(monitors.len());
    for monitor in monitors {
        let name = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .unwrap_or_else(|_| "Monitor".into());
        let raw_x = monitor.x().unwrap_or(0);
        let raw_y = monitor.y().unwrap_or(0);
        let raw_w = monitor.width().unwrap_or(0);
        let raw_h = monitor.height().unwrap_or(0);
        let reported_scale = monitor.scale_factor().unwrap_or(1.0);
        let image = monitor
            .capture_image()
            .map_err(|err| anyhow!("capture {name}: {err}"))?;
        if image.width() == 0 || image.height() == 0 {
            return Err(anyhow!("capture {name} returned an empty image"));
        }
        let (origin_x, origin_y, point_w, point_h) = initial_point_rect(
            raw_x,
            raw_y,
            raw_w,
            raw_h,
            reported_scale,
            image.width(),
            image.height(),
        );
        log(format!(
            "monitor {name} raw=({raw_x},{raw_y} {raw_w}x{raw_h}) scale={reported_scale:.2} image={}x{} points=({origin_x:.0},{origin_y:.0} {point_w:.0}x{point_h:.0})",
            image.width(),
            image.height()
        ));
        shots.push(MonitorShot {
            name,
            raw_x,
            raw_y,
            raw_w,
            raw_h,
            reported_scale,
            image,
            origin_x,
            origin_y,
            point_w,
            point_h,
        });
    }
    // Top-to-bottom, then left-to-right, so a later match against window
    // rectangles has a stable order when two displays share a resolution.
    shots.sort_by(|a, b| {
        a.raw_y
            .cmp(&b.raw_y)
            .then(a.raw_x.cmp(&b.raw_x))
            .then(a.name.cmp(&b.name))
    });
    separate_mixed_scale_origins(&mut shots);
    let capture_ms = started.elapsed().as_millis();
    log(format!(
        "froze {} monitor(s) in {capture_ms} ms",
        shots.len()
    ));
    Ok(Desktop {
        monitors: shots,
        captured_at: started,
        capture_ms,
    })
}

pub fn image_is_suspicious_black(image: &RgbaImage) -> bool {
    if image.width() < 8 || image.height() < 8 {
        return false;
    }
    // Sample a grid. A real black wallpaper can be black; this is only a hint
    // when combined with a permission error, not a hard failure.
    let mut dark = 0u32;
    let mut seen = 0u32;
    let step_x = (image.width() / 8).max(1);
    let step_y = (image.height() / 8).max(1);
    let mut y = step_y / 2;
    while y < image.height() {
        let mut x = step_x / 2;
        while x < image.width() {
            let p = image.get_pixel(x, y).0;
            if p[0] < 8 && p[1] < 8 && p[2] < 8 {
                dark += 1;
            }
            seen += 1;
            x += step_x;
        }
        y += step_y;
    }
    seen > 0 && dark * 100 / seen > 97
}

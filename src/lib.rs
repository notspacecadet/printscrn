//! Stillshot library: freeze-frame capture, annotation, and clipboard encoding.
//! The binary in `main.rs` is the tray app; these modules are also what the
//! unit tests exercise without opening a window.

pub mod annotate;
pub mod app;
pub mod capture;
pub mod clipboard;
pub mod config;
pub mod desktop;
pub mod gate;
pub mod geom;
pub mod hotkey;
pub mod icon;
pub mod log;
pub mod overlay;
pub mod paint;
pub mod platform;
pub mod save;

use std::process::ExitCode;
use std::time::Instant;

pub fn run() -> eframe::Result {
    platform::enable_dpi_awareness();
    log::init();
    log::log(format!("version {}", env!("CARGO_PKG_VERSION")));
    install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--self-check") {
        std::process::exit(self_check());
    }
    if let Some(path) = args
        .iter()
        .position(|arg| arg == "--dump-clipboard")
        .and_then(|i| args.get(i + 1))
    {
        match clipboard::read_image() {
            Ok(image) => {
                if let Err(err) = image.save(path) {
                    eprintln!("save failed: {err}");
                    std::process::exit(1);
                }
                println!("wrote {} ({}x{})", path, image.width(), image.height());
            }
            Err(err) => {
                eprintln!("clipboard read failed: {err}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--bench-capture") {
        match capture::capture_desktop() {
            Ok(desktop) => {
                println!("capture_ms={}", desktop.capture_ms);
                for monitor in &desktop.monitors {
                    println!(
                        "monitor {} image={}x{} points=({:.0},{:.0} {:.0}x{:.0})",
                        monitor.name,
                        monitor.image.width(),
                        monitor.image.height(),
                        monitor.origin_x,
                        monitor.origin_y,
                        monitor.point_w,
                        monitor.point_h
                    );
                }
            }
            Err(err) => {
                eprintln!("capture failed: {err}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    if !platform::acquire_single_instance() {
        log::log("another Stillshot is already running; exiting without capturing");
        return Ok(());
    }
    // A leftover file from an older build must not freeze the screen on startup.
    let _ = std::fs::remove_file(config::config_dir().join("capture.request"));

    let background = args.iter().any(|arg| arg == "--background");
    let capture_now = args.iter().any(|arg| arg == "--capture");
    let launch = app::Launch {
        show_settings: args.iter().any(|arg| arg == "--settings") || (!background && !capture_now),
        capture_now,
    };

    let (rgba, width, height) = icon::icon_rgba(64);
    let icon = eframe::egui::IconData {
        rgba,
        width,
        height,
    };
    let mut native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Stillshot")
            .with_inner_size([460.0, 560.0])
            .with_min_inner_size([400.0, 420.0])
            .with_icon(std::sync::Arc::new(icon))
            .with_visible(launch.show_settings),
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    if cfg!(target_os = "windows") {
        // Every overlay window is presented from inside the settings window's
        // frame. With vsync each of those presents waits for its own vertical
        // blank, so two monitors would halve the frame rate. The root frame
        // is capped at about 60 Hz while a freeze is open.
        native.glow_options.vsync = false;
    }
    eframe::run_native(
        "Stillshot",
        native,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, launch)))),
    )
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::log(format!("panic: {info}"));
        platform::emergency_close_windows();
        previous(info);
    }));
}

fn self_check() -> i32 {
    println!("formats: {}", clipboard::format_names().join(", "));
    let started = Instant::now();
    let desktop = match capture::capture_desktop() {
        Ok(desktop) => desktop,
        Err(err) => {
            eprintln!("FAIL capture: {err}");
            return 1;
        }
    };
    println!("capture_ms={}", started.elapsed().as_millis());
    if desktop.monitors.is_empty() {
        eprintln!("FAIL no monitors");
        return 1;
    }
    let image = &desktop.monitors[0].image;
    println!("primary={}x{}", image.width(), image.height());
    let copy_started = Instant::now();
    if let Err(err) = clipboard::copy_image(image) {
        eprintln!("FAIL clipboard write: {err}");
        return 1;
    }
    println!("clipboard_write_ms={}", copy_started.elapsed().as_millis());
    match clipboard::read_image() {
        Ok(back) => {
            if back.width() != image.width() || back.height() != image.height() {
                eprintln!(
                    "FAIL clipboard size {}x{} != {}x{}",
                    back.width(),
                    back.height(),
                    image.width(),
                    image.height()
                );
                return 1;
            }
            let sample = image.get_pixel(image.width() / 2, image.height() / 2).0;
            let got = back.get_pixel(back.width() / 2, back.height() / 2).0;
            println!("center_pixel src={sample:?} clipboard={got:?}");
            if sample != got {
                eprintln!("FAIL center pixel mismatch");
                return 1;
            }
        }
        Err(err) => {
            eprintln!("FAIL clipboard read: {err}");
            return 1;
        }
    }
    println!("self-check ok");
    let _ = ExitCode::SUCCESS;
    0
}

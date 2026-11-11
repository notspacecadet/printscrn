//! Login item, single-instance lock, keyboard hook, and platform permission helpers.

use anyhow::{Result, anyhow};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static OVERLAY_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Start a 60s watchdog before grabbing the screen. If the UI thread is stuck
/// inside capture, this still exits the process and drops the overlay.
pub fn begin_overlay_watch() {
    let generation = OVERLAY_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        if OVERLAY_GENERATION.load(Ordering::SeqCst) == generation {
            crate::log::log("watchdog: overlay still open after 60s; exiting");
            emergency_close_windows();
            std::process::exit(2);
        }
    });
}

pub fn end_overlay_watch() {
    OVERLAY_GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub fn set_launch_at_login(enabled: bool) -> Result<()> {
    let launch = auto_launch()?;
    if enabled {
        launch.enable().map_err(|err| anyhow!("{err}"))?;
    } else if launch.is_enabled().unwrap_or(false) {
        launch.disable().map_err(|err| anyhow!("{err}"))?;
    }
    Ok(())
}

pub fn launch_at_login_enabled() -> bool {
    auto_launch()
        .and_then(|launch| launch.is_enabled().map_err(|err| anyhow!("{err}")))
        .unwrap_or(false)
}

fn auto_launch() -> Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| anyhow!("executable path is not utf-8"))?;
    #[allow(unused_mut)]
    let mut builder = auto_launch::AutoLaunchBuilder::new();
    builder.set_app_name("Stillshot");
    builder.set_app_path(exe);
    builder.set_args(&["--background"]);
    #[cfg(target_os = "macos")]
    builder.set_use_launch_agent(true);
    builder.build().map_err(|err| anyhow!("{err}"))
}

pub fn open_path(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub fn open_screen_recording_settings() {
    #[cfg(target_os = "macos")]
    {
        macos::open_screen_recording_settings();
    }
}

pub fn screen_recording_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::screen_recording_granted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn request_screen_recording() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::request_screen_recording()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Hold a process-wide lock. A second launch writes a capture request and
/// returns false so the caller can exit.
pub fn acquire_single_instance() -> bool {
    #[cfg(target_os = "windows")]
    {
        return windows::acquire_single_instance();
    }
    #[cfg(unix)]
    {
        unix_lock()
    }
}

pub fn signal_capture_request() {
    let path = crate::config::config_dir().join("capture.request");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, b"1");
}

pub fn take_capture_request() -> bool {
    let path = crate::config::config_dir().join("capture.request");
    if path.exists() {
        let _ = std::fs::remove_file(path);
        true
    } else {
        false
    }
}

#[cfg(unix)]
fn unix_lock() -> bool {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;
    let path = crate::config::config_dir().join("instance.lock");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
    else {
        return true;
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return false;
    }
    // The lock lives as long as the fd does.
    std::mem::forget(file);
    true
}

/// Start the Windows keyboard hook thread. It carries Print Screen and the
/// keys that close the overlay. Does nothing on other platforms.
pub fn start_keyboard_hook() {
    #[cfg(target_os = "windows")]
    windows::start_keyboard_hook();
}

/// Give the platform layer a way to wake the UI thread from another thread.
pub fn set_waker(waker: Box<dyn Fn() + Send + Sync>) {
    #[cfg(target_os = "windows")]
    windows::set_waker(waker);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = waker;
    }
}

pub fn install_print_screen_hook(ctrl: bool, alt: bool, shift: bool, meta: bool) {
    #[cfg(target_os = "windows")]
    windows::install_print_screen_hook(ctrl, alt, shift, meta);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (ctrl, alt, shift, meta);
    }
}

pub fn remove_print_screen_hook() {
    #[cfg(target_os = "windows")]
    windows::remove_print_screen_hook();
}

pub fn print_screen_pressed() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::take_print_screen()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

pub fn set_overlay_active(active: bool) {
    #[cfg(target_os = "windows")]
    windows::set_overlay_active(active);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = active;
    }
}

/// A request to close the overlay that did not come through the overlay
/// window: the keyboard hook, or a key-state poll. False on platforms
/// without either.
pub fn dismiss_pressed() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::dismiss_pressed()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Ctrl+Alt+Q, edge-triggered.
pub fn panic_quit_pressed() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::panic_quit_pressed()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Monitor rectangles `(x, y, width, height)` in physical pixels, in the order
/// the windowing layer numbers monitors. Empty when the platform cannot say.
pub fn monitor_rects() -> Vec<(i32, i32, i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        windows::monitor_rects()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}

/// Cursor position in the same coordinates as [`monitor_rects`].
pub fn cursor_position() -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        windows::cursor_position()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

pub fn emergency_close_windows() {
    #[cfg(target_os = "windows")]
    windows::emergency_close_windows();
}

pub fn enable_dpi_awareness() {
    #[cfg(target_os = "windows")]
    windows::enable_dpi_awareness();
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

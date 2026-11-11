//! Screen Recording permission. The system prompt is only useful after the
//! user has been told why we need it, so the settings window calls
//! [`request_screen_recording`] from a button instead of blocking startup.

use std::process::Command;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

pub fn screen_recording_granted() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

pub fn request_screen_recording() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

pub fn open_screen_recording_settings() {
    // Ventura and later understand the second URL. Monterey still handles the first.
    let urls = [
        "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_ScreenCapture",
        "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
    ];
    for url in urls {
        if Command::new("open")
            .arg(url)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return;
        }
    }
}

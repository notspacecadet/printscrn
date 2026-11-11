//! Put an RGBA image on the OS clipboard in the formats each platform pastes.
//!
//! * Windows: PNG (format name `"PNG"`), classic `CF_DIB`, and `CF_DIBV5`.
//! * macOS: `NSPasteboard` types `public.png` and `public.tiff`.
//! * Linux: `image/png` via the X11 clipboard, so the same code path can be
//!   exercised in this environment. The Linux process has to stay alive; the
//!   clipboard owner is this app, which is the tray-resident process.

use anyhow::Result;
use image::RgbaImage;

mod encode;

#[cfg(all(unix, not(target_os = "macos")))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub use encode::{
    decode_dib, decode_png, decode_tiff, encode_dib, encode_dib_v5, encode_png, encode_tiff,
};

/// Names of the clipboard representations [`copy_image`] writes.
pub fn format_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["PNG", "CF_DIB", "CF_DIBV5"]
    }
    #[cfg(target_os = "macos")]
    {
        &["public.png", "public.tiff"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &["image/png"]
    }
}

/// Copy `image` to the clipboard. Call this on the UI thread: AppKit's
/// pasteboard is main-thread-bound, and Windows clipboard calls are happy there too.
pub fn copy_image(image: &RgbaImage) -> Result<()> {
    if image.width() == 0 || image.height() == 0 {
        anyhow::bail!("refusing to copy an empty image");
    }
    let png = encode::encode_png(image)?;
    #[cfg(target_os = "windows")]
    {
        let dib = encode_dib(image);
        let dib_v5 = encode_dib_v5(image);
        windows::set_clipboard_image(&png, &dib, &dib_v5)?;
    }
    #[cfg(target_os = "macos")]
    {
        let tiff = encode::encode_tiff(image)?;
        macos::set_clipboard_image(&png, &tiff)?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        linux::set_clipboard_image(image)?;
    }
    let _ = png;
    Ok(())
}

/// Read the clipboard image back, when the platform lets us.
/// Linux and Windows go through `arboard`. macOS reads `public.png`.
pub fn read_image() -> Result<RgbaImage> {
    #[cfg(target_os = "macos")]
    {
        return macos::read_png_image();
    }
    #[cfg(not(target_os = "macos"))]
    {
        use arboard::Clipboard;
        let mut clipboard = Clipboard::new()?;
        let image = clipboard.get_image()?;
        let width = image.width as u32;
        let height = image.height as u32;
        RgbaImage::from_raw(width, height, image.bytes.into_owned())
            .ok_or_else(|| anyhow::anyhow!("clipboard image buffer has the wrong size"))
    }
}

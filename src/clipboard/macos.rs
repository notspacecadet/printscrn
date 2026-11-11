//! macOS pasteboard: explicit PNG and TIFF representations.
//!
//! `NSImage` alone often advertises only TIFF until someone asks. Writing
//! `public.png` and `public.tiff` ourselves is what Preview, Chrome, Slack,
//! and Messages actually request.

use anyhow::{Result, anyhow};
use image::RgbaImage;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeTIFF};
use objc2_foundation::{NSArray, NSData};

pub fn set_clipboard_image(png: &[u8], tiff: &[u8]) -> Result<()> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let png_type = unsafe { NSPasteboardTypePNG };
    let tiff_type = unsafe { NSPasteboardTypeTIFF };
    let types = NSArray::from_slice(&[png_type, tiff_type]);
    let declared = unsafe { pasteboard.declareTypes_owner(&types, None) };
    if declared == 0 {
        return Err(anyhow!("NSPasteboard declareTypes returned 0"));
    }
    let png_data = NSData::with_bytes(png);
    let tiff_data = NSData::with_bytes(tiff);
    let png_ok = pasteboard.setData_forType(Some(&png_data), png_type);
    let tiff_ok = pasteboard.setData_forType(Some(&tiff_data), tiff_type);
    if png_ok && tiff_ok {
        Ok(())
    } else {
        Err(anyhow!(
            "NSPasteboard setData failed (png={png_ok}, tiff={tiff_ok})"
        ))
    }
}

pub fn read_png_image() -> Result<RgbaImage> {
    let pasteboard = NSPasteboard::generalPasteboard();
    let data = pasteboard
        .dataForType(unsafe { NSPasteboardTypePNG })
        .ok_or_else(|| anyhow!("pasteboard has no public.png"))?;
    // objc2-foundation keeps `bytes` crate-private; `to_vec` is the public copy.
    let bytes = data.to_vec();
    crate::clipboard::decode_png(&bytes)
}

//! Windows clipboard: PNG + CF_DIB + CF_DIBV5 in one `OpenClipboard` session.
//!
//! PNG is placed first. Electron and Chromium look the PNG format up by name
//! and prefer it (Slack, Discord, WhatsApp Web, Gmail, Claude). Classic
//! `CF_DIB` is what Paint and a lot of Win32 apps ask for. `CF_DIBV5` carries
//! an alpha mask for apps that understand it.

use anyhow::{Result, anyhow};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock};

const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;

pub fn set_clipboard_image(png: &[u8], dib: &[u8], dib_v5: &[u8]) -> Result<()> {
    unsafe {
        if !open_clipboard() {
            return Err(anyhow!(
                "OpenClipboard failed; another app is holding the clipboard"
            ));
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err(anyhow!("EmptyClipboard failed"));
        }
        let png_format = register_png_format()?;
        // Order matters for the few apps that take whichever format they see first.
        // PNG covers browsers and chat apps; DIB covers Paint and Office.
        let png_ok = put_format(png_format, png);
        let dib_ok = put_format(CF_DIB, dib);
        let v5_ok = put_format(CF_DIBV5, dib_v5);
        CloseClipboard();
        if !png_ok || !dib_ok || !v5_ok {
            return Err(anyhow!(
                "SetClipboardData failed (png={png_ok}, dib={dib_ok}, dibv5={v5_ok})"
            ));
        }
        Ok(())
    }
}

unsafe fn open_clipboard() -> bool {
    for _ in 0..15 {
        if OpenClipboard(std::ptr::null_mut()) != 0 {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}

unsafe fn register_png_format() -> Result<u32> {
    let name: Vec<u16> = "PNG".encode_utf16().chain(std::iter::once(0)).collect();
    let format = RegisterClipboardFormatW(name.as_ptr());
    if format == 0 {
        Err(anyhow!("RegisterClipboardFormat(\"PNG\") failed"))
    } else {
        Ok(format)
    }
}

unsafe fn put_format(format: u32, bytes: &[u8]) -> bool {
    let handle = GlobalAlloc(GHND, bytes.len());
    if handle.is_null() {
        return false;
    }
    let locked = GlobalLock(handle);
    if locked.is_null() {
        GlobalFree(handle);
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), locked as *mut u8, bytes.len());
    GlobalUnlock(handle);
    let set = SetClipboardData(format, handle as HANDLE);
    if set.is_null() {
        // SetClipboardData takes ownership only on success.
        GlobalFree(handle);
        false
    } else {
        true
    }
}

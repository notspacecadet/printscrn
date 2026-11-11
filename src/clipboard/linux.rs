//! X11 clipboard via arboard (`image/png`). The `Clipboard` owner is kept
//! alive for the life of the process so the selection survives after we return.

use anyhow::Result;
use arboard::{Clipboard, ImageData};
use image::RgbaImage;
use std::borrow::Cow;
use std::sync::Mutex;

static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

pub fn set_clipboard_image(image: &RgbaImage) -> Result<()> {
    let mut slot = CLIPBOARD.lock().unwrap_or_else(|err| err.into_inner());
    if slot.is_none() {
        *slot = Some(Clipboard::new()?);
    }
    let clipboard = slot.as_mut().unwrap();
    clipboard.set_image(ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Borrowed(image.as_raw()),
    })?;
    Ok(())
}

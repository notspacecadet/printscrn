//! PNG, classic CF_DIB, CF_DIBV5, and TIFF bytes.
//!
//! The Windows clipboard wants both a PNG (`RegisterClipboardFormat("PNG")`,
//! which Chrome, Slack, WhatsApp Web, and Gmail paste) and a DIB (Paint and
//! older native apps). macOS wants `public.png` and `public.tiff` on
//! `NSPasteboard`. These encoders are pure so both layouts can be tested
//! without a window server.

use anyhow::{Result, anyhow};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::tiff::TiffEncoder;
use image::{ExtendedColorType, ImageEncoder, RgbaImage};
use std::io::Cursor;

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity((image.width() * image.height()) as usize);
    let encoder = PngEncoder::new_with_quality(&mut buf, CompressionType::Fast, FilterType::Sub);
    encoder
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|err| anyhow!("png encode: {err}"))?;
    Ok(buf)
}

/// Classic `CF_DIB`: a 40-byte `BITMAPINFOHEADER` plus bottom-up 32-bit BGRA.
/// `biHeight` is positive because Word and Paint reject a top-down DIB.
pub fn encode_dib(image: &RgbaImage) -> Vec<u8> {
    let w = image.width();
    let h = image.height();
    let pixels = (w as usize) * (h as usize) * 4;
    let mut out = vec![0u8; 40 + pixels];
    write_u32(&mut out, 0, 40);
    write_i32(&mut out, 4, w as i32);
    write_i32(&mut out, 8, h as i32);
    write_u16(&mut out, 12, 1);
    write_u16(&mut out, 14, 32);
    write_u32(&mut out, 16, 0); // BI_RGB
    write_u32(&mut out, 20, pixels as u32);
    write_bgra_bottom_up(&mut out[40..], image);
    out
}

/// `CF_DIBV5` with explicit channel masks so alpha-aware apps can read it.
pub fn encode_dib_v5(image: &RgbaImage) -> Vec<u8> {
    let w = image.width();
    let h = image.height();
    let pixels = (w as usize) * (h as usize) * 4;
    const HEADER: usize = 124;
    let mut out = vec![0u8; HEADER + pixels];
    write_u32(&mut out, 0, HEADER as u32);
    write_i32(&mut out, 4, w as i32);
    write_i32(&mut out, 8, h as i32);
    write_u16(&mut out, 12, 1);
    write_u16(&mut out, 14, 32);
    write_u32(&mut out, 16, 3); // BI_BITFIELDS
    write_u32(&mut out, 20, pixels as u32);
    // masks at offset 40: R, G, B, A as they appear in a little-endian DWORD of BGRA memory
    write_u32(&mut out, 40, 0x00ff_0000);
    write_u32(&mut out, 44, 0x0000_ff00);
    write_u32(&mut out, 48, 0x0000_00ff);
    write_u32(&mut out, 52, 0xff00_0000);
    write_u32(&mut out, 56, 0x7352_4742); // LCS_sRGB
    // CIEXYZTRIPLE at 60 is 36 bytes of zeros, which is correct for LCS_sRGB.
    write_u32(&mut out, 108, 4); // LCS_GM_IMAGES
    write_bgra_bottom_up(&mut out[HEADER..], image);
    out
}

pub fn encode_tiff(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    let encoder = TiffEncoder::new(&mut cursor);
    encoder
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|err| anyhow!("tiff encode: {err}"))?;
    Ok(cursor.into_inner())
}

pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|err| anyhow!("png decode: {err}"))?;
    Ok(image.to_rgba8())
}

pub fn decode_tiff(bytes: &[u8]) -> Result<RgbaImage> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Tiff)
        .map_err(|err| anyhow!("tiff decode: {err}"))?;
    Ok(image.to_rgba8())
}

/// Decode a classic CF_DIB blob back to top-down RGBA. Used by tests and as a
/// sanity check of the header we hand to Windows.
pub fn decode_dib(bytes: &[u8]) -> Result<RgbaImage> {
    if bytes.len() < 40 {
        return Err(anyhow!("DIB shorter than BITMAPINFOHEADER"));
    }
    let header = read_u32(bytes, 0);
    if header < 40 {
        return Err(anyhow!("unexpected DIB header size {header}"));
    }
    let width = read_i32(bytes, 4);
    let height = read_i32(bytes, 8);
    let planes = read_u16(bytes, 12);
    let bit_count = read_u16(bytes, 14);
    if planes != 1 || bit_count != 32 || width <= 0 || height == 0 {
        return Err(anyhow!("unsupported DIB {width}x{height} {bit_count}bpp"));
    }
    let top_down = height < 0;
    let height = height.unsigned_abs();
    let width = width as u32;
    let offset = header as usize;
    let need = offset + (width as usize) * (height as usize) * 4;
    if bytes.len() < need {
        return Err(anyhow!("DIB pixel buffer truncated"));
    }
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        let src_y = if top_down { y } else { height - 1 - y };
        for x in 0..width {
            let i = offset + ((src_y as usize) * (width as usize) + x as usize) * 4;
            image.put_pixel(
                x,
                y,
                image::Rgba([bytes[i + 2], bytes[i + 1], bytes[i], bytes[i + 3]]),
            );
        }
    }
    Ok(image)
}

fn write_bgra_bottom_up(dst: &mut [u8], image: &RgbaImage) {
    let w = image.width() as usize;
    let h = image.height() as usize;
    for y in 0..h {
        let src_y = h - 1 - y;
        for x in 0..w {
            let pixel = image.get_pixel(x as u32, src_y as u32).0;
            let i = (y * w + x) * 4;
            dst[i] = pixel[2];
            dst[i + 1] = pixel[1];
            dst[i + 2] = pixel[0];
            dst[i + 3] = pixel[3];
        }
    }
}

fn write_u16(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_i32(buf: &mut [u8], offset: usize, value: i32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn read_u16(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

fn read_i32(buf: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn sample() -> RgbaImage {
        let mut image = RgbaImage::new(2, 2);
        image.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        image.put_pixel(1, 0, Rgba([40, 50, 60, 128]));
        image.put_pixel(0, 1, Rgba([70, 80, 90, 255]));
        image.put_pixel(1, 1, Rgba([1, 2, 3, 0]));
        image
    }

    #[test]
    fn png_roundtrip_keeps_alpha() {
        let image = sample();
        let decoded = decode_png(&encode_png(&image).unwrap()).unwrap();
        assert_eq!(decoded.as_raw(), image.as_raw());
    }

    #[test]
    fn tiff_roundtrip_keeps_pixels() {
        let image = sample();
        let decoded = decode_tiff(&encode_tiff(&image).unwrap()).unwrap();
        assert_eq!(decoded.as_raw(), image.as_raw());
    }

    #[test]
    fn dib_is_bottom_up_bgra_with_a_40_byte_header() {
        let image = sample();
        let dib = encode_dib(&image);
        assert_eq!(&dib[0..4], &40u32.to_le_bytes());
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        // First stored row is the bottom source row: pixel (0,1) = RGBA 70,80,90 -> BGRA 90,80,70
        assert_eq!(&dib[40..44], &[90, 80, 70, 255]);
        let decoded = decode_dib(&dib).unwrap();
        assert_eq!(decoded.as_raw(), image.as_raw());
    }

    #[test]
    fn dib_v5_advertises_alpha_masks() {
        let image = sample();
        let dib = encode_dib_v5(&image);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 124);
        assert_eq!(u32::from_le_bytes(dib[16..20].try_into().unwrap()), 3);
        assert_eq!(
            u32::from_le_bytes(dib[40..44].try_into().unwrap()),
            0x00ff_0000
        );
        assert_eq!(
            u32::from_le_bytes(dib[52..56].try_into().unwrap()),
            0xff00_0000
        );
        assert_eq!(
            u32::from_le_bytes(dib[56..60].try_into().unwrap()),
            0x7352_4742
        );
        // Pixel bytes still decode with the classic reader (header size 124).
        let decoded = decode_dib(&dib).unwrap();
        assert_eq!(decoded.as_raw(), image.as_raw());
    }
}

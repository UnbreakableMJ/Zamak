// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Boot-menu wallpaper rendering (FR-UI-001).
//!
//! Parses 24/32-bit uncompressed BMP images (the format every image editor
//! emits by default) and provides helpers to blit them onto a framebuffer
//! with `tiled`, `centered`, or `stretched` placement styles (matching
//! Limine's `wallpaper_style` option).

// Rust guideline compliant 2026-03-30

use crate::gfx::{Canvas, Color};
use alloc::vec::Vec;

/// BMP file header signature ("BM" little-endian).
pub const BMP_SIGNATURE: [u8; 2] = [b'B', b'M'];

/// Errors produced while parsing BMP images.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BmpError {
    /// File is shorter than the fixed headers.
    Truncated,
    /// File does not start with `BM`.
    InvalidSignature,
    /// DIB header variant is not supported (we handle BITMAPINFOHEADER family).
    UnsupportedDib,
    /// Pixel format not supported (we handle 24 bpp and 32 bpp uncompressed).
    UnsupportedBpp,
    /// Compression scheme not supported.
    UnsupportedCompression,
    /// Declared pixel data lies outside the file.
    PixelDataOutOfBounds,
}

/// Wallpaper placement style (matches Limine `wallpaper_style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Tiled,
    Centered,
    Stretched,
}

impl Style {
    /// Parses a Limine `wallpaper_style` value. Defaults to `Stretched`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "tiled" | "TILED" => Self::Tiled,
            "centered" | "CENTERED" => Self::Centered,
            _ => Self::Stretched,
        }
    }
}

/// A decoded BMP image stored as a top-down RGB buffer.
#[derive(Debug)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Color>,
}

impl Bitmap {
    /// Returns the pixel at `(x, y)` or `Color { r: 0, g: 0, b: 0 }` if out of bounds.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color { r: 0, g: 0, b: 0 };
        }
        self.pixels[(y * self.width + x) as usize]
    }
}

/// Parses a BMP byte buffer into a [`Bitmap`].
pub fn parse(bytes: &[u8]) -> Result<Bitmap, BmpError> {
    if bytes.len() < 54 {
        return Err(BmpError::Truncated);
    }

    if bytes[0..2] != BMP_SIGNATURE {
        return Err(BmpError::InvalidSignature);
    }

    let pixel_offset = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
    let dib_size = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
    if dib_size < 40 {
        return Err(BmpError::UnsupportedDib);
    }

    let width = i32::from_le_bytes(bytes[18..22].try_into().unwrap());
    let height_signed = i32::from_le_bytes(bytes[22..26].try_into().unwrap());
    let bpp = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(bytes[30..34].try_into().unwrap());

    // Only BI_RGB (0) and BI_BITFIELDS (3, 32-bit only) are accepted.
    if compression != 0 && !(compression == 3 && bpp == 32) {
        return Err(BmpError::UnsupportedCompression);
    }

    if bpp != 24 && bpp != 32 {
        return Err(BmpError::UnsupportedBpp);
    }

    let width = width.max(0) as u32;
    let height = height_signed.unsigned_abs();
    let top_down = height_signed < 0;

    let bytes_per_pixel = (bpp / 8) as usize;
    let row_bytes_unpadded = width as usize * bytes_per_pixel;
    // BMP rows are padded to a 4-byte boundary.
    let row_stride = (row_bytes_unpadded + 3) & !3;
    let expected = pixel_offset + row_stride * height as usize;
    if bytes.len() < expected {
        return Err(BmpError::PixelDataOutOfBounds);
    }

    let mut pixels = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        // BMP is bottom-up unless height is negative.
        let src_row = if top_down { y } else { height - 1 - y };
        let row_start = pixel_offset + src_row as usize * row_stride;
        for x in 0..width {
            let px = row_start + x as usize * bytes_per_pixel;
            // BMP stores B, G, R in little-endian order.
            let b = bytes[px];
            let g = bytes[px + 1];
            let r = bytes[px + 2];
            pixels.push(Color { r, g, b });
        }
    }

    Ok(Bitmap { width, height, pixels })
}

/// Draws a wallpaper on the canvas using the given placement style.
pub fn draw(canvas: &mut Canvas, wallpaper: &Bitmap, style: Style) {
    let cw = canvas.width() as u32;
    let ch = canvas.height() as u32;

    match style {
        Style::Tiled => draw_tiled(canvas, wallpaper, cw, ch),
        Style::Centered => draw_centered(canvas, wallpaper, cw, ch),
        Style::Stretched => draw_stretched(canvas, wallpaper, cw, ch),
    }
}

fn draw_tiled(canvas: &mut Canvas, wp: &Bitmap, cw: u32, ch: u32) {
    for y in 0..ch {
        for x in 0..cw {
            let color = wp.pixel(x % wp.width, y % wp.height);
            canvas.put_pixel(x as u64, y as u64, color);
        }
    }
}

fn draw_centered(canvas: &mut Canvas, wp: &Bitmap, cw: u32, ch: u32) {
    let offset_x = cw.saturating_sub(wp.width) / 2;
    let offset_y = ch.saturating_sub(wp.height) / 2;
    for y in 0..wp.height.min(ch) {
        for x in 0..wp.width.min(cw) {
            let color = wp.pixel(x, y);
            canvas.put_pixel((offset_x + x) as u64, (offset_y + y) as u64, color);
        }
    }
}

fn draw_stretched(canvas: &mut Canvas, wp: &Bitmap, cw: u32, ch: u32) {
    if wp.width == 0 || wp.height == 0 {
        return;
    }
    // Nearest-neighbour resampling.
    for y in 0..ch {
        let src_y = (y as u64 * wp.height as u64 / ch as u64) as u32;
        for x in 0..cw {
            let src_x = (x as u64 * wp.width as u64 / cw as u64) as u32;
            let color = wp.pixel(src_x, src_y);
            canvas.put_pixel(x as u64, y as u64, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal 2x2 BMP (24 bpp, bottom-up) for tests.
    fn build_2x2_bmp() -> Vec<u8> {
        // Pixel data: row-0 (bottom): red, green; row-1 (top): blue, white.
        // BGR order, padded to 4 bytes per row.
        let mut bmp = Vec::new();

        // File header (14 bytes).
        bmp.extend_from_slice(&BMP_SIGNATURE);
        bmp.extend_from_slice(&70u32.to_le_bytes()); // file size
        bmp.extend_from_slice(&0u32.to_le_bytes()); // reserved
        bmp.extend_from_slice(&54u32.to_le_bytes()); // pixel offset

        // DIB header (40 bytes, BITMAPINFOHEADER).
        bmp.extend_from_slice(&40u32.to_le_bytes()); // dib size
        bmp.extend_from_slice(&2i32.to_le_bytes()); // width
        bmp.extend_from_slice(&2i32.to_le_bytes()); // height (positive = bottom-up)
        bmp.extend_from_slice(&1u16.to_le_bytes()); // planes
        bmp.extend_from_slice(&24u16.to_le_bytes()); // bpp
        bmp.extend_from_slice(&0u32.to_le_bytes()); // compression = BI_RGB
        bmp.extend_from_slice(&16u32.to_le_bytes()); // image size
        bmp.extend_from_slice(&0u32.to_le_bytes()); // x res
        bmp.extend_from_slice(&0u32.to_le_bytes()); // y res
        bmp.extend_from_slice(&0u32.to_le_bytes()); // colors used
        bmp.extend_from_slice(&0u32.to_le_bytes()); // important colors

        // Pixel data — bottom row first (BMP bottom-up).
        // Row 0 (bottom): red (0,0,255 BGR), green (0,255,0 BGR)
        bmp.extend_from_slice(&[0, 0, 255, 0, 255, 0]);
        bmp.extend_from_slice(&[0, 0]); // row padding to 8 bytes
        // Row 1 (top): blue (255,0,0 BGR), white (255,255,255 BGR)
        bmp.extend_from_slice(&[255, 0, 0, 255, 255, 255]);
        bmp.extend_from_slice(&[0, 0]);

        bmp
    }

    #[test]
    fn parse_2x2_bmp() {
        let bmp = build_2x2_bmp();
        let bitmap = parse(&bmp).unwrap();
        assert_eq!(bitmap.width, 2);
        assert_eq!(bitmap.height, 2);
        // Top-left (0,0) after parse should be the logically-top-left pixel,
        // which for a bottom-up BMP is the last source row, first pixel (blue).
        let top_left = bitmap.pixel(0, 0);
        assert_eq!(top_left, Color { r: 0, g: 0, b: 255 });
        // Bottom-right (1,1) should be green.
        let bottom_right = bitmap.pixel(1, 1);
        assert_eq!(bottom_right, Color { r: 0, g: 255, b: 0 });
    }

    #[test]
    fn parse_rejects_bad_signature() {
        let mut bmp = build_2x2_bmp();
        bmp[0] = b'X';
        assert!(matches!(parse(&bmp), Err(BmpError::InvalidSignature)));
    }

    #[test]
    fn parse_rejects_truncated() {
        let bmp = alloc::vec![0u8; 10];
        assert!(matches!(parse(&bmp), Err(BmpError::Truncated)));
    }

    #[test]
    fn parse_rejects_unsupported_bpp() {
        let mut bmp = build_2x2_bmp();
        // Change bpp field (offset 28).
        bmp[28] = 16;
        bmp[29] = 0;
        assert!(matches!(parse(&bmp), Err(BmpError::UnsupportedBpp)));
    }

    #[test]
    fn style_parse() {
        assert_eq!(Style::parse("tiled"), Style::Tiled);
        assert_eq!(Style::parse("centered"), Style::Centered);
        assert_eq!(Style::parse("stretched"), Style::Stretched);
        assert_eq!(Style::parse("unknown"), Style::Stretched);
    }

    #[test]
    fn pixel_out_of_bounds_returns_black() {
        let bitmap = Bitmap {
            width: 2,
            height: 2,
            pixels: alloc::vec![Color { r: 0xFF, g: 0xFF, b: 0xFF }; 4],
        };
        assert_eq!(bitmap.pixel(99, 99), Color { r: 0, g: 0, b: 0 });
    }
}

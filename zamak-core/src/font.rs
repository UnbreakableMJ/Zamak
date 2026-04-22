// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

use crate::gfx::{Canvas, Color};

#[repr(C, packed)]
pub struct Psf1Header {
    pub magic: [u8; 2], // 0x36, 0x04
    pub mode: u8,
    pub char_size: u8,
}

pub struct PsfFont<'a> {
    pub header: &'a Psf1Header,
    pub glyphs: &'a [u8],
}

pub static DEFAULT_FONT: &[u8] = include_bytes!("assets/font.psf");

impl<'a> PsfFont<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 4 || data[0] != 0x36 || data[1] != 0x04 {
            return None;
        }

        let header = unsafe { &*(data.as_ptr() as *const Psf1Header) };
        let glyphs = &data[4..];

        Some(Self { header, glyphs })
    }

    pub fn draw_char(
        &self,
        canvas: &mut Canvas,
        x: u64,
        y: u64,
        c: char,
        fg: Color,
        bg: Option<Color>,
    ) {
        let char_h = self.header.char_size as u64;
        let char_w = 8; // PSF1 is always 8 pixels wide

        // Basic ASCII mapping
        let char_idx = if c.is_ascii() {
            c as usize
        } else {
            '?' as usize
        };

        let glyph_offset = char_idx * char_h as usize;
        if glyph_offset + char_h as usize > self.glyphs.len() {
            return;
        }

        // Draw background if specified
        if let Some(bg_color) = bg {
            canvas.draw_rect(x, y, char_w, char_h, bg_color);
        }

        for row in 0..char_h {
            let bits = self.glyphs[glyph_offset + row as usize];
            for col in 0..8 {
                if (bits >> (7 - col)) & 1 == 1 {
                    canvas.put_pixel(x + col, y + row, fg);
                }
            }
        }
    }

    pub fn draw_string(
        &self,
        canvas: &mut Canvas,
        x: u64,
        y: u64,
        s: &str,
        fg: Color,
        bg: Option<Color>,
    ) {
        let mut cur_x = x;
        for c in s.chars() {
            self.draw_char(canvas, cur_x, y, c, fg, bg);
            cur_x += 8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Color;
    use crate::protocol::Framebuffer;

    /// Host-safe Framebuffer backed by a `Vec<u8>`. The drawing code
    /// writes raw `u32`s through `fb.address`, so the buffer must stay
    /// pinned for the life of the canvas.
    fn mk_fb(width: u64, height: u64, backing: &mut alloc::vec::Vec<u8>) -> Framebuffer {
        let bpp: u64 = 32;
        let pitch = width * (bpp / 8);
        let size = (pitch * height) as usize;
        *backing = alloc::vec![0u8; size + 4]; // +4 guards overshoot writes
        Framebuffer {
            address: backing.as_mut_ptr() as u64,
            width,
            height,
            pitch,
            bpp: bpp as u16,
            red_mask_size: 8,
            red_mask_shift: 16,
            green_mask_size: 8,
            green_mask_shift: 8,
            blue_mask_size: 8,
            blue_mask_shift: 0,
            ..Default::default()
        }
    }

    /// Build a synthetic PSF1 font with a single lit pixel per glyph
    /// (top-left corner). 16-pixel-tall glyphs, 256 characters → 4 KB.
    fn mk_psf1_font() -> alloc::vec::Vec<u8> {
        let char_h: u8 = 16;
        let mut data = alloc::vec![0u8; 4 + (256 * char_h as usize)];
        data[0] = 0x36;
        data[1] = 0x04;
        data[2] = 0; // mode
        data[3] = char_h;
        // Glyph 'A' (0x41): single pixel at row 0, col 0.
        data[4 + ('A' as usize) * char_h as usize] = 0b1000_0000;
        // Glyph 'O' (0x4F): full row 0.
        data[4 + ('O' as usize) * char_h as usize] = 0xFF;
        // Glyph 'K' (0x4B): full row 0.
        data[4 + ('K' as usize) * char_h as usize] = 0xFF;
        data
    }

    #[test]
    fn synthetic_psf1_parses_cleanly() {
        let font_bytes = mk_psf1_font();
        let font = PsfFont::parse(&font_bytes).expect("synthetic PSF1 must parse");
        assert_eq!(font.header.magic, [0x36, 0x04]);
        assert_eq!(font.header.char_size, 16);
        assert_eq!(font.glyphs.len(), 256 * 16);
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let bad = [0x00u8, 0x00, 0x00, 0x00];
        assert!(PsfFont::parse(&bad).is_none());
    }

    #[test]
    fn parse_rejects_too_short() {
        assert!(PsfFont::parse(&[]).is_none());
        assert!(PsfFont::parse(&[0x36, 0x04]).is_none());
    }

    #[test]
    fn draw_string_writes_some_foreground_pixels() {
        let font_bytes = mk_psf1_font();
        let font = PsfFont::parse(&font_bytes).unwrap();
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(64, 16, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        let white = Color {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        };
        font.draw_string(&mut canvas, 0, 0, "OK", white, None);
        // At least some pixel should be non-zero — the string drew.
        let hit = backing.iter().take(64 * 16 * 4).any(|&b| b != 0);
        assert!(hit, "draw_string produced no visible pixels");
    }

    #[test]
    fn draw_char_out_of_bounds_does_not_panic() {
        let font_bytes = mk_psf1_font();
        let font = PsfFont::parse(&font_bytes).unwrap();
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(32, 16, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        let white = Color {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        };
        // x+8 > width, y+h > height — must clip via put_pixel bounds.
        font.draw_char(&mut canvas, 30, 14, 'A', white, None);
    }

    #[test]
    fn draw_char_with_background_fills_bg_rect() {
        let font_bytes = mk_psf1_font();
        let font = PsfFont::parse(&font_bytes).unwrap();
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(16, 16, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        let fg = Color {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        };
        let bg = Color {
            r: 0x10,
            g: 0x20,
            b: 0x30,
        };
        font.draw_char(&mut canvas, 0, 0, 'A', fg, Some(bg));
        // First pixel (0, 0) is a lit A-pixel in our synthetic font → foreground.
        // Pixel (7, 0) is still in the 8-wide bg rect and not lit → should be bg.
        // Offset for (7, 0) = 7 * 4 = 28. Blue byte is at offset 28 + 0 = 28.
        assert_eq!(backing[28], 0x30, "blue of bg pixel");
        assert_eq!(backing[28 + 1], 0x20, "green of bg pixel");
    }

    #[test]
    fn draw_char_non_ascii_falls_back_to_question_mark() {
        let font_bytes = mk_psf1_font();
        let font = PsfFont::parse(&font_bytes).unwrap();
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(16, 16, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        let fg = Color {
            r: 0xFF,
            g: 0xFF,
            b: 0xFF,
        };
        // Non-ASCII char — must not panic even though our synthetic font
        // has a '?' glyph of all-zeros.
        font.draw_char(&mut canvas, 0, 0, 'é', fg, None);
    }
}

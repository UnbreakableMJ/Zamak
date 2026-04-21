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

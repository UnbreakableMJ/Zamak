// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

use crate::protocol::Framebuffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

// Steelbore Palette (used only as fallback — prefer Theme tokens).
pub const VOID_NAVY: Color = Color {
    r: 0x00,
    g: 0x00,
    b: 0x27,
};
pub const MOLTEN_AMBER: Color = Color {
    r: 0xD9,
    g: 0x8E,
    b: 0x32,
};
pub const STEEL_BLUE: Color = Color {
    r: 0x4B,
    g: 0x7E,
    b: 0xB0,
};
pub const RADIUM_GREEN: Color = Color {
    r: 0x50,
    g: 0xFA,
    b: 0x7B,
};
pub const RED_OXIDE: Color = Color {
    r: 0xFF,
    g: 0x5C,
    b: 0x5C,
};
pub const LIQUID_COOLANT: Color = Color {
    r: 0x8B,
    g: 0xE9,
    b: 0xFD,
};

impl From<zamak_theme::Rgb> for Color {
    fn from(rgb: zamak_theme::Rgb) -> Self {
        Self {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        }
    }
}

pub struct Canvas<'a> {
    fb: &'a mut Framebuffer,
}

impl<'a> Canvas<'a> {
    pub fn new(fb: &'a mut Framebuffer) -> Self {
        Self { fb }
    }

    #[inline]
    pub fn width(&self) -> u64 {
        self.fb.width
    }

    #[inline]
    pub fn height(&self) -> u64 {
        self.fb.height
    }

    pub fn clear(&mut self, color: Color) {
        // Optimization: Fill line by line
        for y in 0..self.fb.height {
            for x in 0..self.fb.width {
                self.put_pixel(x, y, color);
            }
        }
    }

    pub fn draw_rect(&mut self, x: u64, y: u64, w: u64, h: u64, color: Color) {
        for dy in 0..h {
            for dx in 0..w {
                self.put_pixel(x + dx, y + dy, color);
            }
        }
    }

    #[inline]
    pub fn put_pixel(&mut self, x: u64, y: u64, color: Color) {
        if x >= self.fb.width || y >= self.fb.height {
            return;
        }

        let pixel_offset = y * self.fb.pitch + x * (self.fb.bpp as u64 / 8);
        let ptr = (self.fb.address + pixel_offset) as *mut u8;

        unsafe {
            // Assume 32bpp RGB for simplicity for now, handled more robustly in v2
            // VBE/GOP are usually BGRA or RGBA.
            // We use the mask shifts from Framebuffer struct

            let mut val: u32 = 0;
            val |= (color.r as u32) << self.fb.red_mask_shift;
            val |= (color.g as u32) << self.fb.green_mask_shift;
            val |= (color.b as u32) << self.fb.blue_mask_shift;

            // write_unaligned: in real hardware the framebuffer base is
            // page-aligned and pitch typically width*4, so writes are
            // 4-byte aligned in practice. Under the `Vec<u8>`-backed
            // test harness, alignment isn't guaranteed — Miri's
            // symbolic-alignment check correctly flags a plain write.
            (ptr as *mut u32).write_unaligned(val);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_fb(width: u64, height: u64, backing: &mut alloc::vec::Vec<u8>) -> Framebuffer {
        let bpp: u64 = 32;
        let pitch = width * (bpp / 8);
        let size = (pitch * height) as usize;
        *backing = alloc::vec![0u8; size + 4];
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

    #[test]
    fn palette_constants_match_steelbore_hex() {
        assert_eq!(
            MOLTEN_AMBER,
            Color {
                r: 0xD9,
                g: 0x8E,
                b: 0x32
            }
        );
        assert_eq!(
            STEEL_BLUE,
            Color {
                r: 0x4B,
                g: 0x7E,
                b: 0xB0
            }
        );
        assert_eq!(
            RADIUM_GREEN,
            Color {
                r: 0x50,
                g: 0xFA,
                b: 0x7B
            }
        );
    }

    #[test]
    fn from_theme_rgb_copies_components() {
        let rgb = zamak_theme::Rgb {
            r: 0xAB,
            g: 0xCD,
            b: 0xEF,
        };
        let c: Color = rgb.into();
        assert_eq!(
            c,
            Color {
                r: 0xAB,
                g: 0xCD,
                b: 0xEF
            }
        );
    }

    #[test]
    fn canvas_reports_framebuffer_dimensions() {
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(64, 32, &mut backing);
        let canvas = Canvas::new(&mut fb);
        assert_eq!(canvas.width(), 64);
        assert_eq!(canvas.height(), 32);
    }

    #[test]
    fn put_pixel_writes_correct_value_with_mask_shifts() {
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(4, 1, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        canvas.put_pixel(
            0,
            0,
            Color {
                r: 0x12,
                g: 0x34,
                b: 0x56,
            },
        );
        // red_mask_shift=16, green_mask_shift=8, blue_mask_shift=0.
        // Expected word: 0x00_12_34_56 little-endian → bytes 56 34 12 00.
        assert_eq!(&backing[0..4], &[0x56, 0x34, 0x12, 0x00]);
    }

    #[test]
    fn put_pixel_out_of_bounds_does_not_panic() {
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(4, 4, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        canvas.put_pixel(4, 0, MOLTEN_AMBER); // x == width
        canvas.put_pixel(0, 4, MOLTEN_AMBER); // y == height
        canvas.put_pixel(999, 999, MOLTEN_AMBER);
        assert!(backing[..16].iter().all(|&b| b == 0));
    }

    #[test]
    fn clear_fills_entire_framebuffer() {
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(4, 2, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        canvas.clear(RED_OXIDE);
        // 4×2 pixels × 4 bytes/pixel = 32 bytes. Each pixel's blue byte
        // is at offset 0, red at offset 2.
        assert_eq!(backing[0], 0x5C, "blue byte of first pixel");
        assert_eq!(backing[2], 0xFF, "red byte of first pixel");
        assert_eq!(backing[28], 0x5C, "blue byte of last pixel");
    }

    #[test]
    fn draw_rect_fills_exact_region() {
        let mut backing = alloc::vec::Vec::new();
        let mut fb = mk_fb(4, 4, &mut backing);
        let mut canvas = Canvas::new(&mut fb);
        // Fill a 2x2 rect starting at (1, 1) with green.
        canvas.draw_rect(1, 1, 2, 2, RADIUM_GREEN);
        // Pixel (0, 0) should still be zero.
        assert_eq!(&backing[0..4], &[0, 0, 0, 0]);
        // Pixel (1, 1) is at offset y*pitch + x*4 = 1*16 + 1*4 = 20.
        assert_eq!(backing[20 + 2], 0x50, "red at (1,1)");
        assert_eq!(backing[20 + 1], 0xFA, "green at (1,1)");
    }
}

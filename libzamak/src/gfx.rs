// SPDX-License-Identifier: GPL-3.0-or-later

use crate::protocol::Framebuffer;

#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

// Steelbore Palette
pub const VOID_NAVY: Color = Color { r: 0x00, g: 0x00, b: 0x27 };
pub const MOLTEN_AMBER: Color = Color { r: 0xD9, g: 0x8E, b: 0x32 };
pub const STEEL_BLUE: Color = Color { r: 0x4B, g: 0x7E, b: 0xB0 };
pub const RADIUM_GREEN: Color = Color { r: 0x50, g: 0xFA, b: 0x7B };
pub const RED_OXIDE: Color = Color { r: 0xFF, g: 0x5C, b: 0x5C };
pub const LIQUID_COOLANT: Color = Color { r: 0x8B, g: 0xE9, b: 0xFD };

pub struct Canvas<'a> {
    fb: &'a mut Framebuffer,
}

impl<'a> Canvas<'a> {
    pub fn new(fb: &'a mut Framebuffer) -> Self {
        Self { fb }
    }

    #[inline]
    pub fn width(&self) -> u64 { self.fb.width }

    #[inline]
    pub fn height(&self) -> u64 { self.fb.height }

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
            
            *(ptr as *mut u32) = val;
        }
    }
}

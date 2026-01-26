// SPDX-License-Identifier: GPL-3.0-or-later

use libzamak::tui::{InputSource, Key};
use crate::utils;

pub struct BiosInput;

impl InputSource for BiosInput {
    fn read_key(&mut self) -> Key {
        let mut status: u8;
        // Poll for input (non-blocking in a real loop, but here we scan once per call)
        // Check Status Register (0x64)
        unsafe {
            core::arch::asm!("in al, 0x64", out("al") status);
        }

        // Bit 0 = Output Buffer Full
        if (status & 1) == 0 {
            return Key::None;
        }

        let mut scancode: u8;
        unsafe {
            core::arch::asm!("in al, 0x60", out("al") scancode);
        }

        // Basic scan code set 1 mapping
        match scancode {
            0x48 => Key::Up,
            0x50 => Key::Down,
            0x1C => Key::Enter, // Enter
            0x01 => Key::Esc,   // Esc
            0x12 => Key::Edit,  // 'e'
            0x17 => Key::Char('i'), // 'i'
            0x1E => Key::Char('a'),
            0x30 => Key::Char('b'),
            // ... truncated mapping for brevity
            _ => Key::None,
        }
    }
}

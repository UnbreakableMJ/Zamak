// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! PS/2 keyboard input via x86 I/O ports for the BIOS boot path.
//!
//! Uses safe port I/O wrappers from `zamak_core::arch::x86`.

// Rust guideline compliant 2026-03-30

use zamak_core::arch::x86::inb;
use zamak_core::tui::{InputSource, Key};

/// PS/2 keyboard controller status port.
const KB_STATUS_PORT: u16 = 0x64;

/// PS/2 keyboard data port.
const KB_DATA_PORT: u16 = 0x60;

pub struct BiosInput;

impl InputSource for BiosInput {
    fn read_key(&mut self) -> Key {
        let status = inb(KB_STATUS_PORT);

        // Bit 0 = Output Buffer Full — no key available if clear.
        if (status & 1) == 0 {
            return Key::None;
        }

        let scancode = inb(KB_DATA_PORT);

        // Scan code set 1 mapping (make codes only).
        match scancode {
            0x48 => Key::Up,
            0x50 => Key::Down,
            0x1C => Key::Enter,
            0x01 => Key::Esc,
            0x12 => Key::Edit, // 'e'
            0x17 => Key::Char('i'),
            0x1E => Key::Char('a'),
            0x30 => Key::Char('b'),
            _ => Key::None,
        }
    }
}

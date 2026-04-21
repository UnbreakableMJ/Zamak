// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ZAMAK BIOS Stage 1 — 512-byte MBR boot sector.
//!
//! This is a pure `global_asm!` binary with no Rust runtime. The BIOS
//! loads this 512-byte sector at physical address 0x7C00, and it:
//!
//! 1. Initializes segment registers and stack
//! 2. Reads Stage 2 (zamak-decompressor) from disk via INT 13h Extended Read
//! 3. Jumps to Stage 2 at 0x8000 with boot drive in DL
//!
//! # Patchable Fields
//!
//! The `zamak install` CLI tool patches two fields at fixed MBR offsets:
//! - Offset 440 (4 bytes): Stage 2 starting LBA on disk
//! - Offset 444 (2 bytes): Stage 2 size in sectors
//!
//! # Boot Chain
//!
//! ```text
//! BIOS → Stage 1 (MBR @ 0x7C00, this crate)
//!      → Stage 2 (decompressor @ 0x8000)
//!      → Stage 3 (zamak-bios @ 0xF000)
//! ```

// Rust guideline compliant 2026-03-30

#![no_std]
#![no_main]

mod mbr;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

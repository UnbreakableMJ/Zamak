// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ZAMAK BIOS stage2 decompressor.
//!
//! Loaded by stage1 (MBR) at 0x70000 in 32-bit protected mode.
//! Decompresses the gzip-compressed `zamak-bios.sys` blob to
//! [`STAGE3_LOAD_ADDR`] using `miniz_oxide`, then jumps to stage3
//! with the boot drive ID.
//!
//! # Boot Chain
//!
//! ```text
//! Stage 1 (MBR @ 0x7C00) → Stage 2 (decompressor @ 0x70000)
//!                           → Stage 3 (zamak-bios @ 0xF000)
//! ```

// Rust guideline compliant 2026-03-30

#![no_std]
#![no_main]

extern crate alloc;

mod bump;
mod entry;

use core::panic::PanicInfo;
use core::slice;
use miniz_oxide::inflate::decompress_to_vec_zlib;

/// Physical address where stage3 (`zamak-bios.sys`) is decompressed.
///
/// Limine uses 0xF000 for the decompressed stage2 destination.
/// We follow the same layout for compatibility with the existing
/// entry point code in `zamak-bios`.
const STAGE3_LOAD_ADDR: u32 = 0xF000;

/// Stack pointer set before jumping to stage3.
///
/// Placed at the same address as the decompression target so the
/// stack grows downward from stage3 code (matching Limine behavior).
const STAGE3_STACK_PTR: u32 = 0xF000;

/// Decompresses the gzip-compressed stage3 blob and jumps to it.
///
/// # Arguments
///
/// * `compressed_data` — Pointer to the gzip-compressed stage3 image.
/// * `compressed_size` — Size of the compressed data in bytes.
/// * `boot_drive` — BIOS drive number (DL value from stage1).
/// * `pxe` — Non-zero if booting via PXE (network boot).
///
/// # Safety
///
/// Called from the entry point assembly. The caller must ensure:
/// - `compressed_data` points to valid memory containing a gzip stream
/// - `compressed_size` is the exact size of the compressed data
/// - The decompressed output fits in memory starting at [`STAGE3_LOAD_ADDR`]
/// - No other code or data occupies the decompression target range
#[no_mangle]
pub unsafe extern "C" fn decompress_and_jump(
    compressed_data: *const u8,
    compressed_size: u32,
    boot_drive: u32,
    pxe: u32,
) -> ! {
    // Initialize the bump allocator for miniz_oxide.
    bump::init();

    let compressed =
        slice::from_raw_parts(compressed_data, compressed_size as usize);

    // Skip the gzip header to get to the raw deflate stream.
    // Gzip format: 10-byte fixed header, optional extra fields.
    let deflate_data = skip_gzip_header(compressed);

    // Decompress the deflate stream.
    let decompressed = decompress_to_vec_zlib(deflate_data)
        .unwrap_or_else(|_| halt_with_error());

    // Copy decompressed data to the target physical address.
    let dest = STAGE3_LOAD_ADDR as *mut u8;
    core::ptr::copy_nonoverlapping(
        decompressed.as_ptr(),
        dest,
        decompressed.len(),
    );

    // Jump to stage3 with boot_drive and pxe flag as arguments.
    //
    // SAFETY:
    //   Preconditions:
    //     - Decompressed stage3 code is valid at STAGE3_LOAD_ADDR
    //     - CPU is in 32-bit protected mode with flat segments
    //   Postconditions:
    //     - Control transfers to stage3; this function never returns
    //   Clobbers:
    //     - All registers (new stack frame established)
    //   Worst-case on violation:
    //     - Triple fault if stage3 code is corrupt
    core::arch::asm!(
        "mov esp, {stack}",
        "xor ebp, ebp",
        "push {pxe}",
        "push {drive}",
        "push 0",
        "push {entry}",
        "ret",
        stack = in(reg) STAGE3_STACK_PTR,
        entry = in(reg) STAGE3_LOAD_ADDR,
        drive = in(reg) boot_drive,
        pxe = in(reg) pxe,
        options(noreturn),
    );
}

/// Skips a gzip header and returns the raw deflate stream.
///
/// Handles the standard 10-byte gzip header plus optional FEXTRA,
/// FNAME, FCOMMENT, and FHCRC fields per RFC 1952.
fn skip_gzip_header(data: &[u8]) -> &[u8] {
    // Minimum gzip header: 10 bytes (magic, method, flags, mtime, xfl, os).
    if data.len() < 10 || data[0] != 0x1F || data[1] != 0x8B {
        // Not gzip — assume raw deflate/zlib stream.
        return data;
    }

    let flags = data[3];
    let mut offset = 10;

    // FEXTRA (bit 2): 2-byte length prefix + extra data.
    if flags & (1 << 2) != 0 {
        if offset + 2 > data.len() {
            return data;
        }
        let xlen =
            u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2 + xlen;
    }

    // FNAME (bit 3): null-terminated original filename.
    if flags & (1 << 3) != 0 {
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }
        offset += 1; // Skip the null terminator.
    }

    // FCOMMENT (bit 4): null-terminated comment.
    if flags & (1 << 4) != 0 {
        while offset < data.len() && data[offset] != 0 {
            offset += 1;
        }
        offset += 1;
    }

    // FHCRC (bit 1): 2-byte CRC16 of header.
    if flags & (1 << 1) != 0 {
        offset += 2;
    }

    if offset >= data.len() {
        return data;
    }

    &data[offset..]
}

/// Halts the CPU with a visible error indicator.
///
/// Writes 'E' to VGA text buffer position (0,0) and halts. This provides
/// a minimal visual error signal when decompression fails.
fn halt_with_error() -> ! {
    // VGA text mode buffer at 0xB8000.
    // Write 'E' in bright red on black.
    let vga = 0xB8000 as *mut u16;
    unsafe {
        // 0x4F = white on red background — highly visible.
        core::ptr::write_volatile(vga, 0x4F45); // 'E' with red bg + white fg
        core::ptr::write_volatile(vga.add(1), 0x4F52); // 'R'
        core::ptr::write_volatile(vga.add(2), 0x4F52); // 'R'
    }
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    halt_with_error()
}

#[no_mangle]
pub extern "C" fn rust_eh_personality() {}

// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Stage2 decompressor entry point assembly.
//!
//! The MBR (stage1) jumps here after loading the decompressor to 0x70000.
//! This code zeroes BSS, sets up the stack, and calls
//! [`decompress_and_jump`](super::decompress_and_jump) with the compressed
//! stage3 data pointer, its size, the boot drive, and PXE flag.
//!
//! The compressed stage3 blob is appended immediately after the
//! decompressor binary by the build tooling. Its location and size
//! are passed by stage1 via registers:
//!
//! - `ESI` — pointer to compressed stage3 data
//! - `ECX` — size of compressed stage3 data in bytes
//! - `EDX` — BIOS boot drive number (low byte)
//! - `EBX` — PXE flag (0 = disk boot, 1 = PXE boot)

// Rust guideline compliant 2026-03-30

use core::arch::global_asm;

// SAFETY:
//   Preconditions:
//     - CPU is in 32-bit protected mode with flat segments (set by stage1)
//     - ESI = compressed stage3 pointer, ECX = compressed size
//     - EDX = boot drive (low byte), EBX = PXE flag
//     - Code is loaded at 0x70000 (linker script origin)
//   Postconditions:
//     - BSS is zeroed
//     - `decompress_and_jump` is called with C ABI arguments
//   Clobbers:
//     - EAX, ECX, EDI (BSS zeroing); all others preserved for call
//   Worst-case on violation:
//     - Triple fault if segments are invalid or load address is wrong
global_asm!(
    ".intel_syntax noprefix",
    ".section .entry, \"ax\"",
    ".code32",
    ".global _start",
    "_start:",
    "    cld",
    "",
    // Save register arguments before BSS zeroing clobbers ECX.
    "    push ebx", // PXE flag
    "    push edx", // boot drive
    "    push ecx", // compressed size
    "    push esi", // compressed data pointer
    "",
    // Zero out BSS.
    "    xor eax, eax",
    "    mov edi, offset __bss_start",
    "    mov ecx, offset __bss_end",
    "    sub ecx, offset __bss_start",
    "    rep stosb",
    "",
    // Call decompress_and_jump(compressed_data, compressed_size, boot_drive, pxe).
    // Arguments are already on the stack in correct cdecl order.
    "    call decompress_and_jump",
    "",
    // Should never return — halt if it does.
    ".Lhalt:",
    "    hlt",
    "    jmp .Lhalt",
    ".att_syntax prefix",
);

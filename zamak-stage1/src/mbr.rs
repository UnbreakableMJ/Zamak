// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! 512-byte MBR boot sector assembly.
//!
//! All code resides in `global_asm!` per Steelbore Standard §3.2.
//! Uses `.pushsection`/`.popsection` per §3.9.8 for position-sensitive
//! placement at exactly 512 bytes.

// Rust guideline compliant 2026-03-30

use core::arch::global_asm;

// SAFETY:
//   Preconditions:
//     - BIOS has loaded this 512-byte sector at physical address 0x7C00
//     - DL contains the BIOS boot drive number
//     - CPU is in 16-bit real mode
//   Postconditions:
//     - Stage 2 is loaded at 0x8000 and control is transferred there
//     - DL still contains the boot drive number
//   Clobbers:
//     - All general-purpose registers (entry point, never returns)
//   Worst-case on violation:
//     - Disk read error message displayed, CPU halted
//     - Triple fault if memory at 0x7C00 is not mapped
// §3.9.1 justification: The MBR (~25 instructions + data + DAP + partition table)
// must fit in exactly 512 bytes as a single contiguous block. The boot signature
// at offset 510 and patchable fields at fixed offsets require .org directives
// that only work within one assembly section.
// NOTE: rustc's `global_asm!` default is Intel syntax on x86 targets, so
// no `.intel_syntax noprefix` / `.att_syntax prefix` directives are
// emitted around this block — newer nightlies warn about redundant
// switches and our CI runs with `-D warnings`.
global_asm!(
    ".pushsection .mbr, \"ax\"",
    ".code16",
    ".global mbr_start",
    "mbr_start:",
    "    ljmp 0x0000, offset .Lmbr_init",
    "",
    ".Lmbr_init:",
    "    cli",
    "    xor ax, ax",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "    mov sp, 0x7c00",
    "    sti",
    "",
    "    mov [.Lboot_drive], dl",
    "",
    // Print welcome message.
    "    mov si, offset .Lmsg_welcome",
    "    call .Lprint_string",
    "",
    // Load Stage 2 from disk using INT 13h Extended Read.
    "    mov eax, [.Lstage2_lba]",
    "    mov [.Ldap_lba], eax",
    "    mov cx, [.Lstage2_size]",
    "    mov [.Ldap_count], cx",
    "",
    "    mov ah, 0x42",
    "    mov dl, [.Lboot_drive]",
    "    mov si, offset .Ldap",
    "    int 0x13",
    "    jc  .Lmbr_read_error",
    "",
    // Jump to Stage 2 at 0x8000.
    "    mov si, offset .Lmsg_jumping",
    "    call .Lprint_string",
    "",
    "    mov dl, [.Lboot_drive]",
    "    ljmp 0x0000, 0x8000",
    "",
    ".Lmbr_halt:",
    "    cli",
    "    hlt",
    "    jmp .Lmbr_halt",
    "",
    ".Lmbr_read_error:",
    "    mov si, offset .Lmsg_err_read",
    "    call .Lprint_string",
    "    jmp .Lmbr_halt",
    "",
    // print_string: SI = pointer to NUL-terminated string.
    ".Lprint_string:",
    "    mov ah, 0x0e",
    ".Lprint_loop:",
    "    lodsb",
    "    test al, al",
    "    jz   .Lprint_done",
    "    int  0x10",
    "    jmp  .Lprint_loop",
    ".Lprint_done:",
    "    ret",
    "",
    // Data.
    ".Lboot_drive: .byte 0",
    ".Lmsg_welcome:  .asciz \"ZAMAK Stage 1\\r\\n\"",
    ".Lmsg_jumping:  .asciz \"Loading Stage 2...\\r\\n\"",
    ".Lmsg_err_read: .asciz \"Disk Read Error!\\r\\n\"",
    "",
    // Disk Address Packet (DAP) for INT 13h Extended Read.
    ".align 4",
    ".Ldap:",
    "    .byte 0x10", // DAP size (16 bytes)
    "    .byte 0",    // Reserved
    ".Ldap_count:",
    "    .word 0",      // Sector count (patched at runtime)
    "    .word 0x8000", // Destination offset (Stage 2 load address)
    "    .word 0x0000", // Destination segment
    ".Ldap_lba:",
    "    .quad 1", // Start LBA (patched at runtime)
    "",
    // Patchable fields at fixed MBR offsets.
    // The `zamak install` CLI patches these after writing the MBR.
    ".org 440",                // Offset 440: stage2 location
    ".Lstage2_lba:  .long 1",  // LBA of Stage 2 (patched by installer)
    ".Lstage2_size: .word 32", // Size in sectors (patched by installer)
    "",
    // Partition table area (offset 446, 64 bytes).
    ".org 446",
    ".fill 64, 1, 0",
    "",
    // Boot signature at offset 510.
    ".word 0xAA55",
    ".popsection",
);

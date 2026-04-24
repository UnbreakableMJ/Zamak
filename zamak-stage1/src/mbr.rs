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
    // Print boot drive (DL) as two hex digits for debugging.
    "    mov si, offset .Lmsg_drive",
    "    call .Lprint_string",
    "    mov bl, [.Lboot_drive]",
    "    shr bl, 4",
    "    call .Lprint_hex_nibble",
    "    mov bl, [.Lboot_drive]",
    "    and bl, 0x0f",
    "    call .Lprint_hex_nibble",
    "    mov al, 0x0d",
    "    mov dx, 0x3F8",
    "    out dx, al",
    "    mov al, 0x0a",
    "    out dx, al",
    "",
    // First verify Extended INT 13h is supported (AH=0x41).
    "    mov ah, 0x41",
    "    mov bx, 0x55aa",
    "    mov dl, [.Lboot_drive]",
    "    int 0x13",
    "    jc  .Lmbr_no_ext",
    "    cmp bx, 0xaa55",
    "    jne .Lmbr_no_ext",
    // Load Stage 2 in 64-sector (32 KiB) chunks so no single INT 13h
    // transfer crosses a 64 KiB real-mode segment boundary — SeaBIOS
    // returns AH=0x0E ("media error") if it does. Chunks start at
    // seg=0x0800,off=0 (phys 0x8000) and advance by 32 KiB of segment
    // per iteration.
    "    mov eax, [.Lstage2_lba]",
    "    mov [.Ldap_lba], eax",
    "    mov cx, [.Lstage2_size]",
    "    mov word ptr [.Ldap_offset], 0",
    "    mov word ptr [.Ldap_segment], 0x0800",
    ".Lread_loop:",
    "    test cx, cx",
    "    jz .Lread_done",
    "    mov ax, 64",
    "    cmp cx, 64",
    "    jae 1f",
    "    mov ax, cx",
    "1:  mov [.Ldap_count], ax",
    "    push cx",
    "    mov ah, 0x42",
    "    mov dl, [.Lboot_drive]",
    "    mov si, offset .Ldap",
    "    int 0x13",
    "    pop cx",
    "    jc .Lmbr_read_error",
    // Advance LBA (+= count, 32-bit), segment (+= count*32), and
    // decrement remaining.
    "    movzx eax, word ptr [.Ldap_count]",
    "    add [.Ldap_lba], eax",
    "    shl ax, 5",
    "    add [.Ldap_segment], ax",
    "    sub cx, [.Ldap_count]",
    "    jmp .Lread_loop",
    ".Lread_done:",
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
    ".Lmbr_no_ext:",
    "    mov si, offset .Lmsg_no_ext",
    "    call .Lprint_string",
    "    jmp .Lmbr_halt",
    "",
    ".Lmbr_read_error:",
    "    mov si, offset .Lmsg_err_read",
    "    call .Lprint_string",
    // Dump the INT 13h status byte (AH) to serial as two hex digits so
    // we can tell what the BIOS didn't like.
    "    mov bl, ah",
    "    shr bl, 4",
    "    call .Lprint_hex_nibble",
    "    mov bl, ah",
    "    and bl, 0x0f",
    "    call .Lprint_hex_nibble",
    "    mov al, '\\r'",
    "    mov dx, 0x3F8",
    "    out dx, al",
    "    mov al, '\\n'",
    "    out dx, al",
    "    jmp .Lmbr_halt",
    "",
    // print_hex_nibble: BL = low nibble; writes the ASCII char to COM1.
    ".Lprint_hex_nibble:",
    "    and bl, 0x0f",
    "    add bl, '0'",
    "    cmp bl, '9'",
    "    jbe 1f",
    "    add bl, 7",
    "1:",
    "    mov al, bl",
    "    mov dx, 0x3F8",
    "    out dx, al",
    "    ret",
    "",
    // print_string: SI = pointer to NUL-terminated string.
    //
    // Writes each byte to both the BIOS teletype (INT 10h AH=0Eh,
    // for real hardware with a display) AND COM1 (0x3F8, for QEMU
    // `-serial stdio` and serial-console setups). QEMU's 16550A
    // emulation accepts raw byte writes without UART init, so we
    // skip the divisor-latch setup.
    ".Lprint_string:",
    ".Lprint_loop:",
    "    lodsb",
    "    test al, al",
    "    jz   .Lprint_done",
    "    mov  ah, 0x0e",
    "    mov  bx, 0x0007",
    "    int  0x10",
    "    mov  dx, 0x3F8",
    "    out  dx, al",
    "    jmp  .Lprint_loop",
    ".Lprint_done:",
    "    ret",
    "",
    // Data.
    ".Lboot_drive: .byte 0",
    ".Lmsg_welcome:  .asciz \"ZAMAK Stage 1\\r\\n\"",
    ".Lmsg_drive:    .asciz \"DL=\"",
    ".Lmsg_jumping:  .asciz \"Loading Stage 2...\\r\\n\"",
    ".Lmsg_err_read: .asciz \"Disk Read Error!\\r\\nAH=\"",
    ".Lmsg_no_ext:   .asciz \"INT 13h extensions NOT supported!\\r\\n\"",
    "",
    // Disk Address Packet (DAP) for INT 13h Extended Read.
    ".align 4",
    ".Ldap:",
    "    .byte 0x10", // DAP size (16 bytes)
    "    .byte 0",    // Reserved
    ".Ldap_count:",
    "    .word 0", // Sector count (patched at runtime)
    ".Ldap_offset:",
    "    .word 0x8000", // Destination offset (patched: see multi-read loop)
    ".Ldap_segment:",
    "    .word 0x0000", // Destination segment (patched: see multi-read loop)
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

// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Stub bzImage for the `linux-bzimage` smoke suite (M2-12).
//!
//! This is NOT a real Linux kernel — it's the smallest blob that both
//! (a) passes `zamak-core::linux_boot::parse_setup_header` validation
//! so `zamak-uefi` exercises its full Linux-Boot-Protocol plumbing
//! (`PROTOCOL=linux` dispatch, `BootParams` population, RSI-aware
//! handoff), and (b) prints `Linux version 0.0.0-zamak-stub` on COM1
//! so `zamak-test`'s `["ZAMAK", "Linux version"]` sentinels match.
//!
//! # Binary layout
//!
//! After `objcopy -O binary`, the output file is byte-for-byte a
//! minimal bzImage:
//!
//! | file offset | contents |
//! |---|---|
//! | `0x000..0x1F1` | real-mode boot-sector padding (halts if anyone tries BIOS boot) |
//! | `0x1F1..0x260` | setup header with magic `HdrS`, protocol 2.15 |
//! | `0x260..0x400` | setup-sector padding (1 + `setup_sects = 1` = 2 sectors total) |
//! | `0x400..0x600` | kernel-body padding (pre-64-bit-entry header slack) |
//! | `0x600..` | 64-bit entry — prints the version string + exits QEMU |
//!
//! `parse_setup_header` in `zamak-core::linux_boot` requires:
//! - size ≥ 0x260
//! - magic `HdrS` at 0x202
//! - version ≥ 0x0206
//! - nonzero `setup_sects`
//! - `bzimage.len() > setup_size`
//!
//! All satisfied by the `global_asm!` layout below.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

// The entire bzImage layout — setup sectors + kernel body — is in one
// `global_asm!` block placed into a `.boot` linker section. The linker
// script at the top of the ELF ensures `.boot` is the first / only
// section, so `objcopy -O binary` emits these bytes verbatim.
core::arch::global_asm!(
    r#"
    .section .boot, "awx"
    .code16
    .global _start
_start:
    # Legacy real-mode entry. Nothing should boot us as an MBR, but if
    # it does, halt cleanly — don't execute random bytes from the
    # setup header as instructions.
    cli
1:  hlt
    jmp 1b

    # ============================================================
    # Pad to offset 0x1F1 — first byte of the protocol setup header.
    # ============================================================
    .fill 0x1F1 - (. - _start), 1, 0

    # setup_sects (offset 0x1F1): total setup = (1 + setup_sects) * 512
    # = 0x400 bytes. One sector of header-and-padding, one sector of
    # setup-sector-padding, so the kernel body starts at 0x400.
    .byte 1
    # root_flags (0x1F2)
    .word 0
    # syssize (0x1F4): size of the 32-bit/64-bit "kernel" body in
    # 16-byte paragraphs. Our body is very small but must be > 0;
    # declare a generous value — `parse_setup_header` reads the field
    # but doesn't strictly validate it against the actual length, the
    # loader uses `kernel_size(header, bzimage_len)` instead which
    # derives size from the total file size minus setup_size.
    .long 0x40
    # ram_size (0x1F8) — obsolete
    .word 0
    # vid_mode (0x1FA)
    .word 0xFFFF
    # root_dev (0x1FC)
    .word 0
    # boot_flag (0x1FE) — 0xAA55
    .word 0xAA55

    # ============================================================
    # Protocol header proper — offset 0x200.
    # ============================================================
    # jump (0x200): "jmp setup_entry" — two bytes (EB xx). We never
    # execute in real mode; any two bytes suffice.
    .byte 0xEB, 0x66
    # header magic (0x202): "HdrS"
    .ascii "HdrS"
    # version (0x206): 0x020F = Linux boot protocol 2.15
    .word 0x020F
    # realmode_swtch (0x208)
    .long 0
    # start_sys_seg (0x20C) — obsolete
    .word 0
    # kernel_version (0x20E)
    .word 0
    # type_of_loader (0x210): overwritten by bootloader
    .byte 0
    # loadflags (0x211): LOADED_HIGH = 0x01
    .byte 0x01
    # setup_move_size (0x212)
    .word 0
    # code32_start (0x214): overwritten by bootloader to actual
    # physical load address. Default to 1 MiB.
    .long 0x100000
    # ramdisk_image (0x218) / ramdisk_size (0x21C) — bootloader fills
    .long 0
    .long 0
    # bootsect_kludge (0x220) — obsolete
    .long 0
    # heap_end_ptr (0x224)
    .word 0
    # ext_loader_ver (0x226)
    .byte 0
    # ext_loader_type (0x227)
    .byte 0
    # cmd_line_ptr (0x228) — bootloader fills
    .long 0
    # initrd_addr_max (0x22C) — 0xFFFFFFFF means "anywhere below 4 GiB"
    .long 0xFFFFFFFF
    # kernel_alignment (0x230): 2 MiB
    .long 0x200000
    # relocatable_kernel (0x234): 1 = yes, loader may choose any
    # 2-MiB-aligned address
    .byte 1
    # min_alignment (0x235): log2(2 MiB) = 21
    .byte 21
    # xloadflags (0x236): bit 0 = kernel has 64-bit entry at 0x200;
    # bit 3 = can be loaded above 4 GiB
    .word 0x09
    # cmdline_size (0x238): max cmdline length our kernel "accepts"
    .long 0x1000
    # hardware_subarch (0x23C)
    .long 0
    # hardware_subarch_data (0x240)
    .quad 0
    # payload_offset (0x248)
    .long 0
    # payload_length (0x24C)
    .long 0
    # setup_data (0x250)
    .quad 0
    # pref_address (0x258): preferred load phys addr = 16 MiB
    .quad 0x1000000
    # init_size (0x260): decompressed kernel size estimate
    .long 0x10000
    # handover_offset (0x264): 0 = no EFI handover
    .long 0
    # kernel_info_offset (0x268)
    .long 0

    # ============================================================
    # Pad to end of setup sectors (offset 0x400 = (1 + 1) * 512).
    # ============================================================
    .fill 0x400 - (. - _start), 1, 0

    # ============================================================
    # Kernel body. 64-bit entry lives at body+0x200 → file offset
    # 0x600 per Linux boot protocol 2.12+.
    # ============================================================
    .fill 0x200, 1, 0

    .code64
.Lkernel_64_entry:
    # Print "Linux version 0.0.0-zamak-stub\n" to COM1 (0x3F8).
    lea rsi, [rip + .Lstub_message]
1:  mov al, [rsi]
    test al, al
    jz 2f
    mov dx, 0x3F8
    out dx, al
    inc rsi
    jmp 1b

    # Exit QEMU via isa-debug-exit (port 0x501, value 0x31 → QEMU
    # exits with code 0x63, which zamak-test treats as pass).
2:  mov al, 0x31
    mov dx, 0x501
    out dx, al

    # If QEMU is not present, halt so the test harness watchdog can
    # fire instead of triple-faulting.
3:  hlt
    jmp 3b

.Lstub_message:
    .asciz "Linux version 0.0.0-zamak-stub\n"
"#
);

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

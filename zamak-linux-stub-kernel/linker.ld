/*
 * SPDX-License-Identifier: GPL-3.0-or-later
 * SPDX-FileCopyrightText: 2026 Mohamed Hammad
 *
 * Linker script for zamak-linux-stub-kernel.
 *
 * Produces an ELF whose single `.boot` section holds the entire
 * bzImage-format blob (setup sectors + kernel body) laid out with
 * hand-rolled `.byte` / `.fill` directives from the `global_asm!`
 * block in src/main.rs. build-images.sh runs `objcopy -O binary` to
 * strip the ELF wrapper and leaves a raw bzImage.
 *
 * The kernel-body 64-bit entry lives at raw-file offset 0x600, which
 * corresponds to `kernel_load_phys + 0x200` at run time (kernel body
 * starts at bzImage offset `(1 + setup_sects) * 512 = 0x400` for
 * `setup_sects = 1`, and the 64-bit entry-point is at +0x200 per
 * Linux boot protocol 2.12+).
 */

OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

SECTIONS {
    . = 0x0;

    .boot : ALIGN(1) {
        KEEP(*(.boot))
        KEEP(*(.boot.*))
    }

    /DISCARD/ : {
        *(.note.* .comment .eh_frame .eh_frame_hdr .got .got.plt)
        *(.dynsym .dynstr .hash .gnu.hash .dynamic)
    }
}

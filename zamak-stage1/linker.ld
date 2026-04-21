/* SPDX-License-Identifier: GPL-3.0-or-later */

/*
 * Linker script for zamak-stage1 (BIOS MBR boot sector).
 *
 * Origin: 0x7C00 — BIOS loads the MBR here.
 * Output: exactly 512 bytes after objcopy -O binary.
 */

OUTPUT_FORMAT(elf32-i386)
ENTRY(mbr_start)

SECTIONS
{
    . = 0x7C00;

    .mbr : {
        KEEP(*(.mbr))
    }

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
        *(.text .text.*)
        *(.rodata .rodata.*)
        *(.data .data.*)
        *(.bss .bss.*)
    }
}

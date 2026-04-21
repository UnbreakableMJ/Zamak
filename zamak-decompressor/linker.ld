/* SPDX-License-Identifier: GPL-3.0-or-later */

/*
 * Linker script for zamak-decompressor (BIOS stage2).
 *
 * Origin: 0x70000 — MBR stage1 loads the compressed decompressor here.
 * The decompressor inflates zamak-bios.sys to STAGE3_LOAD_ADDR (0xF000)
 * and jumps to it.
 */

OUTPUT_FORMAT(elf32-i386)
ENTRY(_start)

SECTIONS
{
    . = 0x70000;

    .text : {
        KEEP(*(.entry))
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /DISCARD/ : {
        *(.eh_frame*)
        *(.note .note.*)
        *(.comment)
    }
}

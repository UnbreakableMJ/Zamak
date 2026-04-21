/*
 * SPDX-License-Identifier: GPL-3.0-or-later
 * SPDX-FileCopyrightText: 2026 Mohamed Hammad
 *
 * Linker script for zamak-test-kernel.
 *
 * The kernel is mapped at the standard Limine Protocol kernel virtual
 * base (0xFFFF_FFFF_8000_0000). Request sections are kept in a dedicated
 * .limine_requests area so the bootloader's request scanner can find
 * them by marker. See zamak-proto::START_MARKER / END_MARKER.
 */

OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

KERNEL_VIRT_BASE = 0xFFFFFFFF80000000;

SECTIONS {
    . = KERNEL_VIRT_BASE + 0x100000;

    .text : ALIGN(4K) {
        *(.text .text.*)
    }

    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    }

    .limine_requests_start : ALIGN(8) {
        KEEP(*(.limine_requests_start))
    }

    .limine_requests : ALIGN(8) {
        KEEP(*(.limine_requests))
    }

    .limine_requests_end : ALIGN(8) {
        KEEP(*(.limine_requests_end))
    }

    .data : ALIGN(4K) {
        *(.data .data.*)
    }

    .bss : ALIGN(4K) {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ : {
        *(.note.* .comment .eh_frame .eh_frame_hdr)
    }
}

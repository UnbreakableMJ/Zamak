ENTRY(_start)

SECTIONS
{
    . = 0x8000;

    .entry : {
        KEEP(*(.entry))
    }

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    }

    .trampoline : {
        KEEP(*(.trampoline))
    }

    .mbr : {
        KEEP(*(.mbr))
    }

    /DISCARD/ : {
        *(.eh_frame)
        *(.comment)
    }
}

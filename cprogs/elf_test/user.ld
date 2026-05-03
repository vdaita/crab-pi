ENTRY(_start)

USER_BASE = 0x10000000;

SECTIONS
{
    . = USER_BASE;

    .text : ALIGN(4)
    {
        *(.text.boot)
        *(.text*)
    }

    .rodata : ALIGN(4)
    {
        *(.rodata*)
    }

    .data : ALIGN(4)
    {
        *(.data*)
    }

    .bss : ALIGN(4)
    {
        *(.bss*)
        *(COMMON)
    }
}

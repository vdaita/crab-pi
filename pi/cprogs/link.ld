ENTRY(_start)

SECTIONS {
    . = 0x03000000;

    .text : {
        *(.text.unlikely .text.*_unlikely .text.unlikely.*)
        *(.text.exit .text.exit.*)
        *(.text.startup .text.startup.*)
        *(.text.hot .text.hot.*)
        *(.text .stub .text.* .gnu.linkonce.t.*)
    }

    .init : {
        KEEP (*crti.o(.init))
        KEEP (*(.init))
        KEEP (*crtn.o(.init))
    }

    .fini : {
        KEEP (*crti.o(.fini))
        KEEP (*(.fini))
        KEEP (*crtn.o(.fini))
    }

    .init_array : {
        __init_array_start = .;
        KEEP (*crti.o(.init_array))
        KEEP (*(SORT(.init_array.*)))
        KEEP (*(.init_array))
        KEEP (*crtn.o(.init_array))
        __init_array_end = .;
    }

    .preinit_array : {
        __preinit_array_start = .;
        KEEP (*(.preinit_array*))
        __preinit_array_end = .;
    }

    .fini_array : {
        __fini_array_start = .;
        KEEP (*crti.o(.fini_array))
        KEEP (*(SORT(.fini_array.*)))
        KEEP (*(.fini_array))
        KEEP (*crtn.o(.fini_array))
        __fini_array_end = .;
    }

    .rodata : {
        *(.rodata .rodata.* .gnu.linkonce.r.*)
    }

    .ARM.extab : {
        *(.ARM.extab* .gnu.linkonce.armextab.*)
    }

    .ARM.exidx : {
        __exidx_start = .;
        *(.ARM.exidx* .gnu.linkonce.armexidx.*)
        __exidx_end = .;
    }

    .eh_frame : {
        KEEP (*(.eh_frame))
    }

    .got : {
        *(.got)
    }

    .got.plt : {
        *(.got.plt)
    }

    .data : {
        *(.data .data.* .gnu.linkonce.d.*)
    }

    .bss : {
        __bss_start = .;
        *(.dynbss)
        *(.bss .bss.* .gnu.linkonce.b.*)
        *(COMMON)
        __bss_end = .;
    }

    /DISCARD/ : {
        *(.note.GNU-stack)
        *(.gnu_debuglink)
        *(.gnu.lto_*)
    }
}
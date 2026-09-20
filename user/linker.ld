ENTRY(_start)

PHDRS {
    text PT_LOAD FLAGS(5);    /* R_X */
    rodata PT_LOAD FLAGS(4);  /* R__ */
    data PT_LOAD FLAGS(6);    /* RW_ */
}

SECTIONS {
    . = 0x400000; /* Start at 4MB */

    .text : {
        *(.text.entry)
        *(.text .text.*)
    } :text

    . = ALIGN(4096);

    .rodata : {
        *(.rodata .rodata.*)
    } :rodata

    . = ALIGN(4096);

    .data : {
        *(.data .data.*)
    } :data

    . = ALIGN(4096);

    .bss : {
        *(.bss .bss.*)
    } :data

    . = ALIGN(4096);

    .got : {
        *(.got .got.*)
    } :data

    . = ALIGN(4096);
    __heap_start = .;
    .heap (NOLOAD) : {
        . = . + 0x100000; /* 1 MiB heap, adjust as needed */
    } :data
    __heap_end = .;

    /DISCARD/ : {
        *(.eh_frame)
        *(.note .note.*)
    }
}

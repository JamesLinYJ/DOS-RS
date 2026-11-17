ENTRY(_start)
PHDRS {
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
}
SECTIONS {
    . = DEFINED(__image_base) ? __image_base : 0x60000000;
    .text : { *(.text .text.*) *(.rodata .rodata.*) } :text
    . = DEFINED(__data_base) ? __data_base : ALIGN(4096);
    .data : { *(.data .data.*) } :data
    .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } :data
    /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}

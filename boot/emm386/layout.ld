ENTRY(monitor_start)
trap = 0x270; /* rust16 device ABI: clear DF before entering Rust */
SECTIONS {
    . = 0x800;
    .text : { KEEP(*(.entry)) *(.text .text.*) }
    .rodata : { *(.rodata .rodata.*) }
    .data : { *(.data .data.*) *(.got*) }
    .bss (NOLOAD) : { __zero_start = .; *(.bss .bss.*) *(COMMON) __zero_end = .; }
    ASSERT(. <= 0xb000, "resident tables overlap the monitor stack")
    /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}

ENTRY(_start)
PHDRS { image PT_LOAD FLAGS(7); }
SECTIONS {
  . = 0x100000;
  __load_start = .;
  .text : { KEEP(*(.multiboot)) *(.text.boot) *(.text .text.*) } :image
  .rodata : ALIGN(16) { *(.rodata .rodata.*) } :image
  .data : ALIGN(16) { *(.data .data.*) *(.got .got.*) } :image
  __load_end = .;
  .bss (NOLOAD) : ALIGN(4096) { *(.bss.boot) *(.bss .bss.*) *(COMMON) . = ALIGN(16); . += 0x10000; __stack_top = .; } :image
  __bss_end = .;
  /DISCARD/ : { *(.eh_frame*) *(.comment) }
}

ENTRY(_start)
SECTIONS {
  . = 0x7e00;
  .start : { KEEP(*(.start)) }
  .text : { *(.text .text.*) }
  .rodata : { *(.rodata .rodata.*) }
  .data : { *(.data .data.*) *(.got*) }
  .bss : { *(.bss .bss.*) }
  ASSERT(. < 0x10000, "BIOS scratch exceeds segment zero")
  /DISCARD/ : { *(.eh_frame*) *(.comment*) }
}

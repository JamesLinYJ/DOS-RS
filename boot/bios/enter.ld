ENTRY(_start)
SECTIONS {
  . = 0x100000;
  .start : { KEEP(*(.start)) }
  .text : { *(.text .text.*) }
  .rodata : { *(.rodata .rodata.*) }
  .data : { *(.data .data.*) *(.got*) }
  .bss : { *(.bss .bss.*) }
  ASSERT(. < 0x200000, "transition exceeds staging reservation")
  /DISCARD/ : { *(.eh_frame*) *(.comment*) }
}

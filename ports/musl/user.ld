ENTRY(_start)
PHDRS {
  headers PT_PHDR PHDRS;
  text PT_LOAD FILEHDR PHDRS FLAGS(5);
  data PT_LOAD FLAGS(6);
  tls PT_TLS FLAGS(4);
}
SECTIONS {
  . = 0x61000000 + SIZEOF_HEADERS;
  .text : { *(.text .text.*) *(.rodata .rodata.*) } :text
  . = ALIGN(4096);
  .init_array : { PROVIDE_HIDDEN(__init_array_start = .); KEEP(*(SORT_BY_INIT_PRIORITY(.init_array.*))) KEEP(*(.init_array)) PROVIDE_HIDDEN(__init_array_end = .); } :data
  .fini_array : { PROVIDE_HIDDEN(__fini_array_start = .); KEEP(*(SORT_BY_INIT_PRIORITY(.fini_array.*))) KEEP(*(.fini_array)) PROVIDE_HIDDEN(__fini_array_end = .); } :data
  .data : { *(.data .data.*) *(.got .got.*) } :data
  .tdata : { *(.tdata .tdata.*) } :data :tls
  .tbss (NOLOAD) : { *(.tbss .tbss.*) } :data :tls
  .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } :data
  PROVIDE(_end = .);
  /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}

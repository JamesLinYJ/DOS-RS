//! Default prompt from `CMD/COMMAND/TCMD2A.ASM`.
//!
//! `print_prompt` looks for a `PROMPT=` value. This environment has none,
//! so it prints the default drive letter from INT 21h AH=19h and `>`
//! (`SYM` in `COMEQU.ASM`).

use core::arch::asm;

const SYM: u8 = b'>';

/// One trip through `print_prompt` when no `PROMPT` string is set.
pub fn print_default_prompt() {
    let drive = default_drive();
    let letter = b'A'.wrapping_add(drive);
    crate::bios::mscon::con_write_bytes(&[letter, SYM]);
}

fn default_drive() -> u8 {
    let mut ax: u16 = 0x1900;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x21", inout("ax") ax);
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x21", inout("ax") ax);
    }
    (ax & 0xff) as u8
}

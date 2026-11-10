//! Default prompt from `CMD/COMMAND/TCMD2A.ASM`.
//!
//! `print_prompt` looks for a `PROMPT=` value. This environment has none,
//! so it prints the default drive letter from INT 21h AH=19h and `>`
//! (`SYM` in `COMEQU.ASM`).

use core::arch::asm;

const SYM: u8 = b'>';

/// `VERSION` from this file. CRLF, the version line, CRLF.
///
/// INT 21h AH=30h returns the major version in AL and the minor in AH.
/// Message 1040 substitutes those two numbers.
pub fn version() {
    let (major, minor) = dos_version();
    crate::bios::mscon::con_write_bytes(b"\r\n");
    crate::bios::mscon::con_write_bytes(super::tdata::VER_MESSAGE);
    put_dec(major);
    crate::bios::mscon::con_write_bytes(b".");
    put_dec(minor);
    crate::bios::mscon::con_write_bytes(b"\r\n\r\n");
}

fn dos_version() -> (u8, u8) {
    let mut eax: u32 = 0x3000;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    ((eax & 0xff) as u8, ((eax >> 8) & 0xff) as u8)
}

fn put_dec(mut value: u8) {
    let mut buf = [0u8; 3];
    let mut n = 0usize;
    if value == 0 {
        crate::bios::mscon::con_write_bytes(b"0");
        return;
    }
    while value != 0 {
        buf[n] = b'0' + (value % 10);
        n += 1;
        value /= 10;
    }
    while n != 0 {
        n -= 1;
        crate::bios::mscon::con_write_bytes(&buf[n..n + 1]);
    }
}

/// One trip through `print_prompt` when no `PROMPT` string is set.
pub fn print_default_prompt() {
    let drive = default_drive();
    let letter = b'A'.wrapping_add(drive);
    crate::bios::mscon::con_write_bytes(&[letter, SYM]);
}

fn default_drive() -> u8 {
    let mut eax: u32 = 0x1900;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    (eax & 0xff) as u8
}

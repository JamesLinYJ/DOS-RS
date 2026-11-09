//! `BADFIL` from `BIOS/SYSINIT2.ASM`.
//!
//! DS:DX points at an ASCIIZ name. The routine prints `BADLD_PRE`, then
//! each byte of that name with INT 21h AH=2, then `CRLFM` with AH=9.
//! `DoNotShowNum` is already 1 by the time `COMERR` runs, so the line
//! number is not printed.

use super::sysimes;

pub fn badfil(name: &[u8]) {
    print_dollar(sysimes::BADLD_PRE);
    let mut index = 0;
    while index < name.len() {
        let byte = name[index];
        if byte == 0 {
            break;
        }
        con_out(byte);
        index += 1;
    }
    print_dollar(sysimes::CRLFM);
}

fn print_dollar(text: &[u8]) {
    dos_int(0x09, text.as_ptr() as u32);
}

fn con_out(byte: u8) {
    dos_int(0x02, u32::from(byte));
}

fn dos_int(ah: u8, dx: u32) {
    let mut ax = u32::from(ah) << 8;
    let mut edx = dx;
    #[cfg(target_arch = "x86")]
    unsafe {
        core::arch::asm!(
            "int 0x21",
            inout("eax") ax,
            inout("edx") edx,
        );
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "int 0x21",
            inout("eax") ax,
            inout("edx") edx,
        );
    }
    let _ = (ax, edx);
}

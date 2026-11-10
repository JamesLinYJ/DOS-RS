//! `VOLUME` from `CMD/COMMAND/TCMD1B.ASM`.
//!
//! A blank tail asks INT 21h AH=11h for a volume label. This disk has none,
//! so the line is message 1041 for drive A, after the leading blank.

use core::arch::asm;

use crate::bios::mscon::con_write_bytes;

pub fn volume(tail: &[u8]) {
    if !blank(tail) {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    con_write_bytes(b"\r\n");
    con_write_bytes(b" ");
    let found = search_volume();
    if found == 0 {
        return;
    }
    let drive = b'A'.wrapping_add(default_drive());
    con_write_bytes(b"Volume in drive ");
    con_write_bytes(&[drive]);
    con_write_bytes(b" has no label\r\n");
}

fn search_volume() -> u8 {
    let mut fcb = [0u8; 44];
    fcb[0] = 0xff;
    fcb[6] = 0x08;
    let mut i = 8usize;
    while i < 19 {
        fcb[i] = b'?';
        i += 1;
    }
    let mut eax = 0x1100u32;
    let edx = fcb.as_ptr() as u32;
    unsafe {
        asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            clobber_abi("C"),
        );
    }
    eax as u8
}

fn default_drive() -> u8 {
    let mut eax = 0x1900u32;
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    eax as u8
}

fn blank(tail: &[u8]) -> bool {
    for &byte in tail {
        if byte == b'\r' || byte == b'\n' || byte == 0 {
            return true;
        }
        if byte != b' ' && byte != b'\t' {
            return false;
        }
    }
    true
}

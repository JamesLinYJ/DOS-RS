//! COM entry from `CMD/COMP/COMP1.ASM`.
//!
//! The program is a COM file. DOS loads it at 100h and that word jumps to
//! initialization. The memory-size word stays at 2, the first FCB at 5Ch,
//! and the command tail at 80h.

const ENTRY: u16 = 0x0100;
const MEMORY: u16 = 0x0002;
const FCB: u16 = 0x005C;
const TAIL: u16 = 0x0080;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COMP1 OR");
    put_hex16(ENTRY);
    crate::bios::mscon::con_write_bytes(b" MM");
    put_hex16(MEMORY);
    crate::bios::mscon::con_write_bytes(b" FC");
    put_hex16(FCB);
    crate::bios::mscon::con_write_bytes(b" PA");
    put_hex16(TAIL);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

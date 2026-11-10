//! Switches pulled in by `CMD/COMMAND/COMSW.ASM`.
//!
//! That file includes `VERSION.INC` and nothing else. The included IBM build
//! is on, the IBM copyright is off, and the Microsoft switch is the inverse.

const IBM_VERSION: u16 = 0xFFFF;
const IBM_COPYRIGHT: u16 = 0;
const MICROSOFT: u16 = !IBM_VERSION;
const BUFFER_FLAG: u16 = !IBM_COPYRIGHT;
const INTERNATIONAL: u16 = 0;
const KANJI: u16 = 0;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COMSW IB");
    put_hex16(IBM_VERSION);
    crate::bios::mscon::con_write_bytes(b" CR");
    put_hex16(IBM_COPYRIGHT);
    crate::bios::mscon::con_write_bytes(b" MS");
    put_hex16(MICROSOFT);
    crate::bios::mscon::con_write_bytes(b" BF");
    put_hex16(BUFFER_FLAG);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex16(INTERNATIONAL);
    crate::bios::mscon::con_write_bytes(b" KN");
    put_hex16(KANJI);
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

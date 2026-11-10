//! OEM message constants from `DOS/STDDOSME.ASM`.
//!
//! `debug` is 0. IBM is off, so the OEM number is `FFh`. The country table
//! starts on code page 437 and lists six information entries.

const DEBUG: u8 = 0;
const OEM_NUMBER: u8 = 0xff;
const CODE_PAGE: u16 = 437;
const ENTRIES: u8 = 6;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"STDDOSME DBG");
    put_hex8(DEBUG);
    crate::bios::mscon::con_write_bytes(b" OEM");
    put_hex8(OEM_NUMBER);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(CODE_PAGE);
    crate::bios::mscon::con_write_bytes(b" ENT");
    put_hex8(ENTRIES);
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

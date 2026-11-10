//! Init parser switches from `CMD/COMMAND/IPARSE.ASM`.
//!
//! The system parser is not pulled in. Dates, times, complex lists, keywords,
//! the second and third value forms, quoted strings, and a bare drive are off.

const DATE: u8 = 0;
const TIME: u8 = 0;
const COMPLEX: u8 = 0;
const KEYWORD: u8 = 0;
const VALUE_TWO: u8 = 0;
const VALUE_THREE: u8 = 0;
const QUOTED: u8 = 0;
const DRIVE_ONLY: u8 = 0;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"IPARSE DT");
    put_hex8(DATE);
    crate::bios::mscon::con_write_bytes(b" TM");
    put_hex8(TIME);
    crate::bios::mscon::con_write_bytes(b" CX");
    put_hex8(COMPLEX);
    crate::bios::mscon::con_write_bytes(b" KY");
    put_hex8(KEYWORD);
    crate::bios::mscon::con_write_bytes(b" V2");
    put_hex8(VALUE_TWO);
    crate::bios::mscon::con_write_bytes(b" V3");
    put_hex8(VALUE_THREE);
    crate::bios::mscon::con_write_bytes(b" QU");
    put_hex8(QUOTED);
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(DRIVE_ONLY);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

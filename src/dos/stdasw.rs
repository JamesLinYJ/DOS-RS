//! Alternate-vector switches from `DOS/STDASW.ASM`.
//!
//! `IBM` and `WANG` are off, so escape is `1Bh` and cancel is Ctrl-X.
//! `ALTVECT` is on, so the terminate interrupt is `F0h` instead of `22h`.
//! Printer echo is not a single toggle, and DOS stays at the low end of memory.

const TRUE: u16 = 0xffff;
const FALSE: u16 = !TRUE;

pub const MICROSOFT: u16 = TRUE;
pub const IBM: u16 = FALSE;
pub const WANG: u16 = FALSE;
pub const ALTERNATE_VECTOR: u16 = TRUE;
pub const HIGH_MEMORY: u16 = FALSE;
pub const PRINTER_TOGGLE: u16 = FALSE;
pub const ZERO_EXTEND: u16 = TRUE;
pub const CANCEL: u8 = b'X' - b'@';

/// Escape character. IBM uses 0, Wang uses `1Fh`, and everyone else uses `1Bh`.
pub fn escape(ibm: bool, wang: bool) -> u8 {
    if ibm {
        0
    } else if wang {
        0x1f
    } else {
        0x1b
    }
}

/// `ALTVECT` moves terminate, Ctrl-C, and the fatal abort onto `F0h`.
pub fn terminate_interrupt(alternate: bool) -> u8 {
    if alternate { 0xf0 } else { 0x22 }
}

pub fn show() {
    let escape = escape(IBM != 0, WANG != 0);
    let terminate = terminate_interrupt(ALTERNATE_VECTOR != 0);
    crate::bios::mscon::con_write_bytes(b"STDASW MS");
    put_hex16(MICROSOFT);
    crate::bios::mscon::con_write_bytes(b" IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" WG");
    put_hex16(WANG);
    crate::bios::mscon::con_write_bytes(b" AV");
    put_hex16(ALTERNATE_VECTOR);
    crate::bios::mscon::con_write_bytes(b" HM");
    put_hex16(HIGH_MEMORY);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(escape);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(CANCEL);
    crate::bios::mscon::con_write_bytes(b" PRN");
    put_hex16(PRINTER_TOGGLE);
    crate::bios::mscon::con_write_bytes(b" EXT");
    put_hex16(ZERO_EXTEND);
    crate::bios::mscon::con_write_bytes(b" INT");
    put_hex8(terminate);
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

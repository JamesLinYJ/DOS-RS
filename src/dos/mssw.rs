//! Build switches from `DOS/MSSW.ASM`.
//!
//! `IBMVER` is true, so this copy uses a null escape and Escape as cancel.
//! Printer echo is one key. DOS is not moved to the end of memory.

const TRUE: u16 = 0xffff;
const FALSE: u16 = 0;

pub const IBM: u16 = TRUE;
pub const WANG: u16 = FALSE;
pub const HIGH_MEMORY: u16 = FALSE;
pub const ESCAPE: u8 = 0;
pub const CANCEL: u8 = 27;
pub const PRINTER_TOGGLE: u16 = TRUE;
pub const ZERO_EXTEND: u16 = TRUE;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"MSSW IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" WG");
    put_hex16(WANG);
    crate::bios::mscon::con_write_bytes(b" HM");
    put_hex16(HIGH_MEMORY);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(ESCAPE);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(CANCEL);
    crate::bios::mscon::con_write_bytes(b" PRN");
    put_hex16(PRINTER_TOGGLE);
    crate::bios::mscon::con_write_bytes(b" EXT");
    put_hex16(ZERO_EXTEND);
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

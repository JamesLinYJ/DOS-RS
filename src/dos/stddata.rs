//! Standard data segment from `DOS/STDDATA.ASM`.
//!
//! The segment includes `STDSW.ASM`, where `IBM` and `ALTVECT` are off.
//! `debug` is off, so the big `DOSSYM` include is skipped. `installed` is on.

const TRUE: u16 = 0xffff;
const FALSE: u16 = 0;

pub const DEBUG: u16 = FALSE;
pub const INSTALLED: u16 = TRUE;
pub const IBM: u16 = FALSE;
pub const ALTERNATE_VECTOR: u16 = FALSE;
pub const RAINBOW: u16 = FALSE;
pub const HIGH_MEMORY: u16 = FALSE;
pub const ESCAPE: u8 = 0x1b;
pub const CANCEL: u8 = b'X' - b'@';

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"STDDATA DBG");
    put_hex8(DEBUG as u8);
    crate::bios::mscon::con_write_bytes(b" INS");
    put_hex16(INSTALLED);
    crate::bios::mscon::con_write_bytes(b" IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" AV");
    put_hex16(ALTERNATE_VECTOR);
    crate::bios::mscon::con_write_bytes(b" RB");
    put_hex16(RAINBOW);
    crate::bios::mscon::con_write_bytes(b" HM");
    put_hex16(HIGH_MEMORY);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(ESCAPE);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(CANCEL);
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

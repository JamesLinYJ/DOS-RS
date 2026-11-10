//! Build switches from `DOS/HIGHSW.ASM`.
//!
//! The Microsoft build is selected. IBM and Wang are off, so escape is `1Bh`
//! and the cancel character is Ctrl-X. DOS moves itself to the top of memory.

const TRUE: u16 = 0xffff;
const FALSE: u16 = !TRUE;

pub const MSVER: u16 = TRUE;
pub const IBM: u16 = FALSE;
pub const WANG: u16 = FALSE;
pub const ALTVECT: u16 = FALSE;
pub const HIGHMEM: u16 = TRUE;
pub const ESCCH: u8 = if IBM != 0 {
    0
} else if WANG != 0 {
    0x1f
} else {
    0x1b
};
pub const CANCEL: u8 = b'X' - b'@';
pub const TOGLPRN: u16 = FALSE;
pub const ZEROEXT: u16 = TRUE;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"HIGHSW MS");
    put_hex16(MSVER);
    crate::bios::mscon::con_write_bytes(b" IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" HI");
    put_hex16(HIGHMEM);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(ESCCH);
    crate::bios::mscon::con_write_bytes(b" CX");
    put_hex8(CANCEL);
    crate::bios::mscon::con_write_bytes(b" PRN");
    put_hex16(TOGLPRN);
    crate::bios::mscon::con_write_bytes(b" EXT");
    put_hex16(ZEROEXT);
    crate::bios::mscon::con_write_bytes(b"\r\n");
    let _ = ALTVECT;
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

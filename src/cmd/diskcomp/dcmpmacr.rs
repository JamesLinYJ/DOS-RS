//! Macros from `CMD/DISKCOMP/DCMPMACR.INC`.
//!
//! A DOS call with a subfunction packs both bytes. A call with only a
//! function replaces the high byte and leaves the low byte. A call with
//! no function leaves the word alone. The track table starts with a zero
//! count, then sector numbers 1 through 63, each 512 bytes long.

const SECTOR_BYTES: u16 = 512;
const SECTOR_LIMIT: u16 = 63;

fn dos_call(function: Option<u8>, subfunction: Option<u8>, previous: u16) -> u16 {
    match function {
        Some(function) => match subfunction {
            Some(subfunction) => (u16::from(function) << 8) | u16::from(subfunction),
            None => (u16::from(function) << 8) | (previous & 0x00FF),
        },
        None => previous,
    }
}

fn sector(number: u16) -> (u16, u16) {
    (number, SECTOR_BYTES)
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCMPMACR AX");
    put_hex16(dos_call(Some(0x44), Some(0x08), 0));
    crate::bios::mscon::con_write_bytes(b" AX");
    put_hex16(dos_call(Some(0x19), None, 0x000E));
    crate::bios::mscon::con_write_bytes(b" AX");
    put_hex16(dos_call(None, None, 0x0007));
    crate::bios::mscon::con_write_bytes(b" TK0000 N");
    put_hex16(SECTOR_LIMIT);
    let (number, size) = sector(1);
    crate::bios::mscon::con_write_bytes(b" F");
    put_hex16(number);
    put_hex16(size);
    let (number, size) = sector(18);
    crate::bios::mscon::con_write_bytes(b" M");
    put_hex16(number);
    put_hex16(size);
    let (number, size) = sector(SECTOR_LIMIT);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex16(number);
    put_hex16(size);
    crate::bios::mscon::con_write_bytes(b" PR");
    put_hex16(printed(5));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn printed(message: u16) -> u16 {
    message
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

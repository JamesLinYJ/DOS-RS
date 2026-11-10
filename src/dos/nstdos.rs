//! Non-resident DOS constants from `DOS/NSTDOS.ASM`.
//!
//! The standard switches are included, then `MSCONST.ASM`. The data version
//! is 1, share retry count is 3, and the OEM handler starts at `-1`.

const IBM: u16 = 0;
const DATA_VERSION: u16 = 1;
const RETRY_COUNT: u16 = 3;
const OEM_HANDLER: u16 = 0xffff;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"NSTDOS IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(DATA_VERSION);
    crate::bios::mscon::con_write_bytes(b" RT");
    put_hex16(RETRY_COUNT);
    crate::bios::mscon::con_write_bytes(b" OEM");
    put_hex16(OEM_HANDLER);
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

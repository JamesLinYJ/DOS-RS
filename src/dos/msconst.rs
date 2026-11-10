//! Initialized DOS data from `DOS/MSCONST.ASM`.
//!
//! The data version is 1. The debug marker is the four bytes `BUG `. Share
//! retries are 3 and the idle loop count is 1. The maximum sector is 128
//! bytes. The NUL device is a character device with the null attribute. The
//! default sharer table accepts slots 1, 2, 8, and 12.

const DATA_VERSION: u16 = 1;
const BUG: [u8; 4] = *b"BUG ";
const RETRY_LOOP: u16 = 1;
const MAX_SECTOR: u16 = 128;
const NUL_ATTRIBUTES: u16 = 0x8000 | 0x0004;
const HASH_COUNT: u16 = 1;
const EMS_MODE: u8 = 0xff;
const EMS_SEGMENTS: u16 = 1;
const SHARE_ACCEPTS: [bool; 15] = [
    false, true, true, false, false, false, false, false, true, false, false, false, true, false,
    false,
];

/// Default sharer slots, scanned from 0. Accepting slots set a bit.
pub fn share_mask() -> u16 {
    let mut mask = 0u16;
    let mut slot = 0u16;
    while (slot as usize) < SHARE_ACCEPTS.len() {
        if SHARE_ACCEPTS[slot as usize] {
            mask |= 1 << slot;
        }
        slot += 1;
    }
    mask
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"MSCONST DV");
    put_hex16(DATA_VERSION);
    crate::bios::mscon::con_write_bytes(b" BUG");
    put_hex8(BUG[0]);
    put_hex8(BUG[1]);
    put_hex8(BUG[2]);
    put_hex8(BUG[3]);
    crate::bios::mscon::con_write_bytes(b" RL");
    put_hex16(RETRY_LOOP);
    crate::bios::mscon::con_write_bytes(b" MX");
    put_hex16(MAX_SECTOR);
    crate::bios::mscon::con_write_bytes(b" NA");
    put_hex16(NUL_ATTRIBUTES);
    crate::bios::mscon::con_write_bytes(b" HS");
    put_hex16(HASH_COUNT);
    crate::bios::mscon::con_write_bytes(b" EMS");
    put_hex8(EMS_MODE);
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex16(EMS_SEGMENTS);
    crate::bios::mscon::con_write_bytes(b" J");
    put_hex16(share_mask());
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

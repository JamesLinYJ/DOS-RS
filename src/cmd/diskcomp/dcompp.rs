//! Parser switches from `CMD/DISKCOMP/DCOMPP.ASM`.
//!
//! Dates, times, file specifications, capitalization, complex lists,
//! quoted strings, numbers, and keywords are left out. Drive-only syntax
//! and switches are included, and the parser data is addressed by the data
//! segment. The documented results run from no error and end of line
//! through the syntax code, with no code five.

const SWITCHES: &[(u8, u8)] = &[
    (b'I', 0),
    (b'F', 0),
    (b'D', 0),
    (b'T', 0),
    (b'L', 0),
    (b'C', 0),
    (b'X', 0),
    (b'R', 1),
    (b'Q', 0),
    (b'N', 0),
    (b'K', 0),
    (b'S', 1),
    (b'1', 0),
    (b'2', 0),
    (b'3', 0),
    (b'B', 1),
];

const RESULTS: &[u16] = &[0, 0xFFFF, 1, 2, 3, 4, 6, 7, 8, 9];

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCOMPP");
    let mut index = 0usize;
    while index < SWITCHES.len() {
        let (tag, value) = SWITCHES[index];
        crate::bios::mscon::con_write_bytes(b" ");
        crate::bios::mscon::con_write_bytes(&[tag]);
        put_hex8(value);
        index += 1;
    }
    index = 0;
    while index < RESULTS.len() {
        crate::bios::mscon::con_write_bytes(b" R");
        put_hex16(RESULTS[index]);
        index += 1;
    }
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

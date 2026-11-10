//! Parser switches from `CMD/DISKCOPY/DCOPYP.ASM`.
//!
//! Dates, times, file specifications, complex lists, quoted strings,
//! numbers, and keywords are left out. Drive-only syntax, switches, and
//! file-table capitalization are included. The parser data is addressed
//! by the data segment.

const SWITCHES: &[(u8, u8)] = &[
    (b'F', 0),
    (b'D', 0),
    (b'T', 0),
    (b'L', 0),
    (b'C', 1),
    (b'X', 0),
    (b'R', 1),
    (b'Q', 0),
    (b'N', 0),
    (b'K', 0),
    (b'S', 1),
    (b'1', 0),
    (b'2', 0),
    (b'3', 0),
    (b'I', 0),
    (b'B', 1),
];

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCOPYP");
    let mut index = 0usize;
    while index < SWITCHES.len() {
        let (tag, value) = SWITCHES[index];
        crate::bios::mscon::con_write_bytes(b" ");
        crate::bios::mscon::con_write_bytes(&[tag]);
        put_hex8(value);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

//! Segment order from `CMD/COMMAND/COMSEG.ASM`.
//!
//! The resident group is loaded first. The transient group starts at `TAIL`.
//! Arenas, the environment, and the tails are paragraph aligned. Code and
//! data segments are byte aligned.

const RESIDENT: [(u8, bool); 8] = [
    (1, false),
    (2, false),
    (3, true),
    (4, true),
    (5, true),
    (6, true),
    (7, true),
    (8, true),
];

const TRANSIENT: [(u8, bool); 4] = [(9, false), (10, false), (11, false), (12, true)];

fn alignment(segments: &[(u8, bool)]) -> u8 {
    let mut bits = 0u8;
    let mut index = 0usize;
    while index < segments.len() && index < 8 {
        if segments[index].1 {
            bits |= 1 << index;
        }
        index += 1;
    }
    bits
}

fn order(segments: &[(u8, bool)]) -> u32 {
    let mut packed = 0u32;
    let mut index = 0usize;
    while index < segments.len() {
        packed = (packed << 4) | u32::from(segments[index].0);
        index += 1;
    }
    packed
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COMSEG R");
    put_hex8(RESIDENT.len() as u8);
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(TRANSIENT.len() as u8);
    crate::bios::mscon::con_write_bytes(b" RA");
    put_hex8(alignment(&RESIDENT));
    crate::bios::mscon::con_write_bytes(b" TA");
    put_hex8(alignment(&TRANSIENT));
    crate::bios::mscon::con_write_bytes(b" OR");
    put_hex32(order(&RESIDENT));
    crate::bios::mscon::con_write_bytes(b" OT");
    put_hex16(order(&TRANSIENT) as u16);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex32(value: u32) {
    put_hex16((value >> 16) as u16);
    put_hex16(value as u16);
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

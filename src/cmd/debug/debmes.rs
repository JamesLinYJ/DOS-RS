//! Message tables from `CMD/DEBUG/DEBMES.ASM`.
//!
//! The system-version messages are left out. Each table is a word number, a
//! class byte, a word handle, a word substitution list, a word count, an
//! input byte, and a word buffer. Display reads them in that order and does
//! not call the fatal message. A failed load is displayed and the carry is
//! kept. The drive letter starts as A. The second write field is left
//! aligned, and the unallocated pads are blanks.

struct Note {
    number: u16,
    class: u8,
    handle: u8,
    count: u8,
}

struct Piece {
    param: u8,
    flags: u8,
    width: u8,
    floor: u8,
    pad: u8,
}

const NOTES: &[Note] = &[
    Note { number: 8, class: 255, handle: 1, count: 0 },
    Note { number: 44, class: 255, handle: 1, count: 0 },
    Note { number: 1, class: 255, handle: 1, count: 0 },
    Note { number: 9, class: 255, handle: 1, count: 0 },
    Note { number: 10, class: 255, handle: 1, count: 0 },
    Note { number: 2, class: 1, handle: 2, count: 0 },
    Note { number: 12, class: 255, handle: 1, count: 0 },
    Note { number: 13, class: 255, handle: 1, count: 0 },
    Note { number: 14, class: 255, handle: 1, count: 1 },
    Note { number: 15, class: 255, handle: 1, count: 1 },
    Note { number: 16, class: 255, handle: 1, count: 1 },
    Note { number: 17, class: 255, handle: 1, count: 1 },
    Note { number: 8, class: 1, handle: 2, count: 0 },
    Note { number: 19, class: 255, handle: 1, count: 1 },
    Note { number: 20, class: 255, handle: 1, count: 0 },
    Note { number: 20, class: 255, handle: 1, count: 0 },
    Note { number: 21, class: 255, handle: 1, count: 0 },
    Note { number: 21, class: 255, handle: 1, count: 0 },
    Note { number: 22, class: 255, handle: 1, count: 0 },
    Note { number: 23, class: 255, handle: 1, count: 0 },
    Note { number: 24, class: 255, handle: 1, count: 0 },
    Note { number: 25, class: 255, handle: 1, count: 0 },
    Note { number: 26, class: 255, handle: 1, count: 0 },
    Note { number: 27, class: 255, handle: 1, count: 1 },
    Note { number: 32, class: 255, handle: 1, count: 2 },
    Note { number: 33, class: 255, handle: 1, count: 3 },
    Note { number: 34, class: 255, handle: 1, count: 2 },
    Note { number: 35, class: 255, handle: 1, count: 2 },
    Note { number: 36, class: 255, handle: 1, count: 2 },
    Note { number: 37, class: 255, handle: 1, count: 1 },
    Note { number: 38, class: 255, handle: 1, count: 1 },
    Note { number: 39, class: 255, handle: 1, count: 2 },
    Note { number: 40, class: 255, handle: 1, count: 1 },
    Note { number: 41, class: 255, handle: 1, count: 1 },
    Note { number: 42, class: 255, handle: 1, count: 6 },
    Note { number: 46, class: 255, handle: 1, count: 1 },
    Note { number: 47, class: 255, handle: 1, count: 1 },
    Note { number: 50, class: 255, handle: 1, count: 2 },
    Note { number: 51, class: 255, handle: 1, count: 2 },
    Note { number: 55, class: 255, handle: 1, count: 1 },
    Note { number: 56, class: 255, handle: 1, count: 2 },
    Note { number: 57, class: 255, handle: 1, count: 0 },
    Note { number: 58, class: 255, handle: 1, count: 0 },
    Note { number: 59, class: 255, handle: 1, count: 0 },
    Note { number: 60, class: 255, handle: 1, count: 0 },
    Note { number: 61, class: 255, handle: 1, count: 0 },
    Note { number: 62, class: 255, handle: 1, count: 0 },
    Note { number: 63, class: 255, handle: 1, count: 0 },
    Note { number: 64, class: 255, handle: 1, count: 0 },
    Note { number: 65, class: 255, handle: 1, count: 0 },
    Note { number: 66, class: 255, handle: 1, count: 0 },
    Note { number: 67, class: 255, handle: 1, count: 0 },
    Note { number: 68, class: 255, handle: 1, count: 0 },
    Note { number: 70, class: 255, handle: 1, count: 0 },
    Note { number: 71, class: 255, handle: 1, count: 0 },
    Note { number: 72, class: 255, handle: 1, count: 2 },
    Note { number: 75, class: 255, handle: 1, count: 2 },
    Note { number: 76, class: 255, handle: 1, count: 1 },
    Note { number: 78, class: 255, handle: 1, count: 0 },
];

const PIECES: &[Piece] = &[
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 16, width: 1, floor: 1, pad: 32 },
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 2, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 3, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 16, width: 2, floor: 2, pad: 32 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 2, flags: 35, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 144, width: 0, floor: 0, pad: 32 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 147, width: 2, floor: 2, pad: 48 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 3, flags: 147, width: 2, floor: 2, pad: 48 },
    Piece { param: 4, flags: 147, width: 2, floor: 2, pad: 48 },
    Piece { param: 5, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 6, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 2, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 3, flags: 0, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 16, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 0, width: 0, floor: 0, pad: 32 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 147, width: 2, floor: 2, pad: 48 },
    Piece { param: 2, flags: 147, width: 2, floor: 2, pad: 48 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 147, width: 2, floor: 2, pad: 48 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 147, width: 4, floor: 4, pad: 48 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 32 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 32 },
    Piece { param: 1, flags: 163, width: 4, floor: 4, pad: 32 },
    Piece { param: 2, flags: 163, width: 4, floor: 4, pad: 32 },
];

const FATAL: u16 = 45;
const ARGUMENT_BYTES: u16 = 80;
const OPCODE_BYTES: u16 = 0x51;
const ERROR_KIND_BYTES: u8 = 3;
const NAMES: &[u8] = b"dfbfbrbp";

fn store(note: &Note) -> [u8; 12] {
    let mut raw = [0u8; 12];
    raw[0] = note.number as u8;
    raw[1] = (note.number >> 8) as u8;
    raw[2] = note.class;
    raw[3] = note.handle;
    raw[7] = note.count;
    raw
}

fn load(raw: &[u8; 12]) -> (u16, u8, u16, u16, u8, u16) {
    let number = u16::from_le_bytes([raw[0], raw[1]]);
    let class = raw[2];
    let handle = u16::from_le_bytes([raw[3], raw[4]]);
    let count = u16::from_le_bytes([raw[7], raw[8]]);
    let input = raw[9];
    let buffer = u16::from_le_bytes([raw[10], raw[11]]);
    (number, class, handle, count, input, buffer)
}

fn note_digest(notes: &[Note]) -> (u32, u16) {
    let mut sum = 0u32;
    let mut mixed = 0u16;
    let mut index = 0usize;
    while index < notes.len() {
        let note = &notes[index];
        sum = sum.wrapping_add(u32::from(note.number));
        sum = sum.wrapping_add(u32::from(note.class));
        sum = sum.wrapping_add(u32::from(note.handle));
        sum = sum.wrapping_add(u32::from(note.count));
        mixed ^= note.number;
        index += 1;
    }
    (sum, mixed)
}

fn piece_digest(pieces: &[Piece]) -> (u16, u16) {
    let mut sum = 0u16;
    let mut mixed = 0u16;
    let mut index = 0usize;
    while index < pieces.len() {
        let piece = &pieces[index];
        sum = sum.wrapping_add(u16::from(piece.param));
        sum = sum.wrapping_add(u16::from(piece.width));
        sum = sum.wrapping_add(u16::from(piece.floor));
        mixed ^= (u16::from(piece.flags) << 8) | u16::from(piece.pad);
        index += 1;
    }
    (sum, mixed)
}

fn missing(number: u16) -> bool {
    let mut index = 0usize;
    while index < NOTES.len() {
        if NOTES[index].number == number {
            return false;
        }
        index += 1;
    }
    true
}

fn by_count(count: u8) -> u16 {
    let mut index = 0usize;
    while index < NOTES.len() {
        if NOTES[index].count == count {
            return NOTES[index].number;
        }
        index += 1;
    }
    0
}

fn printf(display_failed: bool) -> bool {
    display_failed
}

fn preload(load_failed: bool) -> (bool, bool) {
    if load_failed { (true, true) } else { (false, false) }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBMES SV00 FT");
    put_hex16(FATAL);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex16(NOTES.len() as u16);
    let (sum, mixed) = note_digest(NOTES);
    crate::bios::mscon::con_write_bytes(b" SM");
    put_hex32(sum);
    crate::bios::mscon::con_write_bytes(b" XR");
    put_hex16(mixed);
    crate::bios::mscon::con_write_bytes(b" SB");
    put_hex16(PIECES.len() as u16);
    let (piece_sum, piece_mix) = piece_digest(PIECES);
    crate::bios::mscon::con_write_bytes(b" SS");
    put_hex16(piece_sum);
    crate::bios::mscon::con_write_bytes(b" SX");
    put_hex16(piece_mix);
    if missing(6) {
        crate::bios::mscon::con_write_bytes(b" A6");
    }
    if missing(7) {
        crate::bios::mscon::con_write_bytes(b" A7");
    }
    let (number, class, handle, count, input, buffer) = load(&store(&NOTES[0]));
    crate::bios::mscon::con_write_bytes(b" F");
    put_hex16(number);
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(class);
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex16(handle);
    crate::bios::mscon::con_write_bytes(b" K");
    put_hex16(count);
    crate::bios::mscon::con_write_bytes(b" I");
    put_hex8(input);
    crate::bios::mscon::con_write_bytes(b" B");
    put_hex16(buffer);
    let found = load(&store(&NOTES[5]));
    crate::bios::mscon::con_write_bytes(b" NF");
    put_hex16(found.0);
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(found.1);
    crate::bios::mscon::con_write_bytes(b" H");
    put_hex16(found.2);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_hex16(by_count(6));
    crate::bios::mscon::con_write_bytes(b" K06 DR");
    put_hex16(by_count(1));
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(PIECES[2].width);
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex8(PIECES[2].pad);
    crate::bios::mscon::con_write_bytes(b" UA");
    put_hex8(PIECES[40].pad);
    crate::bios::mscon::con_write_bytes(b" DA");
    put_hex8(PIECES[39].flags);
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(PIECES[39].width);
    crate::bios::mscon::con_write_bytes(b" DK");
    put_hex8(PIECES[29].flags);
    crate::bios::mscon::con_write_bytes(b" WR");
    put_hex8(PIECES[16].flags);
    crate::bios::mscon::con_write_bytes(b" DL41 EN");
    let mut index = 0usize;
    while index < NAMES.len() {
        put_hex8(NAMES[index]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" AB");
    put_hex16(ARGUMENT_BYTES);
    crate::bios::mscon::con_write_bytes(b" OB");
    put_hex16(OPCODE_BYTES);
    crate::bios::mscon::con_write_bytes(b" EZ");
    put_hex8(ERROR_KIND_BYTES);
    crate::bios::mscon::con_write_bytes(if printf(false) { b" PF01" } else { b" PF00" });
    crate::bios::mscon::con_write_bytes(if printf(true) { b" PF01" } else { b" PF00" });
    let (carry, shown) = preload(false);
    crate::bios::mscon::con_write_bytes(if carry { b" LD01" } else { b" LD00" });
    crate::bios::mscon::con_write_bytes(if shown { b" DY01" } else { b" ND00" });
    let (carry, shown) = preload(true);
    crate::bios::mscon::con_write_bytes(if carry { b" LD01" } else { b" LD00" });
    crate::bios::mscon::con_write_bytes(if shown { b" DY01" } else { b" ND00" });
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

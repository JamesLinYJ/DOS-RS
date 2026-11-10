//! Equates from `CMD/DEBUG/DEBEQU.ASM`.
//!
//! The executable file control block is the same word as the file control
//! block. The breakpoint table is five bytes times ten breakpoints. The
//! two-letter names are built so the first letter is the low byte. The
//! build is the IBM set, so the interrupt mask port is 21h and the mask is
//! every bit. Japan mode and the system-version switch are off, and DEBUG
//! still installs the Control-C vector. Both sector commands are zero.

const FILE_BLOCK: u16 = 0x005C;
const EXE_BLOCK: u16 = FILE_BLOCK;
const BREAK_MAX: u16 = 10;
const BREAK_BYTES: u16 = 5 * BREAK_MAX;
const NAME_B_X: u16 = b'B' as u16 + 0x5800;
const NAME_B_P: u16 = b'B' as u16 + 0x5000;
const NAME_S_I: u16 = b'S' as u16 + 0x4900;
const NAME_D_I: u16 = b'D' as u16 + 0x4900;
const WORD_COMMA: u16 = 0x2C00;
const INTERRUPT_MASK: u16 = 0b1111_1111;

const FIELDS: &[(&[u8], u16)] = &[
    (b"PR", b'-' as u16),
    (b"FC", FILE_BLOCK),
    (b"EX", EXE_BLOCK),
    (b"BL", 80),
    (b"BM", BREAK_MAX),
    (b"BT", BREAK_BYTES),
    (b"RT", 14),
    (b"SD", 0),
    (b"BZ", 512),
    (b"NB", NAME_B_X),
    (b"NP", NAME_B_P),
    (b"NS", NAME_S_I),
    (b"ND", NAME_D_I),
    (b"CW", WORD_COMMA),
    (b"OL", 35),
    (b"IJ", 0),
    (b"CC", 0xFFFF),
    (b"SV", 0),
    (b"MP", 0x21),
    (b"IM", INTERRUPT_MASK),
    (b"CR", 13),
    (b"LF", 10),
    (b"TB", 9),
    (b"BS", 8),
    (b"EF", 0x1A),
    (b"RB", 0x7F),
    (b"EQ", b'=' as u16),
    (b"MN", b'-' as u16),
    (b"SP", b' ' as u16),
    (b"DQ", b'"' as u16),
    (b"SQ", b'\'' as u16),
    (b"CM", b',' as u16),
    (b"PD", b'.' as u16),
    (b"CN", b':' as u16),
    (b"SC", b';' as u16),
    (b"LB", b'[' as u16),
    (b"AT", b'@' as u16),
    (b"ZR", b'0' as u16),
    (b"LA", b'a' as u16),
    (b"LZ", b'z' as u16),
    (b"UA", b'A' as u16),
    (b"UC", b'C' as u16),
    (b"UE", b'E' as u16),
    (b"UF", b'F' as u16),
    (b"UL", b'L' as u16),
    (b"UM", b'M' as u16),
    (b"UN", b'N' as u16),
    (b"UP", b'P' as u16),
    (b"US", b'S' as u16),
    (b"UX", b'X' as u16),
    (b"UZ", b'Z' as u16),
    (b"VS", 1),
    (b"VB", 3),
    (b"VT", 0x22),
    (b"VK", 0x23),
    (b"VE", 0x24),
    (b"VR", 0x25),
    (b"VW", 0x26),
    (b"IO", 0x440D),
    (b"RW", 0x08),
    (b"RS", 0),
    (b"WS", 0),
    (b"DR", 1),
    (b"LS", 0),
    (b"LE", 2),
    (b"G4", 0x40),
    (b"G2", 0x42),
    (b"G3", 0x43),
    (b"MM", 0x44),
    (b"G5", 0x45),
    (b"G6", 0x46),
    (b"G7", 0x47),
    (b"G8", 0x48),
    (b"GB", 0x4B),
    (b"GD", 0x4D),
    (b"GE", 0x4E),
    (b"GM", 0x00),
    (b"FR", 0x5800),
    (b"GS", 0x01),
    (b"HL", 0x00FF),
    (b"LM", 0x40),
    (b"E0", 0x80),
    (b"E3", 0x83),
    (b"E4", 0x84),
    (b"E5", 0x85),
    (b"E6", 0x86),
    (b"E7", 0x87),
    (b"E8", 0x88),
    (b"E9", 0x89),
    (b"EA", 0x8A),
    (b"EB", 0x8B),
    (b"ED", 0x8D),
    (b"EE", 0x8E),
    (b"EF", 0x8F),
    (b"NI", 0xFF),
];

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBEQU");
    let mut index = 0usize;
    while index < FIELDS.len() {
        let (tag, value) = FIELDS[index];
        crate::bios::mscon::con_write_bytes(b" ");
        crate::bios::mscon::con_write_bytes(tag);
        put_hex16(value);
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

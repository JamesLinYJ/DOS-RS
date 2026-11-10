//! Message descriptors from `CMD/DISKCOPY/DCOPYMS.INC`.
//!
//! Each descriptor is a number, the standard output handle, a substitution
//! count, and a class word. The class defaults to FF00h. There is no message
//! 24. Press-a-key clears the buffer before a silent read, and the copy-again
//! prompt echoes the key. A serial field is right aligned, four digits, padded
//! with zero. Drive letters start one below 'A'.

const NOTES: &[(u16, u16, u8)] = &[
    (2, 0, 0),
    (3, 0, 0),
    (4, 0, 0),
    (5, 0, 0),
    (6, 0, 0),
    (7, 0, 0),
    (8, 1, 0),
    (9, 1, 0),
    (10, 0, 0),
    (11, 0, 0),
    (12, 0, 0),
    (13, 1, 0),
    (14, 0, 0),
    (15, 0, 0xC0 + 8),
    (16, 0, 0xC0 + 1),
    (17, 3, 0),
    (18, 0, 0),
    (19, 3, 0),
    (20, 3, 0),
    (21, 0, 0),
    (22, 0, 0),
    (23, 0, 0),
    (25, 0, 0),
    (26, 2, 0),
];

fn class_word(input: u8) -> u16 {
    0xFF00 | u16::from(input)
}

fn flags(align: u8, size: u8, kind: u8) -> u8 {
    ((align & 1) << 7) | ((size & 0x03) << 4) | (kind & 0x0F)
}

fn missing(number: u16) -> bool {
    let mut index = 0usize;
    while index < NOTES.len() {
        if NOTES[index].0 == number {
            return false;
        }
        index += 1;
    }
    true
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCOPYMS");
    let mut index = 0usize;
    while index < NOTES.len() {
        let (number, count, input) = NOTES[index];
        crate::bios::mscon::con_write_bytes(b" M");
        put_hex16(number);
        put_hex16(class_word(input));
        put_hex8(count as u8);
        index += 1;
    }
    if missing(24) {
        crate::bios::mscon::con_write_bytes(b" A18");
    }
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex8(flags(0, 1, 0));
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(flags(0, 0, 0));
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(flags(0, 2, 1));
    crate::bios::mscon::con_write_bytes(b" R");
    put_hex8(flags(1, 2, 3));
    crate::bios::mscon::con_write_bytes(b" Z");
    put_hex8(4);
    put_hex8(b'0');
    crate::bios::mscon::con_write_bytes(b" DL");
    put_hex8(b'A');
    put_hex8(b':');
    put_hex8(0);
    crate::bios::mscon::con_write_bytes(b" ID");
    put_hex8(b'A' - 1);
    crate::bios::mscon::con_write_bytes(b" FS");
    put_hex8(4);
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

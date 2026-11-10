//! Message descriptors from `CMD/DISKCOMP/DCOMPMS.INC`.
//!
//! Each descriptor is a number, the standard output handle, a substitution
//! count, and a class word. The class defaults to FF00h. Strike-a-key adds
//! a cleared buffer and silent input. The yes-or-no prompt adds a cleared
//! buffer and echoed input. Substitution flags put the size in bits 5 and 4
//! and the type in the low nibble. Drive letters start one below 'A'.

const NOTES: &[(u16, u16, u8)] = &[
    (2, 0, 0),
    (3, 0, 0),
    (4, 0, 0),
    (5, 0, 0),
    (6, 0, 0),
    (7, 1, 0),
    (8, 1, 0),
    (9, 0, 0),
    (10, 0, 0),
    (11, 1, 0),
    (12, 0, 0xC0 + 8),
    (13, 0, 0),
    (14, 0, 0xC0 + 1),
    (15, 3, 0),
    (16, 0, 0),
    (17, 3, 0),
    (18, 2, 0),
    (19, 0, 0),
    (20, 0, 0),
    (21, 0, 0),
    (22, 0, 0),
];

fn class_word(input: u8) -> u16 {
    0xFF00 | u16::from(input)
}

fn flags(size: u8, kind: u8) -> u8 {
    ((size & 0x03) << 4) | (kind & 0x0F)
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DCOMPMS");
    let mut index = 0usize;
    while index < NOTES.len() {
        let (number, count, input) = NOTES[index];
        crate::bios::mscon::con_write_bytes(b" M");
        put_hex16(number);
        put_hex16(class_word(input));
        put_hex8(count as u8);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex8(flags(1, 0));
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(flags(0, 0));
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(flags(2, 1));
    crate::bios::mscon::con_write_bytes(b" DL");
    put_hex8(b'A');
    put_hex8(b':');
    put_hex8(0);
    crate::bios::mscon::con_write_bytes(b" ID");
    put_hex8(b'A' - 1);
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(0);
    crate::bios::mscon::con_write_bytes(b" TR");
    put_hex16(0);
    crate::bios::mscon::con_write_bytes(b" FS");
    put_hex8(4);
    crate::bios::mscon::con_write_bytes(b" I");
    put_hex8(5);
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

//! Message blocks from `CMD/COMP/COMPMS.INC`.
//!
//! Utility messages use class `FF`. The name prompts ask for a buffered line.
//! "Compare more" clears the keyboard and then reads one character. A compare
//! offset is an unsigned hex dword, and the two file bytes are unsigned hex
//! bytes. The names in "%1 and %2" are two ASCIIZ fields.

const STDOUT: u16 = 1;
const STDERR: u16 = 2;
const UTILITY: u16 = 0xFF00;
const PROMPT: u16 = 0xFF0A;
const ANSWER: u16 = 0xFFC1;
const PARSE: u16 = 0x0200;
const SPACE: u8 = b' ';

#[derive(Clone, Copy)]
struct Field {
    which: u8,
    kind: u8,
}

struct Note {
    number: u16,
    handle: u16,
    count: u16,
    class: u16,
    fields: [Field; 2],
    used: u8,
    input: bool,
}

fn kind(size: u8, field: u8) -> u8 {
    (size << 4) | field
}

const NONE: Field = Field { which: 0, kind: 0 };

fn text(which: u8) -> Field {
    Field { which, kind: kind(0b01, 0b0000) }
}

fn hex_byte() -> Field {
    Field { which: 1, kind: kind(0b01, 0b0011) }
}

fn hex_dword() -> Field {
    Field { which: 1, kind: kind(0b11, 0b0011) }
}

fn plain(number: u16) -> Note {
    Note { number, handle: STDOUT, count: 0, class: UTILITY, fields: [NONE, NONE], used: 0, input: false }
}

fn one(number: u16, field: Field) -> Note {
    Note { number, handle: STDOUT, count: 1, class: UTILITY, fields: [field, NONE], used: 1, input: false }
}

fn notes() -> [Note; 23] {
    [
        plain(2),
        plain(3),
        plain(4),
        plain(5),
        one(6, hex_dword()),
        one(7, hex_byte()),
        one(8, hex_byte()),
        plain(9),
        plain(10),
        one(11, text(0)),
        one(12, text(0)),
        one(13, text(0)),
        plain(14),
        Note { number: 15, handle: STDOUT, count: 0, class: PROMPT, fields: [NONE, NONE], used: 0, input: true },
        Note { number: 16, handle: STDOUT, count: 0, class: PROMPT, fields: [NONE, NONE], used: 0, input: true },
        plain(17),
        Note { number: 18, handle: STDOUT, count: 0, class: ANSWER, fields: [NONE, NONE], used: 0, input: false },
        Note { number: 19, handle: STDOUT, count: 2, class: UTILITY, fields: [text(1), text(2)], used: 2, input: false },
        plain(20),
        one(21, text(0)),
        plain(22),
        plain(23),
        Note { number: 0, handle: STDERR, count: 1, class: PARSE, fields: [text(0), NONE], used: 1, input: false },
    ]
}

fn write_field(field: &Field) {
    crate::bios::mscon::con_write_bytes(b" F");
    put_hex8(field.which);
    put_hex8(field.kind);
    put_hex8(0);
    put_hex8(1);
    put_hex8(SPACE);
}

fn write_note(note: &Note) {
    crate::bios::mscon::con_write_bytes(b" M");
    put_hex16(note.number);
    put_hex16(note.handle);
    put_hex16(note.count);
    put_hex16(note.class);
    let mut index = 0usize;
    while index < note.used as usize {
        write_field(&note.fields[index]);
        index += 1;
    }
    if note.input {
        crate::bios::mscon::con_write_bytes(b" X");
    }
}

pub fn show() {
    let extended = text(0);
    let table = notes();
    crate::bios::mscon::con_write_bytes(b"COMPMS L");
    put_hex8(extended.which);
    put_hex8(extended.kind);
    put_hex8(0);
    put_hex8(1);
    put_hex8(SPACE);
    let mut index = 0usize;
    while index < table.len() {
        write_note(&table[index]);
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

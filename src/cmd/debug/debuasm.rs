//! Unassembler entry from `CMD/DEBUG/DEBUASM.ASM`.
//!
//! The default length is the dump size shifted right twice. The line loop
//! runs once even when the count is already zero, and a zero count is not
//! decremented. The opcode is multiplied by four to reach its mnemonic.
//! The operation buffer is 50 zeros, then 35 spaces. Column padding is 14
//! minus twice the bytes already shown, with the sign kept. Segment
//! prefixes fall through: DS adds four, SS three, CS two, and ES one.

const DUMP_BYTES: u16 = 128;
const ZERO_FILL: usize = 50;
const SPACE_FILL: usize = 35;
const COLUMNS: u8 = 14;

fn default_length(dump_bytes: u16) -> u16 {
    dump_bytes >> 2
}

fn fetch(left: u16, seen: u8) -> (u16, u8) {
    let left = if left == 0 { 0 } else { left - 1 };
    (left, seen.wrapping_add(1))
}

fn lines(mut left: u16) -> (u16, u16) {
    let mut ran = 0u16;
    loop {
        let (next, _) = fetch(left, 0);
        left = next;
        ran += 1;
        if left == 0 {
            break;
        }
    }
    (ran, left)
}

fn word_bit(opcode: u8) -> u8 {
    opcode & 1
}

fn table_index(opcode: u8) -> u16 {
    u16::from(opcode) << 2
}

fn line_buffer() -> [u8; ZERO_FILL] {
    let mut buffer = [0u8; ZERO_FILL];
    let mut index = 0usize;
    while index < SPACE_FILL {
        buffer[index] = b' ';
        index += 1;
    }
    buffer
}

fn pad_count(seen: u8) -> u16 {
    let used = seen.wrapping_mul(2);
    let gap = COLUMNS.wrapping_sub(used);
    (gap as i8) as u16
}

fn prefix_mark(kind: u8) -> u8 {
    let mut high = 0u8;
    if kind == b'D' {
        high = high.wrapping_add(1);
    }
    if kind == b'D' || kind == b'S' {
        high = high.wrapping_add(1);
    }
    if kind == b'D' || kind == b'S' || kind == b'C' {
        high = high.wrapping_add(1);
    }
    if kind == b'D' || kind == b'S' || kind == b'C' || kind == b'E' {
        high = high.wrapping_add(1);
    }
    high
}

fn copy_name(name: &[u8]) -> usize {
    let mut copied = 0usize;
    while copied < name.len() && name[copied] != 0 {
        copied += 1;
    }
    copied
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBUASM LN");
    put_hex16(default_length(DUMP_BYTES));
    let (ran, left) = lines(4);
    crate::bios::mscon::con_write_bytes(b" R");
    put_hex16(ran);
    crate::bios::mscon::con_write_bytes(b" Z");
    put_hex16(left);
    let (ran, left) = lines(0);
    crate::bios::mscon::con_write_bytes(b" R");
    put_hex16(ran);
    crate::bios::mscon::con_write_bytes(b" Z");
    put_hex16(left);
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(table_index(0x90));
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(word_bit(0x90));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(table_index(0x01));
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(word_bit(0x01));
    let buffer = line_buffer();
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(buffer[0]);
    crate::bios::mscon::con_write_bytes(b" E");
    put_hex8(buffer[SPACE_FILL - 1]);
    crate::bios::mscon::con_write_bytes(b" Z");
    put_hex8(buffer[SPACE_FILL]);
    crate::bios::mscon::con_write_bytes(b" F");
    put_hex16(ZERO_FILL as u16);
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex16(SPACE_FILL as u16);
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(pad_count(0));
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(pad_count(1));
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(pad_count(7));
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(pad_count(8));
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex8(prefix_mark(b'D'));
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex8(prefix_mark(b'S'));
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(prefix_mark(b'C'));
    crate::bios::mscon::con_write_bytes(b" E");
    put_hex8(prefix_mark(b'E'));
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(copy_name(b"NOP\0") as u8);
    crate::bios::mscon::con_write_bytes(b" T09 N");
    put_hex8(copy_name(b"\0") as u8);
    crate::bios::mscon::con_write_bytes(b" T09\r\n");
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

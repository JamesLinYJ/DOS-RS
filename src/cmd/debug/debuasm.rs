//! Unassembler from `CMD/DEBUG/DEBUASM.ASM`.
//!
//! The default length is the dump size shifted right twice. The line loop
//! runs once even when the count is already zero, and a zero count is not
//! decremented. The opcode is multiplied by four to reach its mnemonic.
//! The operation buffer is 50 zeros, then 35 spaces. Column padding is 14
//! minus twice the bytes already shown, with the sign kept. Segment
//! prefixes fall through: DS adds four, SS three, CS two, and ES one.
//!
//! Memory forms are chosen in the original order: a direct address only
//! when the mode is zero and the field is six, then BX, BP, and an optional
//! index. A mode of three names a register. Far calls are the odd group
//! entries from two through five. Signed bytes print a sign and a magnitude.

const DUMP_BYTES: u16 = 128;
const ZERO_FILL: usize = 50;
const SPACE_FILL: usize = 35;
const COLUMNS: u8 = 14;
const SAVED_B: u16 = 0x1111;
const SAVED_P: u16 = 0x2222;
const SAVED_S: u16 = 0x3333;
const SAVED_D: u16 = 0x4444;
const FLOAT_OPS: &[u8] = b"ADD$MUL$COM$COMP$SUB$SUBR$DIV$DIVR$";
const FLOAT_MARKS: &[u8] = b"CHS$ABS$@$@$TST$";
const WIDTHS: &[u8] = b"DWORD PTR $DWORD PTR $QWORD PTR $WORD PTR $BYTE PTR $TBYTE PTR $";

struct Text {
    bytes: [u8; 24],
    length: usize,
}

struct Memory {
    text: Text,
    segment: u8,
    place: u16,
}

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

fn daa(mut value: u8, mut carry: bool) -> (u8, bool) {
    if (value & 0x0f) > 9 {
        let sum = u16::from(value) + 6;
        value = sum as u8;
        if sum > 0xFF {
            carry = true;
        }
    }
    if value > 0x9f || carry {
        value = value.wrapping_add(0x60);
        carry = true;
    } else {
        carry = false;
    }
    (value, carry)
}

fn hex_digit(nibble: u8) -> u8 {
    let summed = nibble.wrapping_add(0x90);
    let (adjusted, carry) = daa(summed, false);
    let sum = u16::from(adjusted) + 0x40 + u16::from(carry);
    let (adjusted, _) = daa(sum as u8, sum > 0xFF);
    adjusted
}

fn hex_byte(value: u8) -> [u8; 2] {
    [hex_digit(value >> 4), hex_digit(value & 0x0f)]
}

fn pair(names: &[u8], index: u8) -> [u8; 2] {
    let start = usize::from(index & 7) * 2;
    [names[start], names[start + 1]]
}

fn push_pair(text: &mut Text, bytes: [u8; 2]) {
    text.push(bytes[0]);
    text.push(bytes[1]);
}

fn memory(mode: u8, field: u8, wide: bool, displacement: u16) -> Memory {
    let mut text = Text::new();
    if mode == 3 {
        let names: &[u8] = if wide { b"AXCXDXBXSPBPSIDI" } else { b"ALCLDLBLAHCHDHBH" };
        push_pair(&mut text, pair(names, field));
        return Memory { text, segment: 0, place: 0 };
    }
    let mut segment = 3u8;
    let mut place = 0u16;
    text.push(b'[');
    if field == 6 && mode == 0 {
        push_pair(&mut text, hex_byte((displacement >> 8) as u8));
        push_pair(&mut text, hex_byte(displacement as u8));
        text.push(b']');
        return Memory { text, segment, place: displacement };
    }
    if field <= 1 || field == 7 {
        push_pair(&mut text, *b"BX");
        place = SAVED_B;
    } else if field <= 3 || field == 6 {
        push_pair(&mut text, *b"BP");
        place = SAVED_P;
        segment = 2;
    }
    if field < 4 {
        text.push(b'+');
    }
    if field < 6 {
        if field & 1 == 0 {
            push_pair(&mut text, *b"SI");
            place = place.wrapping_add(SAVED_S);
        } else {
            push_pair(&mut text, *b"DI");
            place = place.wrapping_add(SAVED_D);
        }
    }
    if mode == 0 {
        text.push(b']');
    } else if mode == 2 {
        text.push(b'+');
        push_pair(&mut text, hex_byte((displacement >> 8) as u8));
        push_pair(&mut text, hex_byte(displacement as u8));
        place = place.wrapping_add(displacement);
        text.push(b']');
    } else {
        let byte = displacement as u8;
        let negative = (byte as i8) < 0;
        text.push(if negative { b'-' } else { b'+' });
        let magnitude = if negative { byte.wrapping_neg() } else { byte };
        push_pair(&mut text, hex_byte(magnitude));
        place = place.wrapping_add(byte as i8 as u16);
        text.push(b']');
    }
    Memory { text, segment, place }
}

fn far_group(field: u8) -> bool {
    (2..6).contains(&field) && field & 1 == 1
}

fn reversed_pair(first: u8, second: u8) -> [u8; 2] {
    [second, first]
}

fn copy_element(table: &[u8], which: u8, text: &mut Text) -> bool {
    let mut cursor = 0usize;
    let mut left = which;
    while left > 0 {
        while cursor < table.len() && table[cursor] != b'$' {
            cursor += 1;
        }
        if cursor < table.len() {
            cursor += 1;
        }
        left -= 1;
    }
    while cursor < table.len() && table[cursor] != b'$' {
        if table[cursor] == b'@' {
            return false;
        }
        text.push(table[cursor]);
        cursor += 1;
    }
    true
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
    crate::bios::mscon::con_write_bytes(b" T09");
    write_operands();
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_operands() {
    let sample = hex_byte(0x1A);
    crate::bios::mscon::con_write_bytes(b" HX");
    put_hex8(sample[0]);
    put_hex8(sample[1]);
    write_signed(0x05);
    write_signed(0xFF);
    write_signed(0x80);
    write_signed(0x00);
    write_memory(3, 0, true, 0);
    write_memory(3, 1, false, 0);
    write_memory(0, 0, true, 0);
    write_memory(0, 7, true, 0);
    write_memory(0, 2, true, 0);
    write_memory(0, 4, true, 0);
    write_memory(1, 0, true, 0x00FE);
    write_memory(2, 4, true, 0x00AB);
    write_memory(0, 6, true, 0x1234);
    crate::bios::mscon::con_write_bytes(if far_group(3) { b" FR" } else { b" NR" });
    crate::bios::mscon::con_write_bytes(if far_group(2) { b" FR" } else { b" NR" });
    crate::bios::mscon::con_write_bytes(if far_group(0) { b" FR" } else { b" NR" });
    let far = reversed_pair(b'A', b'F');
    let tail = reversed_pair(b' ', b'R');
    crate::bios::mscon::con_write_bytes(b" FW");
    put_hex8(far[0]);
    put_hex8(far[1]);
    put_hex8(tail[0]);
    put_hex8(tail[1]);
    let shift = reversed_pair(b'1', b',');
    crate::bios::mscon::con_write_bytes(b" S1");
    put_hex8(shift[0]);
    put_hex8(shift[1]);
    crate::bios::mscon::con_write_bytes(b" CL2C");
    put_hex8(b'C');
    put_hex8(0x4C);
    write_element(WIDTHS, 3);
    write_element(WIDTHS, 4);
    write_element(FLOAT_OPS, 0);
    write_element(FLOAT_OPS, 7);
    write_element(FLOAT_MARKS, 2);
}

fn write_signed(byte: u8) {
    let negative = (byte as i8) < 0;
    let magnitude = if negative { byte.wrapping_neg() } else { byte };
    let digits = hex_byte(magnitude);
    crate::bios::mscon::con_write_bytes(b" SG");
    put_hex8(if negative { b'-' } else { b'+' });
    put_hex8(digits[0]);
    put_hex8(digits[1]);
}

fn write_memory(mode: u8, field: u8, wide: bool, displacement: u16) {
    let formed = memory(mode, field, wide, displacement);
    crate::bios::mscon::con_write_bytes(b" M");
    write_text(&formed.text);
    crate::bios::mscon::con_write_bytes(b" G");
    put_hex8(formed.segment);
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex16(formed.place);
}

fn write_element(table: &[u8], which: u8) {
    let mut text = Text::new();
    crate::bios::mscon::con_write_bytes(b" E");
    if copy_element(table, which, &mut text) {
        write_text(&text);
    } else {
        crate::bios::mscon::con_write_bytes(b"RV");
    }
}

fn write_text(text: &Text) {
    let mut index = 0usize;
    while index < text.length {
        put_hex8(text.bytes[index]);
        index += 1;
    }
}

impl Text {
    fn new() -> Self {
        Self { bytes: [0; 24], length: 0 }
    }

    fn push(&mut self, byte: u8) {
        if self.length < self.bytes.len() {
            self.bytes[self.length] = byte;
            self.length += 1;
        }
    }
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

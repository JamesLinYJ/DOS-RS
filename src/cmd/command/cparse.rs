//! Command-line token scan from `CMD/COMMAND/CPARSE.ASM`.
//!
//! Spaces and tabs are skipped. One other delimiter is allowed, and a second
//! one ends an empty token. `+` marks the token that follows it. A carriage
//! return sets carry and leaves the token empty. Switches are looked up in
//! `VBAPW`; the bit is the count left after the match. A star expands to
//! question marks.

const SWITCHES: &[u8] = b"VBAPW";
const F_SWITCH: u16 = 0x8000;
const F_BAD: u16 = 0x4000;
const SWITCH_MARK: u8 = b'/';

struct Lex {
    info: u8,
    switches: u16,
    carriage: bool,
    len: u8,
}

fn is_space(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

fn is_delim(byte: u8) -> bool {
    byte == b' ' || byte == b'=' || byte == b',' || byte == b';' || byte == b'\t' || byte == 0x0A
}

fn switch_bit(letter: u8) -> Option<u16> {
    let folded = if letter.is_ascii_lowercase() { letter - 0x20 } else { letter };
    let mut remaining = SWITCHES.len() as u16;
    let mut index = 0usize;
    while index < SWITCHES.len() {
        remaining -= 1;
        if SWITCHES[index] == folded {
            return Some(1u16 << remaining);
        }
        index += 1;
    }
    None
}

fn parse(input: &[u8], special: u8, expand: bool) -> Lex {
    let mut cursor = 0usize;
    let mut info = 0u8;
    let mut switches = 0u16;
    let mut skipped = false;
    let mut len = 0u8;
    let mut element = 0u8;
    let mut in_extension = false;
    while cursor < input.len() {
        let byte = input[cursor];
        cursor += 1;
        if is_space(byte) {
            continue;
        }
        if is_delim(byte) {
            if !skipped {
                skipped = true;
                continue;
            }
            break;
        }
        if byte == special {
            info |= 0x80;
            continue;
        }
        if byte == b'\r' {
            return Lex { info, switches, carriage: true, len: 0 };
        }
        if byte == SWITCH_MARK {
            info |= 1;
            switches |= F_SWITCH;
            if cursor >= input.len() || input[cursor] == b'\r' {
                switches |= F_BAD;
                return Lex { info, switches, carriage: true, len: 0 };
            }
            let letter = input[cursor];
            len = 1;
            if let Some(bit) = switch_bit(letter) {
                switches |= bit;
            } else {
                switches |= F_BAD;
            }
            break;
        }
        let mut current = byte;
        loop {
            if current == b'.' {
                in_extension = true;
                element = 0xFF;
            } else if current == b'?' || current == b'*' {
                info |= 2;
            }
            if current == b'*' && expand {
                let mut marks = if in_extension { 2u8 } else { 7 };
                marks = marks.wrapping_sub(element);
                while marks != 0 {
                    len = len.wrapping_add(1);
                    marks -= 1;
                    element = element.wrapping_add(1);
                }
            } else if current == b'/' || current == b'\\' {
                info |= 4;
                len = len.wrapping_add(1);
                element = 0;
            } else {
                len = len.wrapping_add(1);
                element = element.wrapping_add(1);
            }
            if cursor >= input.len() {
                break;
            }
            let next = input[cursor];
            if is_delim(next) || next == b'\r' || next == SWITCH_MARK || next == special {
                break;
            }
            if next == b':' {
                len = len.wrapping_add(1);
                break;
            }
            cursor += 1;
            current = next;
        }
        break;
    }
    Lex { info, switches, carriage: false, len }
}

fn write_lex(label: &[u8], lex: Lex) {
    crate::bios::mscon::con_write_bytes(label);
    crate::bios::mscon::con_write_bytes(b" I");
    put_hex8(lex.info);
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex16(lex.switches);
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(u8::from(lex.carriage));
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(lex.len);
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"CPARSE");
    write_lex(b" F", parse(b" FOO\r", b'+', true));
    write_lex(b" R", parse(b"\r", b'+', true));
    write_lex(b" P", parse(b"+B\r", b'+', true));
    write_lex(b" V", parse(b"/V ", b'+', true));
    write_lex(b" Q", parse(b"/Q ", b'+', true));
    write_lex(b" E", parse(b"/\r", b'+', true));
    write_lex(b" S", parse(b"*\r", b'+', true));
    write_lex(b" A", parse(b"A*\r", b'+', true));
    write_lex(b" D", parse(b".*\r", b'+', true));
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

//! `$STD_CON_STRING_INPUT` from `DOS/KSTRIN.ASM`.
//!
//! A zero-length buffer returns immediately. A template that is longer than
//! the buffer, or that does not end in carriage return, is discarded. Ctrl-F
//! is ignored. Carriage return ends the line. The last byte of the buffer is
//! reserved, so extra characters are dropped.

const CARRIAGE_RETURN: u8 = 13;
const CTRL_F: u8 = 6;

pub struct Line {
    pub text: [u8; 16],
    pub length: u8,
    pub ended: bool,
}

/// True when the template length fits and its last byte is carriage return.
pub fn keep_template(capacity: u8, template_len: u8, ends_with_return: bool) -> bool {
    if capacity == 0 {
        return false;
    }
    if capacity <= template_len {
        return false;
    }
    ends_with_return
}

/// Read keys into a buffer of `capacity` bytes, including the final return.
pub fn read_line(capacity: u8, keys: &[u8]) -> Line {
    let mut text = [0u8; 16];
    if capacity == 0 {
        return Line {
            text,
            length: 0,
            ended: false,
        };
    }
    let room = capacity - 1;
    let mut length = 0u8;
    let mut ended = false;
    let mut index = 0usize;
    while index < keys.len() {
        let key = keys[index];
        index += 1;
        if key == CTRL_F {
            continue;
        }
        if key == CARRIAGE_RETURN {
            ended = true;
            break;
        }
        if length >= room {
            continue;
        }
        if (length as usize) < text.len() {
            text[length as usize] = key;
        }
        length = length.wrapping_add(1);
    }
    Line { text, length, ended }
}

pub fn show() {
    let empty = read_line(0, b"Hi\r");
    let dropped = keep_template(4, 4, true);
    let hello = read_line(8, b"Hi\r");
    let skipped = read_line(8, b"A\x06B\r");
    let full = read_line(3, b"ABC\r");
    crate::bios::mscon::con_write_bytes(b"KSTRIN Z");
    put_hex8(empty.length);
    crate::bios::mscon::con_write_bytes(if dropped { b" KEEP" } else { b" DROP" });
    crate::bios::mscon::con_write_bytes(b" HI");
    put_hex8(hello.length);
    crate::bios::mscon::con_write_bytes(if hello.ended { b" CR" } else { b" NO" });
    crate::bios::mscon::con_write_bytes(b" SK");
    put_name(&skipped);
    crate::bios::mscon::con_write_bytes(b" FL");
    put_name(&full);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_name(line: &Line) {
    let count = line.length as usize;
    if count == 0 {
        crate::bios::mscon::con_write_bytes(b"00");
        return;
    }
    crate::bios::mscon::con_write_bytes(&line.text[..count]);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

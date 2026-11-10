//! `PFMT`, `MOut`, and `MNum` from `DOS/PRINT.ASM`.
//!
//! Formatting is bracketed by INT 2Ah calls `8007h` and `8107h`. `$S` reads
//! a segment word and then a zero-terminated string. `MNum` rotates one
//! nibble at a time into a four-digit hex field.

use super::dosprint::{self, Piece};

const ENTER_PRINT: u16 = 0x8007;
const LEAVE_PRINT: u16 = 0x8107;

/// `PFMT`. The printer critical section is entered before the template runs.
pub fn pfmt(
    template: &[u8],
    pieces: &[Piece],
    user: u16,
    process: u16,
    out: &mut [u8],
) -> (u16, u16, u16, usize) {
    let entered = ENTER_PRINT;
    let (count, selector) = dosprint::format(template, pieces, user, process, out);
    let left = LEAVE_PRINT;
    (entered, selector, left, count)
}

/// `MOut`. Copy bytes until a zero.
pub fn mout(message: &[u8], out: &mut [u8]) -> usize {
    let mut written = 0usize;
    let mut index = 0usize;
    while index < message.len() {
        let byte = message[index];
        if byte == 0 {
            break;
        }
        index += 1;
        if written < out.len() {
            out[written] = byte;
            written += 1;
        }
    }
    written
}

/// `MNum`. Four hex digits, high nibble first.
pub fn mnum(value: u16, out: &mut [u8]) -> usize {
    let mut number = value;
    let mut written = 0usize;
    let mut digits = 4u8;
    while digits != 0 {
        number = number.rotate_left(4);
        let mut ascii = (number as u8 & 0x0f) + b'0';
        if ascii > b'9' {
            ascii += b'A' - b'0' - 10;
        }
        if written < out.len() {
            out[written] = ascii;
            written += 1;
        }
        digits -= 1;
    }
    written
}

pub fn show() {
    let mut formatted = [0u8; 8];
    let (entered, selector, left, count) = pfmt(
        b"$S$z",
        &[Piece::Segment(0x1234), Piece::Text(b"AB")],
        1,
        2,
        &mut formatted,
    );
    let mut message = [0u8; 4];
    let message_len = mout(b"CD\0", &mut message);
    let mut digits = [0u8; 4];
    let digit_len = mnum(0x0a1b, &mut digits);
    crate::bios::mscon::con_write_bytes(b"PRINT EN");
    put_hex16(entered);
    crate::bios::mscon::con_write_bytes(b" SEL");
    put_hex16(selector);
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(&formatted[..count]);
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(&message[..message_len]);
    crate::bios::mscon::con_write_bytes(b" MN");
    crate::bios::mscon::con_write_bytes(&digits[..digit_len]);
    crate::bios::mscon::con_write_bytes(b" LV");
    put_hex16(left);
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

//! Copy name helpers from `CMD/COMMAND/COPYPR2.ASM`.
//!
//! `/A` and `/B` change the mode only when exactly one of them is set. Both
//! or neither leave the mode alone. An empty destination tail becomes eight
//! question marks, a dot, and three question marks. Two translated names
//! match only when every byte matches.

const SWITCH_A: u8 = 0x04;
const SWITCH_B: u8 = 0x08;

fn apply_switches(switches: u8, mut ascii: u8, mut binary: u8, mut inexact: u8) -> (u8, u8, u8) {
    let masked = switches & (SWITCH_A + SWITCH_B);
    if masked.count_ones() % 2 == 1 {
        binary = masked & SWITCH_B;
        ascii = masked & SWITCH_A;
        inexact |= ascii;
    }
    (ascii, binary, inexact)
}

fn stars(size: u8) -> (u8, [u8; 13]) {
    let mut pattern = [0u8; 13];
    let mut index = 0usize;
    while index < 8 {
        pattern[index] = b'?';
        index += 1;
    }
    pattern[8] = b'.';
    pattern[9] = b'?';
    pattern[10] = b'?';
    pattern[11] = b'?';
    (size.wrapping_add(12), pattern)
}

fn same_name(left: &[u8], right: &[u8]) -> bool {
    let mut index = 0usize;
    loop {
        let left_byte = if index < left.len() { left[index] } else { 0 };
        let right_byte = if index < right.len() { right[index] } else { 0 };
        if left_byte != right_byte {
            return false;
        }
        if left_byte == 0 {
            return true;
        }
        index += 1;
    }
}

pub fn show() {
    let (ascii, binary, inexact) = apply_switches(SWITCH_A, 0, 0, 0);
    crate::bios::mscon::con_write_bytes(b"CPR2 A");
    put_hex8(ascii);
    put_hex8(binary);
    put_hex8(inexact);
    let (ascii, binary, inexact) = apply_switches(SWITCH_B, 0, 0, 0);
    crate::bios::mscon::con_write_bytes(b" B");
    put_hex8(ascii);
    put_hex8(binary);
    put_hex8(inexact);
    let (ascii, binary, inexact) = apply_switches(SWITCH_A + SWITCH_B, 0x09, 0x01, 0);
    crate::bios::mscon::con_write_bytes(b" K");
    put_hex8(ascii);
    put_hex8(binary);
    put_hex8(inexact);
    let (ascii, binary, inexact) = apply_switches(0, 0, 0, 0);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(ascii);
    put_hex8(binary);
    put_hex8(inexact);
    let (size, pattern) = stars(2);
    crate::bios::mscon::con_write_bytes(b" SZ");
    put_hex8(size);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex8(pattern[0]);
    crate::bios::mscon::con_write_bytes(b" EQ");
    put_hex8(u8::from(same_name(b"A:\\FOO\0", b"A:\\FOO\0")));
    crate::bios::mscon::con_write_bytes(b" NE");
    put_hex8(u8::from(same_name(b"A:\\FOO\0", b"A:\\BAR\0")));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

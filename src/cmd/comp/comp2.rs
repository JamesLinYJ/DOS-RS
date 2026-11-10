//! Byte compare from `CMD/COMP/COMP2.ASM`.
//!
//! The low size word is checked before the high word. Matching files are
//! compared a byte at a time. The tenth mismatch ends the compare. A trailing
//! 1Ah in either file is the end-of-file mark. The mark message comes before
//! the success message, and success is printed only when no bytes differed.

const MARK: u8 = 0x1A;
const STOP_AT: u8 = 10;

struct Compared {
    mismatches: u8,
    stopped: bool,
    offset: u32,
    left_byte: u8,
    right_byte: u8,
    mark: bool,
    ok: bool,
}

fn same_size(left: u32, right: u32) -> bool {
    if (left as u16) != (right as u16) {
        return false;
    }
    ((left >> 16) as u16) == ((right >> 16) as u16)
}

fn has_mark(bytes: &[u8]) -> bool {
    match bytes.last() {
        Some(&MARK) => true,
        Some(_) => false,
        None => false,
    }
}

fn compare(left: &[u8], right: &[u8]) -> Compared {
    let mut mismatches = 0u8;
    let mut offset = 0u32;
    let mut left_byte = 0u8;
    let mut right_byte = 0u8;
    let mut index = 0usize;
    while index < left.len() && mismatches < STOP_AT {
        if left[index] != right[index] {
            if mismatches == 0 {
                offset = index as u32;
                left_byte = left[index];
                right_byte = right[index];
            }
            mismatches = mismatches.wrapping_add(1);
        }
        index += 1;
    }
    Compared {
        mismatches,
        stopped: mismatches == STOP_AT,
        offset,
        left_byte,
        right_byte,
        mark: has_mark(left) || has_mark(right),
        ok: mismatches == 0,
    }
}

fn write_compared(result: &Compared) {
    crate::bios::mscon::con_write_bytes(b" MM");
    put_hex8(result.mismatches);
    if result.mismatches != 0 {
        crate::bios::mscon::con_write_bytes(b" OF");
        put_hex32(result.offset);
        crate::bios::mscon::con_write_bytes(b" L");
        put_hex8(result.left_byte);
        crate::bios::mscon::con_write_bytes(b" R");
        put_hex8(result.right_byte);
    }
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex8(u8::from(result.stopped));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(u8::from(result.mark));
    crate::bios::mscon::con_write_bytes(b" OK");
    put_hex8(u8::from(result.ok));
}

pub fn show() {
    let zeros = [0u8; 11];
    let ones = [1u8; 11];
    crate::bios::mscon::con_write_bytes(b"COMP2 LW");
    put_hex8(u8::from(same_size(1, 2)));
    crate::bios::mscon::con_write_bytes(b" HW");
    put_hex8(u8::from(same_size(0x1_0000, 0)));
    crate::bios::mscon::con_write_bytes(b" EQ");
    put_hex8(u8::from(same_size(3, 3)));
    write_compared(&compare(b"AB\x1a", b"AB\x1a"));
    write_compared(&compare(b"AB", b"AB"));
    write_compared(&compare(b"AX", b"AY"));
    write_compared(&compare(&zeros, &ones));
    write_compared(&compare(b"\x1a", b"\x00"));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex32(value: u32) {
    put_hex16((value >> 16) as u16);
    put_hex16(value as u16);
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

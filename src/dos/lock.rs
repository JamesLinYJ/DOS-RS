//! `$LockOper` from `DOS/LOCK.ASM`.
//!
//! AL=0 locks one range. AL=1 unlocks that same range. AL above 1 is
//! `error_invalid_function`. A closed handle is `error_invalid_handle`.
//! An overlapping lock is `error_lock_violation` (33).

const INVALID_FUNCTION: u16 = 1;
const INVALID_HANDLE: u16 = 6;
const LOCK_VIOLATION: u16 = 0x21;

struct Range {
    start: u32,
    len: u32,
    held: bool,
}

static mut LOCK: Range = Range { start: 0, len: 0, held: false };

pub fn oper(result: u8, open: bool, start: u32, len: u32) -> u16 {
    if result > 1 {
        return INVALID_FUNCTION;
    }
    if !open {
        return INVALID_HANDLE;
    }
    unsafe {
        if result == 0 {
            if LOCK.held && overlap(LOCK.start, LOCK.len, start, len) {
                return LOCK_VIOLATION;
            }
            LOCK.start = start;
            LOCK.len = len;
            LOCK.held = true;
            return 0;
        }
        if LOCK.held && LOCK.start == start && LOCK.len == len {
            LOCK.held = false;
            return 0;
        }
        LOCK_VIOLATION
    }
}

fn overlap(a: u32, a_len: u32, b: u32, b_len: u32) -> bool {
    let a_end = a.saturating_add(a_len);
    let b_end = b.saturating_add(b_len);
    a < b_end && b < a_end
}

pub fn show() {
    unsafe {
        LOCK.held = false;
    }
    let first = oper(0, true, 0, 16);
    let again = oper(0, true, 0, 16);
    let unlocked = oper(1, true, 0, 16);
    crate::bios::mscon::con_write_bytes(b"LOCK ");
    crate::bios::mscon::con_write_bytes(if first == 0 { b"OK VL" } else { b"BAD VL" });
    put_hex8(again as u8);
    crate::bios::mscon::con_write_bytes(b" UL");
    put_hex8(unlocked as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[HEX[(value >> 4) as usize], HEX[(value & 0xf) as usize]]);
}

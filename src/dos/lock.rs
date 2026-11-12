//! `$LockOper` from `DOS/LOCK.ASM`.
//!
//! AL=0 locks one range. AL=1 unlocks that same range. AL above 1 is
//! `error_invalid_function`. A closed handle is `error_invalid_handle`.
//! An overlapping lock is `error_lock_violation` (33).

const INVALID_FUNCTION: u16 = 1;
const INVALID_HANDLE: u16 = 6;
const LOCK_VIOLATION: u16 = 0x21;

static LOCK: crate::kernel_cell::KernelCell<Option<(u32, u32)>> =
    crate::kernel_cell::KernelCell::new(None);

pub fn oper(result: u8, open: bool, start: u32, len: u32) -> u16 {
    if result > 1 {
        return INVALID_FUNCTION;
    }
    if !open {
        return INVALID_HANDLE;
    }
    let mut held = LOCK.borrow();
    if result == 0 {
        if held.is_some_and(|(a, b)| overlap(a, b, start, len)) {
            return LOCK_VIOLATION;
        }
        *held = Some((start, len));
        return 0;
    }
    if *held == Some((start, len)) {
        *held = None;
        return 0;
    }
    LOCK_VIOLATION
}

fn overlap(a: u32, a_len: u32, b: u32, b_len: u32) -> bool {
    let a_end = a.saturating_add(a_len);
    let b_end = b.saturating_add(b_len);
    a < b_end && b < a_end
}

pub fn show() {
    *LOCK.borrow() = None;
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

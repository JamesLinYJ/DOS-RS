//! Disk read setup from `DOS/DISK2.ASM`.
//!
//! `BREAKDOWN` splits a transfer into a partial first sector, whole sectors,
//! and a partial last sector. An exact one-sector transfer is rewritten as a
//! last-sector count of the sector size. A non-compat share mode reports a
//! lock violation without calling the critical-error handler.

const SHARING_MASK: u8 = 0xF0;
const SHARING_COMPAT: u8 = 0x00;
const LOCK_VIOLATION: u16 = 33;

pub struct Pieces {
    pub first: u16,
    pub sectors: u16,
    pub last: u16,
}

/// `BREAKDOWN`. `pos` is `BYTSECPOS` and `len` is the byte count in CX.
pub fn breakdown(len: u16, pos: u16, sector_size: u16) -> Pieces {
    let mut status = pos;
    let mut handle = len;
    if status != 0 {
        status = status.wrapping_sub(sector_size);
        status = status.wrapping_neg();
        let (next, borrow) = handle.overflowing_sub(status);
        if !borrow {
            handle = next;
        } else {
            status = status.wrapping_add(handle);
            handle = 0;
        }
    }
    let first = status;
    let sectors = handle.checked_div(sector_size).unwrap_or(0);
    let mut last = if sector_size == 0 { 0 } else { handle % sector_size };
    let mut sectors = sectors;
    if last | first == 0 && sectors == 1 {
        last = sector_size;
        sectors = 0;
    }
    Pieces { first, sectors, last }
}

/// `READ_LOCK_VIOLATION` when the handle is not an FCB and the share mode is
/// not compatibility. CX is cleared and AX is `error_lock_violation`.
pub fn read_lock_violation(mode: u8, is_fcb: bool) -> Option<u16> {
    let share = mode & SHARING_MASK;
    if is_fcb || share == SHARING_COMPAT {
        return None;
    }
    Some(LOCK_VIOLATION)
}

pub fn show() {
    let partial = breakdown(1000, 100, 512);
    let exact = breakdown(512, 0, 512);
    let locked = read_lock_violation(0x42, false);
    crate::bios::mscon::con_write_bytes(b"DISK2 BD");
    put_hex16(partial.first);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(partial.sectors);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(partial.last);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex16(exact.first);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(exact.sectors);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(exact.last);
    crate::bios::mscon::con_write_bytes(b" LK");
    put_hex16(locked.unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[((value >> 12) & 0xf) as usize],
        hex[((value >> 8) & 0xf) as usize],
        hex[((value >> 4) & 0xf) as usize],
        hex[(value & 0xf) as usize],
    ]);
}

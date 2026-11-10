//! INT 24h reply handling from `DOS/CTRLC.ASM`.
//!
//! `HardErr` classifies the failing sector, then `SET_I24_EXTENDED_ERROR`
//! maps the device code through `ErrMap24`. The user's reply is 0 ignore,
//! 1 retry, 3 fail, and anything else abort. A reply that is not allowed
//! becomes a fail, and a second abort while one is in progress becomes a fail.

use super::ms_code;

const ALLOWED_FAIL: u8 = 0x08;
const ALLOWED_RETRY: u8 = 0x10;
const ALLOWED_IGNORE: u8 = 0x20;

/// Device code 0 through 15, from `ErrMap24` in `DOS/MS_TABLE.ASM`.
const ERR_MAP_24: [u8; 16] = [
    19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 31, 31, 34,
];

static mut ALLOWED: u8 = 0;
static mut FAILERR: u8 = 0;
static mut ABORTING: bool = false;
static mut WPERR: u8 = 0xff;

/// `HardErr` area field. 0 DOS, 1 FAT, 2 directory, 3 data, then the read/write bit.
pub fn area(sector: u16, first_fat: u16, dir_sector: u16, first_data: u16, readop: u8) -> u8 {
    let mut ah = 0u8;
    if sector >= first_fat {
        ah = 1;
        if sector >= dir_sector {
            ah = 2;
            if sector >= first_data {
                ah = 3;
            }
        }
    }
    (ah << 1) | (readop & 1)
}

/// `SET_I24_EXTENDED_ERROR`. Codes past the map stay as they are.
pub fn set_i24(di: u16, drive: u8) -> u16 {
    if di == 0 {
        unsafe { WPERR = drive }
    }
    let code = if (di as usize) < ERR_MAP_24.len() {
        u16::from(ERR_MAP_24[di as usize])
    } else {
        di
    };
    // `ERR_TABLE_24` for write-protect: media, retry after the user, disk.
    let (class, action, locus) = match code {
        19 => (11, 7, 2),
        21 => (5, 7, 2),
        _ => (11, 7, 2),
    };
    ms_code::store(code, class, action, locus);
    code
}

/// Triage AL. `ALLOWED` is the mask from the INT 24 AH register.
pub fn user_reply(al: u8, allowed: u8) -> u8 {
    unsafe { ALLOWED = allowed }
    if al < 1 {
        if unsafe { ALLOWED } & ALLOWED_IGNORE == 0 {
            return do_fail();
        }
        return al;
    }
    if al == 1 {
        if unsafe { ALLOWED } & ALLOWED_RETRY == 0 {
            return do_fail();
        }
        return al;
    }
    if al != 3 {
        return do_abort();
    }
    if unsafe { ALLOWED } & ALLOWED_FAIL == 0 {
        return do_abort();
    }
    do_fail()
}

fn do_fail() -> u8 {
    unsafe { FAILERR = FAILERR.wrapping_add(1) }
    3
}

fn do_abort() -> u8 {
    if unsafe { ABORTING } {
        return do_fail();
    }
    unsafe { ABORTING = true }
    crate::dos::abort::dos_abort();
    0
}

pub fn show() {
    unsafe {
        FAILERR = 0;
        ABORTING = false;
        WPERR = 0xff;
    }
    let spot = area(1, 1, 3, 4, 0);
    let mapped = set_i24(0, 0);
    let (ax, bx, cx) = ms_code::get();
    let allowed = ALLOWED_FAIL | ALLOWED_RETRY | ALLOWED_IGNORE;
    let ignore = user_reply(0, allowed);
    let retry = user_reply(1, allowed);
    let fail = user_reply(3, allowed);
    let _abort = user_reply(2, allowed);
    let aborted = unsafe { ABORTING };
    crate::bios::mscon::con_write_bytes(b"I24 AREA");
    put_hex8(spot);
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex16(ax);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8((bx >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(bx as u8);
    crate::bios::mscon::con_write_bytes(b" LC");
    put_hex8((cx >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" IG");
    put_hex8(ignore);
    crate::bios::mscon::con_write_bytes(b" RT");
    put_hex8(retry);
    crate::bios::mscon::con_write_bytes(b" FL");
    put_hex8(fail);
    crate::bios::mscon::con_write_bytes(if aborted { b" AB\r\n" } else { b" NO\r\n" });
    let _ = mapped;
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

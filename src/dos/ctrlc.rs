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

use core::sync::atomic::{AtomicBool, Ordering};

static ABORTING: AtomicBool = AtomicBool::new(false);

/// `HardErr` area field. 0 DOS, 1 FAT, 2 directory, 3 data, then the read/write bit.
pub fn area(sector: u16, first_fat: u16, dir_sector: u16, first_data: u16, readop: u8) -> u8 {
    let mut function = 0u8;
    if sector >= first_fat {
        function = 1;
        if sector >= dir_sector {
            function = 2;
            if sector >= first_data {
                function = 3;
            }
        }
    }
    (function << 1) | (readop & 1)
}

/// `SET_I24_EXTENDED_ERROR`. Codes past the map stay as they are.
pub fn set_i24(destination: u16, _drive: u8) -> u16 {
    let code = if (destination as usize) < ERR_MAP_24.len() {
        u16::from(ERR_MAP_24[destination as usize])
    } else {
        destination
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
pub fn user_reply(result: u8, allowed: u8) -> u8 {
    if result < 1 {
        if allowed & ALLOWED_IGNORE == 0 {
            return do_fail();
        }
        return result;
    }
    if result == 1 {
        if allowed & ALLOWED_RETRY == 0 {
            return do_fail();
        }
        return result;
    }
    if result != 3 {
        return do_abort();
    }
    if allowed & ALLOWED_FAIL == 0 {
        return do_abort();
    }
    do_fail()
}

fn do_fail() -> u8 {
    3
}

fn do_abort() -> u8 {
    if ABORTING.swap(true, Ordering::Relaxed) {
        return do_fail();
    }
    crate::dos::abort::dos_abort();
    0
}

pub fn show() {
    ABORTING.store(false, Ordering::Relaxed);
    let spot = area(1, 1, 3, 4, 0);
    let mapped = set_i24(0, 0);
    let (status, handle, count) = ms_code::get();
    let allowed = ALLOWED_FAIL | ALLOWED_RETRY | ALLOWED_IGNORE;
    let ignore = user_reply(0, allowed);
    let retry = user_reply(1, allowed);
    let fail = user_reply(3, allowed);
    let _abort = user_reply(2, allowed);
    let aborted = ABORTING.load(Ordering::Relaxed);
    crate::bios::mscon::con_write_bytes(b"I24 AREA");
    put_hex8(spot);
    crate::bios::mscon::con_write_bytes(b" ER");
    put_hex16(status);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8((handle >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(handle as u8);
    crate::bios::mscon::con_write_bytes(b" LC");
    put_hex8((count >> 8) as u8);
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

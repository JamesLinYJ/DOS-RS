//! Verify flag from `DOS/GETSET.ASM`.
//!
//! `$GET_VERIFY_ON_WRITE` returns `VERFLG` in AL. `$SET_VERIFY_ON_WRITE`
//! stores AL bit 0. The byte has no initializer, so it starts at 0.

static mut VERFLG: u8 = 0;

pub fn get_verify() -> u8 {
    unsafe { VERFLG }
}

pub fn set_verify(al: u8) {
    unsafe { VERFLG = al & 1 }
}

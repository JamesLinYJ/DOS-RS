//! DOS version and verify flag from `DOS/GETSET.ASM`.
//!
//! `$GET_VERIFY_ON_WRITE` returns `VERFLG` in AL. `$SET_VERIFY_ON_WRITE`
//! stores AL bit 0. The byte has no initializer, so it starts at 0.

/// AH=30h returns the DOS version, OEM and serial words, preserving FLAGS.
pub const VERSION: (u16, u16, u16) = (0x0004, 0xFF00, 0x0000);

use core::sync::atomic::{AtomicU8, Ordering};

static VERFLG: AtomicU8 = AtomicU8::new(0);

pub fn get_verify() -> u8 {
    VERFLG.load(Ordering::Relaxed)
}

pub fn set_verify(result: u8) {
    VERFLG.store(result & 1, Ordering::Relaxed)
}

//! `$SET_CTRL_C_TRAPPING` from `DOS/DISP.ASM`.
//!
//! AL=0 copies `CntCFlag` into DL. AL=1 stores DL bit 0. AL=2 exchanges
//! that bit with the flag. AL=3 and AL=4 return as they stand. AL=5 copies
//! `BOOTDRIVE`, which this image leaves at 0. Any other AL becomes `0xFF`.

use core::sync::atomic::{AtomicU8, Ordering};

static CNTC_FLAG: AtomicU8 = AtomicU8::new(0);

/// `MSCONST.ASM` `BOOTDRIVE` has no initializer, so the byte is 0.
const BOOT_DRIVE: u8 = 0;

/// Returns the AL and DL the interrupt hands back.
pub fn set_ctrl_c(result: u8, data_low: u8) -> (u8, u8) {
    if result == 0 {
        return (result, CNTC_FLAG.load(Ordering::Relaxed));
    }
    if result > 2 {
        return ctrl_c_extended(result, data_low);
    }
    if result == 2 {
        let old = CNTC_FLAG.swap(data_low & 1, Ordering::Relaxed);
        return (result, old);
    }
    CNTC_FLAG.store(data_low & 1, Ordering::Relaxed);
    (result, data_low)
}

fn ctrl_c_extended(result: u8, data_low: u8) -> (u8, u8) {
    if result == 3 || result == 4 {
        return (result, data_low);
    }
    if result == 5 {
        return (result, BOOT_DRIVE);
    }
    (0xFF, data_low)
}

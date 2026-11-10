//! `$SET_CTRL_C_TRAPPING` from `DOS/DISP.ASM`.
//!
//! AL=0 copies `CntCFlag` into DL. AL=1 stores DL bit 0. AL=2 exchanges
//! that bit with the flag. AL=3 and AL=4 return as they stand. AL=5 copies
//! `BOOTDRIVE`, which this image leaves at 0. Any other AL becomes `0xFF`.

static mut CNTC_FLAG: u8 = 0;

/// `MSCONST.ASM` `BOOTDRIVE` has no initializer, so the byte is 0.
const BOOT_DRIVE: u8 = 0;

/// Returns the AL and DL the interrupt hands back.
pub fn set_ctrl_c(al: u8, dl: u8) -> (u8, u8) {
    if al == 0 {
        return (al, unsafe { CNTC_FLAG });
    }
    if al > 2 {
        return ctrl_c_extended(al, dl);
    }
    if al == 2 {
        let old = unsafe { CNTC_FLAG };
        unsafe { CNTC_FLAG = dl & 1 }
        return (al, old);
    }
    unsafe { CNTC_FLAG = dl & 1 }
    (al, dl)
}

fn ctrl_c_extended(al: u8, dl: u8) -> (u8, u8) {
    if al == 3 || al == 4 {
        return (al, dl);
    }
    if al == 5 {
        return (al, BOOT_DRIVE);
    }
    (0xFF, dl)
}

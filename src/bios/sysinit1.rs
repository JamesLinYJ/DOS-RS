//! Missing command interpreter from `BIOS/SYSINIT1.ASM`.
//!
//! `COMERR` loads DX with `BADCOM` and calls `BADFIL`. The original then
//! jumps to `STALL` and does not return. This boot returns after the
//! message so the debug port can still exit.

use super::sysimes;
use super::sysinit2;

pub fn com_err() {
    sysinit2::badfil(sysimes::BADCOM);
}

/// `ParaRound`. Add 15, rotate the carry in, then shift right three more
/// times. That is a paragraph round-up, including a 16-bit overflow.
pub fn para_round(ax: u16) -> u16 {
    let sum = u32::from(ax) + 15;
    let carry = u32::from(sum > 0xffff);
    let mut ax = sum & 0xffff;
    ax = (ax >> 1) | (carry << 15);
    ax >>= 1;
    ax >>= 1;
    ax >>= 1;
    ax as u16
}

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
/// `ROUND`. Paragraph-round `memlo`, add it to `memhi`, and fail when the
/// result meets `alloclim`.
pub fn round_mem(memlo: u16, memhi: u16, alloclim: u16) -> Result<u16, ()> {
    let next = memhi.wrapping_add(para_round(memlo));
    if next >= alloclim {
        Err(())
    } else {
        Ok(next)
    }
}

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

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
/// A CONFIG line. `BUFFERS=` updates the count. Anything else is the
/// unrecognized-command path, which prints `BADOPM`.
pub fn config_line(line: &[u8]) -> Result<Option<u16>, ()> {
    let upper = b"BUFFERS=";
    if line.len() >= upper.len() && line[..upper.len()].eq_ignore_ascii_case(upper) {
        let count = super::sysinit2::get_num(&line[upper.len()..])?;
        return Ok(Some(count));
    }
    Err(())
}

/// Default buffer count from the allocation path. Unset stays 2. A removable
/// drive larger than 360KB raises it to 3. A fixed disk is ignored.
pub fn default_buffers(already_set: bool, slash_x: bool, large_removable: bool) -> u16 {
    if already_set && !slash_x {
        return 0xFFFF;
    }
    if !large_removable {
        2
    } else {
        3
    }
}

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

pub fn para_round(status: u16) -> u16 {
    let sum = u32::from(status) + 15;
    let carry = u32::from(sum > 0xffff);
    let mut status = sum & 0xffff;
    status = (status >> 1) | (carry << 15);
    status >>= 1;
    status >>= 1;
    status >>= 1;
    status as u16
}

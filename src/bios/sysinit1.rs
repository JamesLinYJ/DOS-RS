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

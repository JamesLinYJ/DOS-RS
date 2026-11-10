//! `$STD_CON_OUTPUT` from `DOS/CPMIO2.ASM`, system call 2.
//!
//! DL is the character. Carriage return and line feed both fall through to
//! the console write. Cursor column, the 64-character status check, and the
//! printer-echo tail are not in this slice.

use crate::bios::mscon;

pub fn out_char(data_low: u8) {
    mscon::con_write_bytes(&[data_low]);
}

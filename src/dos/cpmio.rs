//! `$STD_CON_STRING_OUTPUT` from `DOS/CPMIO.ASM`, system call 9.
//!
//! DS:DX points at a string. Bytes are written until `$`, and `$` itself
//! is not written. DBCS scanning is not in this slice.

use super::cpmio2;

/// Walk the `$`-terminated string at `dx` and hand each byte to call 2.
pub fn write_dollar_string(dx: u32) {
    let ptr = dx as *const u8;
    let mut index = 0usize;
    loop {
        let byte = unsafe { *ptr.add(index) };
        if byte == b'$' {
            break;
        }
        cpmio2::out_char(byte);
        index += 1;
    }
}

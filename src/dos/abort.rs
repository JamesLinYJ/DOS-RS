//! `DOS_ABORT` from `DOS/ABORT.ASM`.
//!
//! The process data block's JFN table is walked from the last handle down
//! to zero, and each one is closed. This image has a single real handle,
//! so the other closes are the invalid-handle return.

const JFN_COUNT: u16 = 20;

pub fn dos_abort() {
    let mut left = JFN_COUNT;
    while left > 0 {
        left -= 1;
        let _ = crate::dos::handle::close(left);
    }
}

pub fn show() {
    let name = b"BAR.COM\0";
    let _ = crate::dos::handle::open_asciiz(name.as_ptr());
    dos_abort();
    let again = crate::dos::handle::close(5);
    crate::bios::mscon::con_write_bytes(b"ABORT H");
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(again >> 4) as usize],
        hex[(again & 0x0f) as usize],
    ]);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

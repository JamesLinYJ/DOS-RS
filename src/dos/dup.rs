//! `DOS_DUP` from `DOS/DUP.ASM`.
//!
//! A local SFT is not a network handle, so the device open is skipped.
//! The reference count is incremented. Wrapping is the same trouble the
//! original comment names, and the add is wrapping on purpose.

static mut REF_COUNT: u16 = 1;

pub fn dos_dup() -> u16 {
    unsafe {
        REF_COUNT = REF_COUNT.wrapping_add(1);
        REF_COUNT
    }
}

pub fn show() {
    let count = dos_dup();
    crate::bios::mscon::con_write_bytes(b"DUP REF");
    put_hex16(count);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        HEX[((value >> 12) & 0xf) as usize],
        HEX[((value >> 8) & 0xf) as usize],
        HEX[((value >> 4) & 0xf) as usize],
        HEX[(value & 0xf) as usize],
    ]);
}

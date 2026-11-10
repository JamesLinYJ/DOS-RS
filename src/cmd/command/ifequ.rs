//! Switch-dependent banner from `CMD/COMMAND/IFEQU.ASM`.
//!
//! The IBM build announces the DBCS-enabled version. Any other build
//! announces the normal version. The Kanji test in this file is commented out.

fn ibm_banner(ibm: bool) -> u8 {
    if ibm { 1 } else { 0 }
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"IFEQU IB");
    put_hex8(ibm_banner(true));
    crate::bios::mscon::con_write_bytes(b" IB");
    put_hex8(ibm_banner(false));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

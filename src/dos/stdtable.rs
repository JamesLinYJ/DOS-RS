//! Dispatch limits from `DOS/STDTABLE.ASM`.
//!
//! The standard switches are off for IBM. `MAXCALL` is the last CP/M-compatible
//! function, `24h`. `MAXCOM` is the last INT 21h function, `6Ch`.

const IBM: u16 = 0;
const MAX_CALL: u8 = 0x24;
const MAX_COMMAND: u8 = 0x6c;

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"STDTABLE IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" CALL");
    put_hex8(MAX_CALL);
    crate::bios::mscon::con_write_bytes(b" COM");
    put_hex8(MAX_COMMAND);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

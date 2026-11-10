//! Standard console device I/O from `DOS/STDCPMIO.ASM`.
//!
//! Escape is `1Bh` and cancel is Ctrl-X. A ready console byte is returned
//! unchanged, and a zero byte is a scan code.

const IBM: u16 = 0;
const ESCAPE: u8 = 0x1b;
const CANCEL: u8 = b'X' - b'@';

pub fn show() {
    let letter = crate::dos::mscpmio::input_ready(b'A');
    crate::bios::mscon::con_write_bytes(b"STDCPMIO IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(ESCAPE);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(CANCEL);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(letter.byte);
    crate::bios::mscon::con_write_bytes(if letter.scan { b" SC01" } else { b" SC00" });
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

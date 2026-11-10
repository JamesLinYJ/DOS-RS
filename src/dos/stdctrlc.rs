//! Standard Ctrl-C checks from `DOS/STDCTRLC.ASM`.
//!
//! `TOGLPRN` is false, so Ctrl-N is its own printer-off key. Ctrl-C is still
//! the break key. Any other byte is left for normal input.

const IBM: u16 = 0;
const CTRL_N: u8 = b'N' - b'@';
const CTRL_C: u8 = b'C' - b'@';

pub enum Key {
    PrinterOff,
    Break,
    Plain,
}

pub fn classify(byte: u8) -> Key {
    if byte == CTRL_N {
        return Key::PrinterOff;
    }
    if byte == CTRL_C {
        return Key::Break;
    }
    Key::Plain
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"STDCTRLC IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(CTRL_N);
    crate::bios::mscon::con_write_bytes(match classify(CTRL_N) {
        Key::PrinterOff => b" OFF",
        _ => b" NO",
    });
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(CTRL_C);
    crate::bios::mscon::con_write_bytes(match classify(CTRL_C) {
        Key::Break => b" BRK",
        _ => b" NO",
    });
    crate::bios::mscon::con_write_bytes(match classify(b'A') {
        Key::Plain => b" PLAIN",
        _ => b" HIT",
    });
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

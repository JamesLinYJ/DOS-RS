//! Console input without echo from `DOS/MSCPMIO.ASM`.
//!
//! The IBM wrapper includes `CPMIO.ASM` with DBCS off. A ready console
//! returns the byte. A zero byte sets the scan flag so Alt-Q can be told
//! from a normal character. A `$` string stops before the dollar sign.

const IBM: u16 = 0xffff;

pub struct Key {
    pub byte: u8,
    pub scan: bool,
}

/// `$STD_CON_INPUT_NO_ECHO` once the status call says a byte is waiting.
pub fn input_ready(byte: u8) -> Key {
    Key {
        byte,
        scan: byte == 0,
    }
}

/// `$STD_CON_STRING_OUTPUT`. Returns how many bytes precede `$`.
pub fn dollar_length(text: &[u8]) -> usize {
    let mut count = 0usize;
    while count < text.len() && text[count] != b'$' && text[count] != 0 {
        count += 1;
    }
    count
}

pub fn show() {
    let letter = input_ready(b'A');
    let scan = input_ready(0);
    let length = dollar_length(b"HI$");
    crate::bios::mscon::con_write_bytes(b"MSCPMIO IBM");
    put_hex16(IBM);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(letter.byte);
    crate::bios::mscon::con_write_bytes(if letter.scan { b" SC01" } else { b" SC00" });
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(scan.byte);
    crate::bios::mscon::con_write_bytes(if scan.scan { b" SC01" } else { b" SC00" });
    crate::bios::mscon::con_write_bytes(b" STR");
    put_hex8(length as u8);
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

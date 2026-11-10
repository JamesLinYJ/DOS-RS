//! Resident data from `CMD/COMMAND/RDATA.ASM`.
//!
//! Echo starts on, in the low bit of a queue. Init pretends the last command
//! was external, verify has not been saved, and the extended-attribute buffer
//! is absent. The default switch is `-` and the default separator is `/`.

const ECHO_FLAG: u8 = 0b0000_0001;
const SUPPRESS: u8 = 1;
const EXTERNAL: u8 = 1;
const VERIFY_SAVED: u16 = 0xFFFF;
const INIT_FLAG: u8 = 0x01;
const SWITCH_CHAR: u8 = b'-';
const SEPARATOR: u8 = b'/';
const ATTRIBUTE_SEGMENT: u16 = 0xFFFF;
const PERMANENT: u8 = 0;
const SINGLE: u16 = 0;
const BATCH: u16 = 0;
const PIPE: u8 = 0;
const APPEND: u8 = 0;
const DISPLAY_CLASS: u8 = 0xFF;
const SUBSTITUTIONS: u8 = 0;
const DRIVE_LETTER: u8 = b'A';
const READ_WORD: &[u8] = b"read";
const WRITE_WORD: &[u8] = b"writ";
const ERROR_COUNT: u8 = 18;

fn echo_on(flag: u8) -> bool {
    flag & 1 != 0
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"RDATA EC");
    put_hex8(ECHO_FLAG);
    put_hex8(u8::from(echo_on(ECHO_FLAG)));
    put_hex8(u8::from(echo_on(ECHO_FLAG >> 1)));
    crate::bios::mscon::con_write_bytes(b" SU");
    put_hex8(SUPPRESS);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(EXTERNAL);
    crate::bios::mscon::con_write_bytes(b" VF");
    put_hex16(VERIFY_SAVED);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(INIT_FLAG);
    crate::bios::mscon::con_write_bytes(b" SW");
    put_hex8(SWITCH_CHAR);
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(SEPARATOR);
    crate::bios::mscon::con_write_bytes(b" XA");
    put_hex16(ATTRIBUTE_SEGMENT);
    crate::bios::mscon::con_write_bytes(b" PM");
    put_hex8(PERMANENT);
    crate::bios::mscon::con_write_bytes(b" SG");
    put_hex16(SINGLE);
    crate::bios::mscon::con_write_bytes(b" BT");
    put_hex16(BATCH);
    crate::bios::mscon::con_write_bytes(b" PF");
    put_hex8(PIPE);
    crate::bios::mscon::con_write_bytes(b" AP");
    put_hex8(APPEND);
    crate::bios::mscon::con_write_bytes(b" DS");
    put_hex8(DISPLAY_CLASS);
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(SUBSTITUTIONS);
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(DRIVE_LETTER);
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(READ_WORD[0]);
    put_hex8(READ_WORD[3]);
    crate::bios::mscon::con_write_bytes(b" WR");
    put_hex8(WRITE_WORD[0]);
    put_hex8(WRITE_WORD[3]);
    crate::bios::mscon::con_write_bytes(b" EN");
    put_hex8(ERROR_COUNT);
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

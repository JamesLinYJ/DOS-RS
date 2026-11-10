//! Command equates from `CMD/COMMAND/COMEQU.ASM`.
//!
//! These values do not depend on the IBM switch. A line that starts with `@`
//! is not echoed. Directory listings allow `/P` and `/W`. Copy allows `/V`,
//! `/A`, and `/B`.

const PROMPT: u8 = b'>';
const LINES_PER_PAGE: u16 = 25;
const NORMAL_PER_LINE: u8 = 1;
const WIDE_PER_LINE: u8 = 5;
const COMMAND_BUFFER: u16 = 128;
const MAX_NEST: u16 = 10;
const NO_ECHO_CHAR: u8 = b'@';
const IN_WORD: u16 = 0x4E49;
const DO_WORD: u16 = 0x4F44;
const SWITCH_V: u16 = 0x10;
const SWITCH_B: u16 = 0x08;
const SWITCH_A: u16 = 0x04;
const SWITCH_P: u16 = 0x02;
const SWITCH_W: u16 = 0x01;
const F_SWITCH: u16 = 0x8000;
const ANSWER_YES: u8 = 1;
const ANSWER_NO: u8 = 0;
const END_OF_LINE: u16 = 0xFFFF;
const END_OF_LINE_IN: u8 = 0x0D;
const RESULT_NUMBER: u8 = 1;
const RESULT_STRING: u8 = 3;
const RESULT_FILE: u8 = 5;
const RESULT_DRIVE: u8 = 6;
const RESULT_DATE: u8 = 7;
const RESULT_TIME: u8 = 8;
const RESULT_OK: u8 = 0;
const INIT_RUNNING: u8 = 0x01;
const INIT_SPECIAL: u8 = 0x02;
const INIT_CTRL_C: u8 = 0x04;
const FAIL_ALLOWED: u8 = 0x08;
const RETRY_ALLOWED: u8 = 0x10;
const IGNORE_ALLOWED: u8 = 0x20;
const ARG_MAX: u16 = 64;

fn switch_dir() -> u16 {
    SWITCH_P + SWITCH_W + F_SWITCH
}

fn switch_copy() -> u16 {
    SWITCH_V + SWITCH_A + SWITCH_B + F_SWITCH
}

/// A batch line is echoed unless its first character is `@`.
pub fn echoes(first: u8) -> bool {
    first != NO_ECHO_CHAR
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COMEQU SYM");
    put_hex8(PROMPT);
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex16(LINES_PER_PAGE);
    crate::bios::mscon::con_write_bytes(b" NL");
    put_hex8(NORMAL_PER_LINE);
    crate::bios::mscon::con_write_bytes(b" WL");
    put_hex8(WIDE_PER_LINE);
    crate::bios::mscon::con_write_bytes(b" CB");
    put_hex16(COMMAND_BUFFER);
    crate::bios::mscon::con_write_bytes(b" NT");
    put_hex16(MAX_NEST);
    crate::bios::mscon::con_write_bytes(b" EC");
    put_hex8(NO_ECHO_CHAR);
    crate::bios::mscon::con_write_bytes(b" EA");
    put_hex8(u8::from(echoes(NO_ECHO_CHAR)));
    crate::bios::mscon::con_write_bytes(b" EB");
    put_hex8(u8::from(echoes(b'A')));
    crate::bios::mscon::con_write_bytes(b" IW");
    put_hex16(IN_WORD);
    crate::bios::mscon::con_write_bytes(b" DW");
    put_hex16(DO_WORD);
    crate::bios::mscon::con_write_bytes(b" SD");
    put_hex16(switch_dir());
    crate::bios::mscon::con_write_bytes(b" SC");
    put_hex16(switch_copy());
    crate::bios::mscon::con_write_bytes(b" Y");
    put_hex8(ANSWER_YES);
    crate::bios::mscon::con_write_bytes(b" N");
    put_hex8(ANSWER_NO);
    crate::bios::mscon::con_write_bytes(b" EL");
    put_hex16(END_OF_LINE);
    crate::bios::mscon::con_write_bytes(b" EI");
    put_hex8(END_OF_LINE_IN);
    crate::bios::mscon::con_write_bytes(b" RN");
    put_hex8(RESULT_NUMBER);
    crate::bios::mscon::con_write_bytes(b" RS");
    put_hex8(RESULT_STRING);
    crate::bios::mscon::con_write_bytes(b" RF");
    put_hex8(RESULT_FILE);
    crate::bios::mscon::con_write_bytes(b" RD");
    put_hex8(RESULT_DRIVE);
    crate::bios::mscon::con_write_bytes(b" DA");
    put_hex8(RESULT_DATE);
    crate::bios::mscon::con_write_bytes(b" TM");
    put_hex8(RESULT_TIME);
    crate::bios::mscon::con_write_bytes(b" OK");
    put_hex8(RESULT_OK);
    crate::bios::mscon::con_write_bytes(b" II");
    put_hex8(INIT_RUNNING);
    crate::bios::mscon::con_write_bytes(b" IS");
    put_hex8(INIT_SPECIAL);
    crate::bios::mscon::con_write_bytes(b" IC");
    put_hex8(INIT_CTRL_C);
    crate::bios::mscon::con_write_bytes(b" FA");
    put_hex8(FAIL_ALLOWED);
    crate::bios::mscon::con_write_bytes(b" RY");
    put_hex8(RETRY_ALLOWED);
    crate::bios::mscon::con_write_bytes(b" IG");
    put_hex8(IGNORE_ALLOWED);
    crate::bios::mscon::con_write_bytes(b" AR");
    put_hex16(ARG_MAX);
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

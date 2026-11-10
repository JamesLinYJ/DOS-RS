//! `ECHO` and `ON_OFF` from `CMD/COMMAND/TUCODE.ASM`.
//!
//! The tail at 81h is the command line after the name (`cmd_copy` adds
//! `PathPos`). `ON` sets the echo bit and `OFF` clears it. A blank tail
//! prints message 1057. Any other tail is printed from 82h, then message 1070.

use crate::bios::mscon::con_write_bytes;

/// `RDATA.ASM` `ECHOFLAG`. The low bit is on.
static mut ECHO_FLAG: u8 = 0b0000_0001;

const ECHO_IS: &[u8] = b"ECHO is ";
const ON_TEXT: &[u8] = b"on";
const OFF_TEXT: &[u8] = b"off";

pub(crate) enum OnOff {
    On,
    Off,
    Blank,
    Other,
}

/// `CNTRLC` / `BREAK`. ON and OFF call INT 21h AH=33h AL=1. A blank tail
/// reads the flag with AL=0 and prints message 1055. Anything else is
/// message 1047.
pub fn break_cmd(tail: &[u8]) {
    match on_off(tail) {
        OnOff::On => {
            dos_break(1, 1);
        }
        OnOff::Off => {
            dos_break(1, 0);
        }
        OnOff::Blank => {
            let data_low = dos_break(0, 0);
            con_write_bytes(b"BREAK is ");
            con_write_bytes(if data_low & 1 == 0 { OFF_TEXT } else { ON_TEXT });
            crlf2();
        }
        OnOff::Other => {
            con_write_bytes(b"Must specify ON or OFF\r\n");
        }
    }
}

/// `VERIFY`. ON and OFF call INT 21h AH=2Eh. A blank tail reads the flag
/// with AH=54h and prints message 1056.
pub fn verify_cmd(tail: &[u8]) {
    match on_off(tail) {
        OnOff::On => dos_verify_set(1),
        OnOff::Off => dos_verify_set(0),
        OnOff::Blank => {
            let result = dos_verify_get();
            con_write_bytes(b"VERIFY is ");
            con_write_bytes(if result & 1 == 0 { OFF_TEXT } else { ON_TEXT });
            crlf2();
        }
        OnOff::Other => {
            con_write_bytes(b"Must specify ON or OFF\r\n");
        }
    }
}

fn dos_verify_set(flag: u8) {
    crate::dos::getset::set_verify(flag);
}

fn dos_verify_get() -> u8 {
    crate::dos::getset::get_verify()
}

fn dos_break(subfunction: u8, flag: u8) -> u8 {
    let (_result, flag) = crate::dos::disp::set_ctrl_c(subfunction, flag);
    flag
}

/// `ECHO`. `line` is the text `cmd_copy` stored at 81h, including the CR.
pub fn echo(tail: &[u8]) {
    match on_off(tail) {
        OnOff::On => unsafe {
            ECHO_FLAG |= 1;
        },
        OnOff::Off => unsafe {
            ECHO_FLAG &= !1;
        },
        OnOff::Blank => print_state(),
        OnOff::Other => {
            crprint(skip_one(tail));
            crlf2();
        }
    }
}

fn print_state() {
    con_write_bytes(ECHO_IS);
    let on = unsafe { ECHO_FLAG & 1 != 0 };
    con_write_bytes(if on { ON_TEXT } else { OFF_TEXT });
    crlf2();
}

/// Message 1070 is a bare CR LF.
fn crlf2() {
    con_write_bytes(b"\r\n");
}

/// `CRPRINT` copies through the CR, then prints that string.
fn crprint(text: &[u8]) {
    let mut end = 0usize;
    while end < text.len() && text[end] != b'\r' {
        end += 1;
    }
    con_write_bytes(&text[..end]);
}

fn skip_one(tail: &[u8]) -> &[u8] {
    if tail.is_empty() {
        tail
    } else {
        &tail[1..]
    }
}

/// `ON_OFF`. Leading blanks, tabs, and one `=` are skipped. The word has to
/// be `ON` or `OFF` and the rest of the line has to be empty.
pub(crate) fn on_off(tail: &[u8]) -> OnOff {
    let mut index = 0usize;
    while index < tail.len() {
        let byte = tail[index];
        index += 1;
        if byte == b' ' || byte == b'\t' {
            continue;
        }
        if byte == b'=' {
            break;
        }
        index -= 1;
        break;
    }
    let (token, rest) = token_at(&tail[index..]);
    if token.is_empty() {
        return OnOff::Blank;
    }
    if rest_is_end(rest) && token.eq_ignore_ascii_case(b"ON") {
        return OnOff::On;
    }
    if rest_is_end(rest) && token.eq_ignore_ascii_case(b"OFF") {
        return OnOff::Off;
    }
    OnOff::Other
}

fn token_at(tail: &[u8]) -> (&[u8], &[u8]) {
    let mut end = 0usize;
    while end < tail.len() && !is_delim(tail[end]) && tail[end] != b'\r' && tail[end] != b'\n' {
        end += 1;
    }
    (&tail[..end], &tail[end..])
}

fn rest_is_end(mut rest: &[u8]) -> bool {
    while let Some((&byte, next)) = rest.split_first() {
        if byte == b'\r' || byte == b'\n' || byte == 0 {
            return true;
        }
        if !is_delim(byte) {
            return false;
        }
        rest = next;
    }
    true
}

fn is_delim(byte: u8) -> bool {
    byte == b' ' || byte == b'\t' || byte == b',' || byte == b';' || byte == b'='
}

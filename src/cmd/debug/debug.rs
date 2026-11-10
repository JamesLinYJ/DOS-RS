//! Command entry from `CMD/DEBUG/DEBUG.ASM`.
//!
//! A blank line asks for another command. Any other first character is
//! measured from 'A'. Below 'A' or past 'Z' is an error. The index is that
//! distance times two. The system-version build is off, so X runs the
//! expanded-memory command. A parity flag is cleared and no parity text is
//! shown. Quit increments its flag and selects the current process with
//! function 50h. The terminate vector is 22h through function 25h.

const COMMANDS: [&[u8]; 26] = [
    b"AS", b"ER", b"CM", b"DU", b"EN", b"FL", b"GO", b"HX", b"IN", b"ER", b"ER", b"LD", b"MV",
    b"NM", b"OU", b"ZT", b"QT", b"RG", b"SE", b"TR", b"UN", b"ER", b"WR", b"EM", b"ER", b"ER",
];
const SET_VECTOR: u16 = (37 << 8) | 0x22;
const CURRENT_PROCESS: u8 = 80;

fn skip(text: &[u8], index: &mut usize) {
    while *index < text.len() && (text[*index] == b' ' || text[*index] == b'\t') {
        *index += 1;
    }
}

fn command(text: &[u8]) -> &'static [u8] {
    let mut index = 0usize;
    skip(text, &mut index);
    if index >= text.len() || text[index] == b'\r' {
        return b"RP";
    }
    let letter = text[index];
    let relative = letter.wrapping_sub(b'A');
    if letter < b'A' || relative > b'Z' - b'A' {
        return b"ER";
    }
    COMMANDS[usize::from(relative)]
}

fn cleared_parity(flag: u8) -> u8 {
    if flag != 0 { 0 } else { flag }
}

fn quit(flag: u8) -> (u8, u8) {
    (flag.wrapping_add(1), CURRENT_PROCESS)
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBUG SV00");
    for text in [
        b"".as_slice(),
        b"A",
        b"B",
        b"C",
        b"D",
        b"E",
        b"F",
        b"G",
        b"H",
        b"I",
        b"J",
        b"K",
        b"L",
        b"M",
        b"N",
        b"O",
        b"P",
        b"Q",
        b"R",
        b"S",
        b"T",
        b"U",
        b"V",
        b"W",
        b"X",
        b"Y",
        b"Z",
        b"a",
        b"@",
        b"[",
        b"\tQ",
        b"   ",
    ] {
        crate::bios::mscon::con_write_bytes(b" ");
        crate::bios::mscon::con_write_bytes(command(text));
    }
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex8(cleared_parity(1));
    crate::bios::mscon::con_write_bytes(b" P");
    put_hex8(cleared_parity(0));
    let (flag, function) = quit(0);
    crate::bios::mscon::con_write_bytes(b" Q");
    put_hex8(flag);
    crate::bios::mscon::con_write_bytes(b" F");
    put_hex8(function);
    crate::bios::mscon::con_write_bytes(b" TV");
    put_hex16(SET_VECTOR);
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

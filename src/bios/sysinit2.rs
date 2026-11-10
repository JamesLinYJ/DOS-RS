//! `BADFIL` from `BIOS/SYSINIT2.ASM`.
//!
//! DS:DX points at an ASCIIZ name. The routine prints `BADLD_PRE`, then
//! each byte of that name with INT 21h AH=2, then `CRLFM` with AH=9.
//! `DoNotShowNum` is already 1 by the time `COMERR` runs, so the line
//! number is not printed.

use super::sysimes;

/// `DELIM`. `/` and NUL are delimiters, and so are space, tab, `=`, comma,
/// and semicolon.
/// `Organize` skips a comment that starts with `;` through the line feed.
pub fn skip_comment(text: &[u8]) -> usize {
    let mut index = 0usize;
    if text.first() != Some(&b';') {
        return 0;
    }
    while index < text.len() && text[index] != b'\n' {
        index += 1;
    }
    if index < text.len() {
        index + 1
    } else {
        index
    }
}

/// `ParseLine` for the device-parameter switches. A line with no `/` switch
/// that names a drive is an error. Letters are matched against `valid`.
pub fn parse_switches(line: &[u8], valid: &[u8]) -> Result<u16, ()> {
    let mut switches = 0u16;
    let mut index = 0usize;
    let mut saw_drive = false;
    while index < line.len() {
        let al = line[index];
        if al == b'\r' || al == b'\n' {
            break;
        }
        if al <= b' ' {
            index += 1;
            continue;
        }
        if al != b'/' {
            return Err(());
        }
        index += 1;
        if index >= line.len() {
            return Err(());
        }
        let letter = line[index].to_ascii_uppercase();
        if !letter.is_ascii_uppercase() {
            return Err(());
        }
        let Some(pos) = valid.iter().position(|byte| *byte == letter) else {
            return Err(());
        };
        switches |= 1u16 << pos;
        if letter == b'D' {
            saw_drive = true;
        }
        index += 1;
    }
    if !saw_drive {
        Err(())
    } else {
        Ok(switches)
    }
}

/// `MOVE_ASCIIZ` length, including the terminating NUL when it is present.
pub fn asciiz_len(text: &[u8]) -> usize {
    text.iter().position(|byte| *byte == 0).map(|index| index + 1).unwrap_or(text.len())
}

pub fn delim(al: u8) -> bool {
    al == b'/' || al == 0 || al == b' ' || al == b'\t' || al == b'=' || al == b',' || al == b';'
}

/// `GETNUM`. A decimal run is returned in AX. A non-digit at the start, or a
/// total that does not fit in 16 bits, is an error.
pub fn get_num(text: &[u8]) -> Result<u16, ()> {
    if text.is_empty() {
        return Err(());
    }
    let mut total: u16 = 0;
    let mut index = 0usize;
    loop {
        let al = text[index];
        let digit = to_digit(al)?;
        let product = u32::from(total) * 10 + u32::from(digit);
        if product > 0xffff {
            return Err(());
        }
        total = product as u16;
        index += 1;
        if index >= text.len() {
            break;
        }
        let next = text[index];
        if next == b'\n' || next == b'\r' || (delim(next) && next != b'=' && next != b';') {
            break;
        }
    }
    Ok(total)
}

fn to_digit(al: u8) -> Result<u8, ()> {
    if al < b'0' {
        return Err(());
    }
    let digit = al - b'0';
    if digit > 9 {
        Err(())
    } else {
        Ok(digit)
    }
}

pub fn badfil(name: &[u8]) {
    print_dollar(sysimes::BADLD_PRE);
    let mut index = 0;
    while index < name.len() {
        let byte = name[index];
        if byte == 0 {
            break;
        }
        con_out(byte);
        index += 1;
    }
    print_dollar(sysimes::CRLFM);
}

fn print_dollar(text: &[u8]) {
    dos_int(0x09, text.as_ptr() as u32);
}

fn con_out(byte: u8) {
    dos_int(0x02, u32::from(byte));
}

fn dos_int(ah: u8, dx: u32) {
    let mut ax = u32::from(ah) << 8;
    let mut edx = dx;
    #[cfg(target_arch = "x86")]
    unsafe {
        core::arch::asm!(
            "int 0x21",
            inout("eax") ax,
            inout("edx") edx,
            clobber_abi("C"),
        );
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "int 0x21",
            inout("eax") ax,
            inout("edx") edx,
            clobber_abi("C"),
        );
    }
    let _ = (ax, edx);
}

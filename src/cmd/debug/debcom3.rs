//! Trace and proceed from `CMD/DEBUG/DEBCOM3.ASM`.
//!
//! Lock and segment prefixes are skipped before the opcode is measured.
//! A near call is three bytes, a far call is five, and `INT 3` is one.
//! `REP` plus a string instruction is two bytes from the `REP`. An indirect
//! call uses the next byte's mode. With no count, trace runs once. `IN AL,21h`
//! and `IN AL,DX` when DX is 21h read the interrupt mask instead of tracing.

fn proceed(bytes: &[u8]) -> Option<u8> {
    let mut index = 0usize;
    while index < bytes.len() {
        let opcode = bytes[index];
        if opcode == 0xF0 || opcode == 0x26 || opcode == 0x2E || opcode == 0x36 || opcode == 0x3E {
            index += 1;
            continue;
        }
        let next = if index + 1 < bytes.len() { bytes[index + 1] } else { 0 };
        let length = instruction_length(opcode, next)?;
        return Some((index + length) as u8);
    }
    None
}

fn instruction_length(opcode: u8, next: u8) -> Option<usize> {
    if opcode == 0xE8 {
        return Some(3);
    }
    if opcode == 0x9A {
        return Some(5);
    }
    if opcode == 0xFF {
        return indirect_length(next);
    }
    if opcode == 0xCC {
        return Some(1);
    }
    if opcode == 0xCD || opcode == 0xE2 || opcode == 0xE1 || opcode == 0xE0 {
        return Some(2);
    }
    if opcode & 0xFE == 0xF2 {
        return string_length(next);
    }
    None
}

fn string_length(next: u8) -> Option<usize> {
    match next & 0xFE {
        0xA4 | 0xA6 | 0xAE | 0xAC | 0xAA => Some(2),
        _ => None,
    }
}

fn indirect_length(next: u8) -> Option<usize> {
    match next & 0xF8 {
        0x50 | 0x58 => Some(3),
        0x90 | 0x98 => Some(4),
        0xD0 => Some(2),
        _ => None,
    }
}

fn trace_count(present: bool, value: u16) -> u16 {
    if present { value } else { 1 }
}

fn mask_skip(opcode: u8, next: u8, dx_value: u16) -> Option<u8> {
    if opcode == 0xE4 && next == 0x21 {
        return Some(2);
    }
    if opcode == 0xEC && dx_value == 0x21 {
        return Some(1);
    }
    None
}

fn write_length(bytes: &[u8]) {
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(proceed(bytes).unwrap_or(0xFF));
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBCOM3");
    write_length(&[0xE8, 0, 0]);
    write_length(&[0x2E, 0xE8, 0, 0]);
    write_length(&[0x9A, 0, 0, 0, 0]);
    write_length(&[0xCC]);
    write_length(&[0xCD, 0x21]);
    write_length(&[0xE0, 0]);
    write_length(&[0xF3, 0xA4]);
    write_length(&[0xF3, 0x90]);
    write_length(&[0xFF, 0xD0]);
    write_length(&[0xFF, 0x50, 0]);
    write_length(&[0xFF, 0x90, 0, 0]);
    write_length(&[0x90]);
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex16(trace_count(false, 5));
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex16(trace_count(true, 5));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(mask_skip(0xE4, 0x21, 0).unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(mask_skip(0xEC, 0, 0x21).unwrap_or(0));
    crate::bios::mscon::con_write_bytes(b" MK");
    put_hex8(mask_skip(0xE4, 0x20, 0x21).unwrap_or(0));
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

//! Registers, flags, and file names from `CMD/DEBUG/DEBCOM2.ASM`.
//!
//! Flag names are stored most significant bit first. A set name turns its bit
//! on and a clear name turns it off. Naming the same bit twice is an error.
//! `F` opens the flag list. `HEX` and `EXE` are recognized extensions. Letters
//! from `a` through `z` are capitalized, and a blank, semicolon, equals sign,
//! tab, comma, or bracket separates a name.

const SET_NAMES: [&[u8]; 16] = [
    b"", b"", b"", b"", b"OV", b"DN", b"EI", b"", b"NG", b"ZR", b"", b"AC", b"", b"PE", b"", b"CY",
];
const CLEAR_NAMES: [&[u8]; 16] = [
    b"", b"", b"", b"", b"NV", b"UP", b"DI", b"", b"PL", b"NZ", b"", b"NA", b"", b"PO", b"", b"NC",
];
const REGISTERS: [&[u8]; 14] = [
    b"AX", b"BX", b"CX", b"DX", b"SP", b"BP", b"SI", b"DI", b"DS", b"ES", b"SS", b"CS", b"IP", b"PC",
];

fn flag_change(name: &[u8]) -> Option<(u16, bool)> {
    let mut index = 0usize;
    while index < SET_NAMES.len() {
        let mask = 1u16 << (15 - index);
        if !SET_NAMES[index].is_empty() && SET_NAMES[index] == name {
            return Some((mask, true));
        }
        if !CLEAR_NAMES[index].is_empty() && CLEAR_NAMES[index] == name {
            return Some((mask, false));
        }
        index += 1;
    }
    None
}

fn apply_flags(mut flags: u16, names: &[&[u8]]) -> Result<u16, ()> {
    let mut seen = 0u16;
    let mut index = 0usize;
    while index < names.len() {
        let Some((mask, turn_on)) = flag_change(names[index]) else {
            return Err(());
        };
        if seen & mask != 0 {
            return Err(());
        }
        seen |= mask;
        if turn_on {
            flags |= mask;
        } else {
            flags &= !mask;
        }
        index += 1;
    }
    Ok(flags)
}

fn flag_text(flags: u16) -> [u8; 24] {
    let mut out = [0u8; 24];
    let mut used = 0usize;
    let mut index = 0usize;
    while index < SET_NAMES.len() {
        let mask = 1u16 << (15 - index);
        let name = if flags & mask != 0 { SET_NAMES[index] } else { CLEAR_NAMES[index] };
        let mut letter = 0usize;
        while letter < name.len() && used < out.len() {
            out[used] = name[letter];
            used += 1;
            letter += 1;
        }
        if !name.is_empty() && used < out.len() {
            out[used] = b' ';
            used += 1;
        }
        index += 1;
    }
    out
}

fn flag_len(flags: u16) -> usize {
    let text = flag_text(flags);
    let mut used = 0usize;
    while used < text.len() && text[used] != 0 {
        used += 1;
    }
    used
}

fn register_index(name: &[u8]) -> Option<u8> {
    let mut index = 0usize;
    while index < REGISTERS.len() {
        if REGISTERS[index] == name {
            return Some(index as u8);
        }
        index += 1;
    }
    None
}

fn upper_letter(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() { byte.to_ascii_uppercase() } else { byte }
}

fn name_separator(byte: u8, switch: u8) -> bool {
    byte == b'['
        || byte == b' '
        || byte == b';'
        || byte == b'='
        || byte == b'\t'
        || byte == b','
        || byte == switch
        || byte == b'\r'
}

fn hex_extension(text: &[u8]) -> bool {
    text.len() >= 3 && text[0] == b'H' && text[1] == b'E' && text[2] == b'X'
}

fn exe_extension(text: &[u8]) -> bool {
    text.len() >= 3 && text[0] == b'E' && text[1] == b'X' && text[2] == b'E'
}

pub fn show() {
    let initial = 0xF202;
    let shown = flag_text(initial);
    crate::bios::mscon::con_write_bytes(b"DEBCOM2 FG");
    let mut index = 0usize;
    let length = flag_len(initial);
    while index < length {
        put_hex8(shown[index]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(apply_flags(initial, &[b"NV"]).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" OV");
    put_hex16(apply_flags(initial, &[b"OV"]).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" DP");
    put_hex16(apply_flags(initial, &[b"OV", b"OV"]).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" CY");
    put_hex16(apply_flags(initial, &[b"CY"]).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" NC");
    put_hex16(apply_flags(initial, &[b"NC"]).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" RG");
    put_hex8(register_index(b"AX").unwrap_or(0xFF));
    crate::bios::mscon::con_write_bytes(b" RG");
    put_hex8(register_index(b"IP").unwrap_or(0xFF));
    crate::bios::mscon::con_write_bytes(b" RG");
    put_hex8(register_index(b"PC").unwrap_or(0xFF));
    crate::bios::mscon::con_write_bytes(b" RG");
    put_hex8(register_index(b"ZZ").unwrap_or(0xFF));
    crate::bios::mscon::con_write_bytes(b" UA");
    put_hex8(upper_letter(b'a'));
    crate::bios::mscon::con_write_bytes(b" UZ");
    put_hex8(upper_letter(b'Z'));
    crate::bios::mscon::con_write_bytes(b" U7");
    put_hex8(upper_letter(b'{'));
    crate::bios::mscon::con_write_bytes(b" U6");
    put_hex8(upper_letter(b'`'));
    for byte in *b"[ A/\r" {
        crate::bios::mscon::con_write_bytes(b" DL");
        put_hex8(u8::from(name_separator(byte, b'/')));
    }
    crate::bios::mscon::con_write_bytes(b" HX");
    put_hex8(u8::from(hex_extension(b"HEX")));
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(u8::from(exe_extension(b"EXE")));
    crate::bios::mscon::con_write_bytes(b" CM");
    put_hex8(u8::from(hex_extension(b"COM") || exe_extension(b"COM")));
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

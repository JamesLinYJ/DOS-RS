//! Debugger commands from `CMD/DEBUG/DEBCOM1.ASM`.
//!
//! A range is an end address or `L` plus a length. The default length is 128
//! bytes, shortened so it does not pass the end of the segment. A length that
//! lands exactly on the segment boundary is kept. Hex digits accumulate by
//! shifting four bits. `DS`, `ES`, `SS`, and `CS` select a saved segment.
//! Move goes forward when the destination pair is below the source. Fill
//! repeats a short list. Search reports every match.

const DEFAULT_LENGTH: u16 = 128;
const ROW_MASK: u8 = 0x0F;

fn hex_digit(byte: u8) -> Option<u8> {
    let mut value = byte.wrapping_sub(b'0');
    if byte < b'0' {
        return None;
    }
    if value < 10 {
        return Some(value);
    }
    value &= 0x5F;
    value = value.wrapping_sub(7);
    if (10..16).contains(&value) { Some(value) } else { None }
}

fn hex_number(text: &[u8], max_digits: u16) -> Result<u32, ()> {
    let mut index = 0usize;
    let Some(first) = text.get(index).copied().and_then(hex_digit) else {
        return Err(());
    };
    let mut value = u32::from(first);
    let mut remaining = max_digits;
    index += 1;
    while let Some(digit) = text.get(index).copied().and_then(hex_digit) {
        remaining = remaining.wrapping_sub(1);
        if remaining == 0 {
            return Err(());
        }
        value = (value << 4) | u32::from(digit);
        index += 1;
    }
    Ok(value)
}

fn length_fits(offset: u16, length: u16) -> bool {
    let sum = u32::from(offset) + u32::from(length);
    let wrapped = offset.wrapping_add(length);
    if sum <= 0xFFFF {
        return true;
    }
    wrapped < 1
}

fn default_span(offset: u16) -> u16 {
    if offset == 0 {
        return DEFAULT_LENGTH;
    }
    let rest = offset.wrapping_neg();
    if rest >= DEFAULT_LENGTH { DEFAULT_LENGTH } else { rest }
}

fn inclusive_span(start: u16, end: u16) -> Result<u32, ()> {
    if end < start {
        return Err(());
    }
    Ok(u32::from(end) - u32::from(start) + 1)
}

fn display_byte(byte: u8) -> u8 {
    if !(b' '..0x7F).contains(&byte) { b'.' } else { byte }
}

fn row_mark(next_offset: u16) -> u8 {
    if next_offset as u8 & ROW_MASK == 0 {
        2
    } else if next_offset as u8 & 7 == 0 {
        1
    } else {
        0
    }
}

fn move_forward(dest_segment: u16, dest_offset: u16, source_segment: u16, source_offset: u16) -> bool {
    let borrow = dest_offset < source_offset;
    let (_, carried) = dest_segment.overflowing_sub(source_segment.wrapping_add(u16::from(borrow)));
    carried
}

fn moved(source: [u8; 4], source_offset: usize, dest_offset: usize, length: usize) -> [u8; 4] {
    let mut out = source;
    if move_forward(0, dest_offset as u16, 0, source_offset as u16) {
        let mut step = 0usize;
        while step < length {
            out[dest_offset + step] = source[source_offset + step];
            step += 1;
        }
    } else {
        let mut step = length;
        while step > 0 {
            step -= 1;
            out[dest_offset + step] = source[source_offset + step];
        }
    }
    out
}

fn fill(length: usize, pattern: &[u8]) -> [u8; 8] {
    let mut out = [0u8; 8];
    if pattern.len() >= length {
        let mut index = 0usize;
        while index < length {
            out[index] = pattern[index];
            index += 1;
        }
        return out;
    }
    let mut index = 0usize;
    while index < pattern.len() {
        out[index] = pattern[index];
        index += 1;
    }
    let mut dest = pattern.len();
    while dest < length {
        out[dest] = out[dest - pattern.len()];
        dest += 1;
    }
    out
}

fn search(haystack: &[u8], needle: &[u8]) -> [Option<u16>; 4] {
    let mut found = [None; 4];
    if needle.is_empty() || haystack.len() < needle.len() {
        return found;
    }
    let mut starts = haystack.len() - needle.len() + 1;
    let mut origin = 0usize;
    let mut used = 0usize;
    while starts != 0 {
        if haystack[origin] == needle[0] {
            let mut matched = true;
            let mut extra = 1usize;
            while extra < needle.len() {
                if haystack[origin + extra] != needle[extra] {
                    matched = false;
                    break;
                }
                extra += 1;
            }
            if matched && used < found.len() {
                found[used] = Some(origin as u16);
                used += 1;
            }
        }
        origin += 1;
        starts -= 1;
    }
    found
}

fn skip_separator(text: &[u8], index: &mut usize) {
    while *index < text.len() && (text[*index] == b' ' || text[*index] == b'\t') {
        *index += 1;
    }
    if *index < text.len() && text[*index] == b',' {
        *index += 1;
        while *index < text.len() && (text[*index] == b' ' || text[*index] == b'\t') {
            *index += 1;
        }
    }
}

fn byte_list(text: &[u8]) -> Result<([u8; 8], usize), ()> {
    let mut out = [0u8; 8];
    let mut used = 0usize;
    let mut index = 0usize;
    loop {
        skip_separator(text, &mut index);
        if index >= text.len() || text[index] == b'\r' {
            break;
        }
        let quote = text[index];
        if quote == b'\'' || quote == b'"' {
            index += 1;
            loop {
                if index >= text.len() || text[index] == b'\r' {
                    return Err(());
                }
                let byte = text[index];
                index += 1;
                if byte == quote {
                    if index < text.len() && text[index] == quote {
                        index += 1;
                        if used < out.len() {
                            out[used] = quote;
                            used += 1;
                        }
                    } else {
                        break;
                    }
                } else if used < out.len() {
                    out[used] = byte;
                    used += 1;
                }
            }
        } else {
            let mut digits = 0usize;
            while index + digits < text.len() && hex_digit(text[index + digits]).is_some() {
                digits += 1;
            }
            if digits == 0 || digits > 2 {
                return Err(());
            }
            let number = hex_number(&text[index..], 2)?;
            index += digits;
            if used < out.len() {
                out[used] = number as u8;
                used += 1;
            }
        }
    }
    if used == 0 { Err(()) } else { Ok((out, used)) }
}

struct Address {
    segment: u16,
    offset: u16,
}

fn segment_letter(letter: u8, saved: [u16; 4]) -> Option<u16> {
    let order = b"CSED";
    let mut remaining = 4u16;
    let mut index = 0usize;
    while index < order.len() {
        remaining = remaining.wrapping_sub(1);
        if order[index] == letter {
            return Some(saved[remaining as usize]);
        }
        index += 1;
    }
    None
}

fn address(text: &[u8], default_segment: u16, saved: [u16; 4]) -> Result<Address, ()> {
    if text.len() >= 2 && text[1] == b'S' {
        let segment = segment_letter(text[0], saved).ok_or(())?;
        if text.get(2) != Some(&b':') {
            return Err(());
        }
        let offset = hex_number(&text[3..], 4)? as u16;
        return Ok(Address { segment, offset });
    }
    let number = hex_number(text, 4)?;
    let mut digits = 0usize;
    while digits < text.len() && hex_digit(text[digits]).is_some() {
        digits += 1;
    }
    if text.get(digits) == Some(&b':') {
        let offset = hex_number(&text[digits + 1..], 4)? as u16;
        Ok(Address { segment: number as u16, offset })
    } else {
        Ok(Address { segment: default_segment, offset: number as u16 })
    }
}

fn write_bytes(bytes: &[u8], count: usize) {
    let mut index = 0usize;
    while index < count {
        put_hex8(bytes[index]);
        index += 1;
    }
}

pub fn show() {
    let saved = [0x1111, 0x2222, 0x3333, 0x4444];
    let forward = moved([1, 2, 3, 4], 1, 0, 3);
    let backward = moved([1, 2, 3, 4], 0, 1, 3);
    let long_fill = fill(4, &[0x41, 0x42, 0x43, 0x44, 0x45, 0x46]);
    let repeated = fill(6, &[0xAB, 0xCD]);
    let hits = search(&[1, 2, 3, 1, 2], &[1, 2]);
    let misses = search(&[1, 2, 3], &[9]);
    let (listed, listed_len) = byte_list(b"41 'A' \"B\"\r").unwrap_or(([0; 8], 0));
    let (quoted, quoted_len) = byte_list(b"\"A\"\"B\"\r").unwrap_or(([0; 8], 0));
    let plain = address(b"10", 0x1234, saved);
    let pair = address(b"2000:5", 0x1234, saved);
    let data = address(b"DS:2", 0x1234, saved);
    let extra = address(b"ES:3", 0x1234, saved);
    let stack = address(b"SS:4", 0x1234, saved);
    let code = address(b"CS:5", 0x1234, saved);
    let bad = address(b"XS:1", 0x1234, saved);
    crate::bios::mscon::con_write_bytes(b"DEBCOM1");
    for byte in *b"09AaG " {
        crate::bios::mscon::con_write_bytes(b" HX");
        put_hex8(hex_digit(byte).unwrap_or(0xFF));
    }
    crate::bios::mscon::con_write_bytes(b" N");
    match hex_number(b"1A", 4) {
        Ok(value) => put_hex32(value),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ERR"),
    }
    crate::bios::mscon::con_write_bytes(b" N");
    match hex_number(b"FFFF", 4) {
        Ok(value) => put_hex32(value),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ERR"),
    }
    crate::bios::mscon::con_write_bytes(b" N");
    match hex_number(b"10000", 4) {
        Ok(value) => put_hex32(value),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ERR"),
    }
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex16(default_span(0));
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex16(default_span(0xFFF0));
    crate::bios::mscon::con_write_bytes(b" L");
    if length_fits(0, 0x10) { put_hex16(0x10); } else { crate::bios::mscon::con_write_bytes(b"ERR"); }
    crate::bios::mscon::con_write_bytes(b" L");
    if length_fits(0xFFF8, 0x10) { put_hex16(0x10); } else { crate::bios::mscon::con_write_bytes(b"ERR"); }
    crate::bios::mscon::con_write_bytes(b" L");
    if length_fits(0xFFF8, 8) { put_hex16(8); } else { crate::bios::mscon::con_write_bytes(b"ERR"); }
    crate::bios::mscon::con_write_bytes(b" E");
    match inclusive_span(0, 0x000F) {
        Ok(value) => put_hex32(value),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ERR"),
    }
    crate::bios::mscon::con_write_bytes(b" E");
    match inclusive_span(0x10, 0) {
        Ok(value) => put_hex32(value),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ERR"),
    }
    crate::bios::mscon::con_write_bytes(b" E");
    match inclusive_span(0, 0xFFFF) {
        Ok(value) => put_hex32(value),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ERR"),
    }
    for byte in [b' ', b'A', 0x1F, 0x7E, 0x7F] {
        crate::bios::mscon::con_write_bytes(b" CH");
        put_hex8(display_byte(byte));
    }
    crate::bios::mscon::con_write_bytes(b" RW");
    put_hex8(row_mark(8));
    crate::bios::mscon::con_write_bytes(b" RW");
    put_hex8(row_mark(16));
    crate::bios::mscon::con_write_bytes(b" MV");
    write_bytes(&forward, 4);
    crate::bios::mscon::con_write_bytes(b" MV");
    write_bytes(&backward, 4);
    crate::bios::mscon::con_write_bytes(b" FL");
    write_bytes(&long_fill, 4);
    crate::bios::mscon::con_write_bytes(b" FL");
    write_bytes(&repeated, 6);
    write_hits(&hits);
    write_hits(&misses);
    crate::bios::mscon::con_write_bytes(b" LS");
    write_bytes(&listed, listed_len);
    crate::bios::mscon::con_write_bytes(b" LS");
    write_bytes(&quoted, quoted_len);
    crate::bios::mscon::con_write_bytes(b" LS");
    match byte_list(b"'\r") {
        Ok(_) => crate::bios::mscon::con_write_bytes(b"OK"),
        Err(()) => crate::bios::mscon::con_write_bytes(b"ER"),
    }
    write_address(plain);
    write_address(pair);
    write_address(data);
    write_address(extra);
    write_address(stack);
    write_address(code);
    write_address(bad);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_hits(hits: &[Option<u16>; 4]) {
    let mut index = 0usize;
    let mut any = false;
    while index < hits.len() {
        if let Some(offset) = hits[index] {
            crate::bios::mscon::con_write_bytes(b" SC");
            put_hex16(offset);
            any = true;
        }
        index += 1;
    }
    if !any {
        crate::bios::mscon::con_write_bytes(b" SC");
    }
}

fn write_address(parsed: Result<Address, ()>) {
    crate::bios::mscon::con_write_bytes(b" AD");
    match parsed {
        Ok(found) => {
            put_hex16(found.segment);
            put_hex16(found.offset);
        }
        Err(()) => crate::bios::mscon::con_write_bytes(b"ER"),
    }
}

fn put_hex32(value: u32) {
    put_hex16((value >> 16) as u16);
    put_hex16(value as u16);
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

//! Path canonicalization from `DOS/MACRO2.ASM`.
//!
//! `/` becomes `\`. A null is a separator for the scan, but it is not a path
//! character. Drive letters are one-based and `` `: `` is rejected. A prefix
//! matches only at a separator. `..` cannot back up past the root slash.
//! `TransPath` allows an exact splice. `TransPathSet` does not.

const FILE_NOT_FOUND: u8 = 2;
const PATH_NOT_FOUND: u8 = 3;
const LIMIT: usize = 48;

/// `PATHCHRCMP`. `/` becomes `\` and both set the separator flag.
pub fn path_char(byte: u8) -> (u8, bool) {
    if byte <= b'/' {
        if byte == b'/' {
            return (b'\\', true);
        }
        return (byte, false);
    }
    if byte == b'\\' {
        return (byte, true);
    }
    (byte, false)
}

/// `PathSep`. Null, `/`, and `\` are separators. `/` is rewritten to `\`.
pub fn path_sep(byte: u8) -> (u8, bool) {
    if byte == 0 {
        return (0, true);
    }
    path_char(byte)
}

/// `DriveFromText`. An empty string or a string without a colon is drive 0.
pub fn drive_from_text(text: &[u8]) -> (u8, usize) {
    if text.is_empty() || text[0] == 0 {
        return (0, 0);
    }
    if text.len() < 2 || text[1] != b':' {
        return (0, 0);
    }
    let letter = (text[0] | 0x20).wrapping_sub(b'a' - 1);
    if letter == 0 {
        return (0xff, 2);
    }
    (letter, 2)
}

/// `TextFromDrive`. Unit 0 is `A:`.
pub fn text_from_drive(unit: u8) -> [u8; 2] {
    text_from_drive_one(unit.wrapping_add(1))
}

/// `TextFromDrive1`. The unit is already one-based.
pub fn text_from_drive_one(unit: u8) -> [u8; 2] {
    [unit.wrapping_add(b'A' - 1), b':']
}

/// `PathPref`. The match has to end on a separator or be followed by one.
pub fn path_prefix(prefix: &[u8], text: &[u8]) -> bool {
    let prefix = until_nul(prefix);
    let mut index = 0usize;
    while index < prefix.len() {
        if index >= text.len() || text[index] == 0 || text[index] != prefix[index] {
            return false;
        }
        index += 1;
    }
    if prefix.is_empty() {
        return false;
    }
    if path_char(prefix[prefix.len() - 1]).1 {
        return true;
    }
    let next = if index < text.len() { text[index] } else { 0 };
    path_sep(next).1
}

/// `SkipBack`. Walking before `limit` is path not found.
pub fn skip_back(text: &[u8], mut cursor: usize, limit: usize) -> Result<usize, u8> {
    loop {
        if cursor < limit || cursor == 0 {
            return Err(PATH_NOT_FOUND);
        }
        cursor -= 1;
        if path_char(text[cursor]).1 {
            return Ok(cursor);
        }
    }
}

/// `ScanPathChar`. A separator sets the flag. A null clears it.
pub fn has_separator(text: &[u8]) -> bool {
    let mut index = 0usize;
    loop {
        let byte = if index < text.len() { text[index] } else { 0 };
        index += 1;
        if path_sep(byte).1 {
            return path_char(byte).1;
        }
    }
}

/// `NoSetDir`. Exact splice is allowed only for `TransPath`.
pub fn no_set_dir(allow_exact: bool) -> u8 {
    if allow_exact { 0 } else { 0xff }
}

/// `Canonicalize`. Leading separators are kept, `.` is dropped, and `..`
/// backs up to the previous separator.
pub fn canonicalize(source: &[u8]) -> Result<[u8; LIMIT], u8> {
    let mut out = [0u8; LIMIT];
    let mut dest = 0usize;
    let mut src = 0usize;
    loop {
        let byte = source.get(src).copied().unwrap_or(0);
        let (norm, is_path) = path_char(byte);
        if !is_path {
            break;
        }
        src += 1;
        if dest >= LIMIT {
            return Err(rest_error(&source[src..]));
        }
        out[dest] = norm;
        dest += 1;
    }
    loop {
        let byte = source.get(src).copied().unwrap_or(0);
        if byte == 0 {
            if dest > 0 && out[dest - 1] == b':' {
                if dest >= LIMIT {
                    return Err(FILE_NOT_FOUND);
                }
                out[dest] = b'\\';
                dest += 1;
            }
            if dest >= LIMIT {
                return Err(FILE_NOT_FOUND);
            }
            out[dest] = 0;
            return Ok(out);
        }
        let (part, used) = take_component(&source[src..])?;
        src += used;
        match part {
            Part::Dot | Part::DotDot => {
                if dest == 0 {
                    return Err(PATH_NOT_FOUND);
                }
                dest -= 1;
                dest = skip_back(&out, dest, 0)?;
            }
            Part::Name(name, length) => {
                if dest + length >= LIMIT {
                    return Err(rest_error(&source[src..]));
                }
                out[dest..dest + length].copy_from_slice(&name[..length]);
                dest += length;
            }
        }
        let mark = source.get(src).copied().unwrap_or(0);
        if !path_sep(mark).1 {
            return Err(rest_error(&source[src..]));
        }
        let (norm, is_path) = path_char(mark);
        if !is_path {
            continue;
        }
        src += 1;
        if dest >= LIMIT {
            return Err(rest_error(&source[src..]));
        }
        out[dest] = norm;
        dest += 1;
        loop {
            let extra = source.get(src).copied().unwrap_or(0);
            if !path_char(extra).1 {
                break;
            }
            src += 1;
        }
    }
}

/// Lead-in classification from `TransPath`. Two separators are a UNC name.
pub fn classify(text: &[u8], device: bool) -> Result<Kind, u8> {
    let (_drive, start) = drive_from_text(text);
    let rest = if start < text.len() { &text[start..] } else { &[] };
    if rest.len() >= 2 {
        let (first, first_path) = path_char(rest[0]);
        let (second, second_path) = path_char(rest[1]);
        if second_path && first_path && first == second {
            return Ok(Kind::Unc);
        }
    }
    if rest.is_empty() || rest[0] == 0 {
        return Err(FILE_NOT_FOUND);
    }
    if device {
        return Ok(Kind::Device);
    }
    Ok(Kind::File)
}

pub enum Kind {
    Unc,
    Device,
    File,
}

enum Part {
    Dot,
    DotDot,
    Name([u8; 12], usize),
}

fn take_component(source: &[u8]) -> Result<(Part, usize), u8> {
    let first = source.first().copied().unwrap_or(0);
    if first == b'.' {
        let next = source.get(1).copied().unwrap_or(0);
        if path_sep(next).1 {
            return Ok((Part::Dot, 1));
        }
        if next == b'.' {
            let after = source.get(2).copied().unwrap_or(0);
            if path_sep(after).1 {
                return Ok((Part::DotDot, 2));
            }
        }
        return Err(rest_error(source));
    }
    let mut name = [0u8; 12];
    let mut length = 0usize;
    let mut base = 0usize;
    let mut extension = 0usize;
    let mut dotted = false;
    let mut meta = false;
    let mut index = 0usize;
    while index < source.len() && source[index] != 0 {
        let (norm, is_path) = path_char(source[index]);
        if is_path {
            break;
        }
        let upper = up(norm);
        if upper == b'*' || upper == b'?' {
            meta = true;
        }
        if upper == b'.' && !dotted {
            dotted = true;
            if length < name.len() {
                name[length] = b'.';
                length += 1;
            }
        } else if !dotted && base < 8 {
            name[length] = upper;
            length += 1;
            base += 1;
        } else if dotted && extension < 3 && upper != b'.' {
            name[length] = upper;
            length += 1;
            extension += 1;
        }
        index += 1;
    }
    if meta || length == 0 {
        return Err(rest_error(&source[index..]));
    }
    Ok((Part::Name(name, length), index))
}

fn rest_error(rest: &[u8]) -> u8 {
    if has_separator(rest) {
        PATH_NOT_FOUND
    } else {
        FILE_NOT_FOUND
    }
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - (b'a' - b'A')
    } else {
        byte
    }
}

fn until_nul(text: &[u8]) -> &[u8] {
    let mut end = 0usize;
    while end < text.len() && text[end] != 0 {
        end += 1;
    }
    &text[..end]
}

pub fn show() {
    let (slash, slash_sep) = path_sep(b'/');
    let (_nul, nul_sep) = path_sep(0);
    let (_plain, nul_char) = path_char(0);
    let (bee, _) = drive_from_text(b"B:Z");
    let (plain, _) = drive_from_text(b"FOO");
    let (tick, _) = drive_from_text(b"`:Q");
    let letters = text_from_drive(0);
    let rooted = path_prefix(b"A:\\\0", b"A:\\FOO\0");
    let glued = path_prefix(b"A:\\FOO\0", b"A:\\FOOBAR\0");
    let nested = path_prefix(b"A:\\FOO\0", b"A:\\FOO\\X\0");
    let canon = canonicalize(b"\\foo\\..\\bar");
    let above = canonicalize(b"\\..");
    let meta = canonicalize(b"foo*");
    let split = has_separator(b"a\\b");
    let whole = has_separator(b"abc");
    crate::bios::mscon::con_write_bytes(b"MACRO2 SL");
    put_hex8(if slash_sep { slash } else { 0 });
    crate::bios::mscon::con_write_bytes(if nul_sep { b" NU1" } else { b" NU0" });
    crate::bios::mscon::con_write_bytes(if nul_char { b" CH1" } else { b" CH0" });
    crate::bios::mscon::con_write_bytes(b" DRV");
    put_hex8(bee);
    crate::bios::mscon::con_write_bytes(b" DRV");
    put_hex8(plain);
    crate::bios::mscon::con_write_bytes(b" DRV");
    put_hex8(tick);
    crate::bios::mscon::con_write_bytes(b" TX");
    put_hex8(letters[0]);
    put_hex8(letters[1]);
    crate::bios::mscon::con_write_bytes(if rooted { b" P1" } else { b" P0" });
    crate::bios::mscon::con_write_bytes(if glued { b" G1" } else { b" G0" });
    crate::bios::mscon::con_write_bytes(if nested { b" N1" } else { b" N0" });
    crate::bios::mscon::con_write_bytes(b" CN");
    match canon {
        Ok(text) => put_path(&text),
        Err(error) => put_hex8(error),
    }
    crate::bios::mscon::con_write_bytes(b" UP");
    put_hex8(match above {
        Ok(_) => 0,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(b" MF");
    put_hex8(match meta {
        Ok(_) => 0,
        Err(error) => error,
    });
    crate::bios::mscon::con_write_bytes(if split { b" SC1" } else { b" SC0" });
    crate::bios::mscon::con_write_bytes(if whole { b" SC1" } else { b" SC0" });
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(no_set_dir(true));
    crate::bios::mscon::con_write_bytes(b" NS");
    put_hex8(no_set_dir(false));
    crate::bios::mscon::con_write_bytes(match classify(b"\\\\x", false) {
        Ok(Kind::Unc) => b" UNC",
        _ => b" BAD",
    });
    crate::bios::mscon::con_write_bytes(match classify(b"CON", true) {
        Ok(Kind::Device) => b" DEV",
        _ => b" BAD",
    });
    crate::bios::mscon::con_write_bytes(match classify(b"FOO", false) {
        Ok(Kind::File) => b" FIL",
        _ => b" BAD",
    });
    crate::bios::mscon::con_write_bytes(b" EM");
    put_hex8(match classify(b"A:", false) {
        Err(error) => error,
        Ok(_) => 0,
    });
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_path(text: &[u8]) {
    let mut index = 0usize;
    while index < text.len() && text[index] != 0 {
        if text[index] == b'\\' {
            crate::bios::mscon::con_write_bytes(b"5C");
        } else {
            crate::bios::mscon::con_write_bytes(&text[index..index + 1]);
        }
        index += 1;
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

//! `PFMT` from `DOS/DOSPRINT.ASM`.
//!
//! `$x` prints four hex digits, `$c` prints one character, and `$s` prints
//! a zero-terminated string. `$S` takes a segment word and then that string.
//! `\n` is carriage return then line feed. `\t` is a tab. `$p` prints the
//! user id, a colon, and the process id.

pub enum Piece<'a> {
    Hex(u16),
    Char(u8),
    Text(&'a [u8]),
    Segment(u16),
}

/// Format `template` into `out`. Returns the byte count and the last `$S` segment.
pub fn format(
    template: &[u8],
    pieces: &[Piece],
    user: u16,
    process: u16,
    out: &mut [u8],
) -> (usize, u16) {
    let mut source = 0usize;
    let mut piece = 0usize;
    let mut written = 0usize;
    let mut selector = 0u16;
    while source < template.len() && written < out.len() {
        let byte = template[source];
        source += 1;
        if byte == 0 {
            break;
        }
        if byte == b'\\' {
            if source >= template.len() {
                break;
            }
            let next = template[source];
            source += 1;
            if next == 0 {
                break;
            }
            if next == b'n' {
                written = push(out, written, 13);
                written = push(out, written, 10);
                continue;
            }
            if next == b't' {
                written = push(out, written, 9);
                continue;
            }
            written = push(out, written, next);
            continue;
        }
        if byte == b'$' {
            if source >= template.len() {
                break;
            }
            let next = template[source];
            source += 1;
            match next {
                b'x' => {
                    if let Some(Piece::Hex(value)) = pieces.get(piece) {
                        written = hex4(out, written, *value);
                        piece += 1;
                    }
                }
                b'c' => {
                    if let Some(Piece::Char(value)) = pieces.get(piece) {
                        written = push(out, written, *value);
                        piece += 1;
                    }
                }
                b's' => {
                    if let Some(Piece::Text(text)) = pieces.get(piece) {
                        written = text_out(out, written, text);
                        piece += 1;
                    }
                }
                b'S' => {
                    if let Some(Piece::Segment(segment)) = pieces.get(piece) {
                        selector = *segment;
                        piece += 1;
                        if let Some(Piece::Text(text)) = pieces.get(piece) {
                            written = text_out(out, written, text);
                            piece += 1;
                        }
                    }
                }
                b'p' => {
                    written = hex4(out, written, user);
                    written = push(out, written, b':');
                    written = hex4(out, written, process);
                }
                _ => written = push(out, written, next),
            }
            continue;
        }
        written = push(out, written, byte);
    }
    (written, selector)
}

fn text_out(out: &mut [u8], mut written: usize, text: &[u8]) -> usize {
    let mut index = 0usize;
    while index < text.len() && text[index] != 0 && written < out.len() {
        written = push(out, written, text[index]);
        index += 1;
    }
    written
}

fn push(out: &mut [u8], written: usize, byte: u8) -> usize {
    if written < out.len() {
        out[written] = byte;
        written + 1
    } else {
        written
    }
}

fn hex4(out: &mut [u8], mut written: usize, value: u16) -> usize {
    let digits = b"0123456789ABCDEF";
    let mut shift = 12i32;
    while shift >= 0 {
        let nibble = ((value >> shift) & 0x0f) as usize;
        written = push(out, written, digits[nibble]);
        shift -= 4;
    }
    written
}

pub fn show() {
    let mut raw = [0u8; 24];
    let (count, _) = format(
        b"X=$x\\n$c$s",
        &[Piece::Hex(0x0a1b), Piece::Char(b'A'), Piece::Text(b"OK")],
        1,
        2,
        &mut raw,
    );
    crate::bios::mscon::con_write_bytes(b"DOSPRINT ");
    let mut index = 0usize;
    while index < count {
        if raw[index] == 13 && index + 1 < count && raw[index + 1] == 10 {
            crate::bios::mscon::con_write_bytes(b"CRLF");
            index += 2;
            continue;
        }
        crate::bios::mscon::con_write_bytes(&raw[index..index + 1]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

//! Command `SYSPARSE` from `CMD/COMMAND/TPARSE.ASM`.
//!
//! Complex lists, keywords, value list 2, quotes, and line feed as an end
//! of line are off. Both command and append entry points call this parser.
//! A slash ends a token only after the first character. `=` makes a keyword.
//! An empty token increments the ordinal before the optional check. A repeated
//! control does not increment it.

const EOL: u16 = 0xFFFF;
const TOO_MANY: u16 = 1;
const MISSING: u16 = 2;
const NOT_SWITCH: u16 = 3;
const NOT_KEYWORD: u16 = 4;

struct Rules {
    ordinal: u16,
    minimum: u16,
    maximum: u16,
    switches: u16,
    keywords: u16,
    optional: bool,
    repeat: bool,
}

struct Parsed {
    code: u16,
    ordinal: u16,
    token: [u8; 16],
    token_len: usize,
    rest: usize,
}

fn end_of_line(byte: u8) -> bool {
    byte == b'\r' || byte == 0
}

fn delimiter(byte: u8, next: u8) -> u8 {
    if byte == b' ' || byte == b'\t' {
        return 1;
    }
    if byte == b',' {
        return 2;
    }
    if byte == 0x81 && next == 0x40 {
        return 3;
    }
    0
}

fn parse(text: &[u8], rules: Rules) -> Parsed {
    let mut index = 0usize;
    let mut ordinal = rules.ordinal;
    loop {
        let byte = if index < text.len() { text[index] } else { 0 };
        let next = if index + 1 < text.len() { text[index + 1] } else { 0 };
        index += 1;
        if end_of_line(byte) {
            let code = if ordinal >= rules.minimum { EOL } else { MISSING };
            return Parsed { code, ordinal, token: [0; 16], token_len: 0, rest: index - 1 };
        }
        match delimiter(byte, next) {
            0 => {
                index -= 1;
                break;
            }
            3 => index += 1,
            1 => {}
            _ => break,
        }
    }
    let mut token = [0u8; 16];
    let mut token_len = 0usize;
    let mut equals = false;
    loop {
        let byte = if index < text.len() { text[index] } else { 0 };
        let next = if index + 1 < text.len() { text[index + 1] } else { 0 };
        index += 1;
        if token_len != 0 && byte == b'/' {
            index -= 1;
            break;
        }
        if end_of_line(byte) {
            index -= 1;
            break;
        }
        match delimiter(byte, next) {
            0 => {}
            3 => {
                index += 1;
                skip_delimiters(text, &mut index);
                break;
            }
            2 => {
                if equals {
                    index -= 1;
                }
                break;
            }
            _ => {
                skip_delimiters(text, &mut index);
                break;
            }
        }
        if token_len < token.len() {
            token[token_len] = byte;
            token_len += 1;
        }
        if byte == b'=' {
            equals = true;
        }
    }
    if token_len != 0 && token[0] == b'/' && rules.switches == 0 {
        return Parsed { code: NOT_SWITCH, ordinal, token, token_len, rest: index };
    } else if equals && rules.keywords == 0 {
        return Parsed { code: NOT_KEYWORD, ordinal, token, token_len, rest: index };
    } else if token_len == 0 || token[0] != b'/' {
        if !equals && ordinal >= rules.maximum {
            return Parsed { code: TOO_MANY, ordinal, token, token_len, rest: index };
        }
        if !equals && !rules.repeat {
            ordinal = ordinal.wrapping_add(1);
        }
        if !equals && token_len == 0 && !rules.optional {
            return Parsed { code: MISSING, ordinal, token, token_len, rest: index };
        }
    }
    Parsed { code: 0, ordinal, token, token_len, rest: index }
}

fn skip_delimiters(text: &[u8], index: &mut usize) {
    loop {
        let byte = if *index < text.len() { text[*index] } else { 0 };
        let next = if *index + 1 < text.len() { text[*index + 1] } else { 0 };
        *index += 1;
        if end_of_line(byte) {
            *index -= 1;
            return;
        }
        match delimiter(byte, next) {
            0 => {
                *index -= 1;
                return;
            }
            3 => *index += 1,
            1 => {}
            _ => return,
        }
    }
}

fn rules(
    ordinal: u16,
    minimum: u16,
    maximum: u16,
    optional: bool,
    repeat: bool,
) -> Rules {
    Rules { ordinal, minimum, maximum, switches: 0, keywords: 0, optional, repeat }
}

fn write_parsed(text: &[u8], parsed: &Parsed) {
    crate::bios::mscon::con_write_bytes(b" PC");
    put_hex16(parsed.code);
    crate::bios::mscon::con_write_bytes(b" OR");
    put_hex16(parsed.ordinal);
    crate::bios::mscon::con_write_bytes(b" TK");
    let mut index = 0usize;
    while index < parsed.token_len {
        put_hex8(parsed.token[index]);
        index += 1;
    }
    write_rest(text, parsed.rest);
}

fn write_rest(text: &[u8], rest: usize) {
    crate::bios::mscon::con_write_bytes(b" RS");
    let mut index = rest;
    while index < text.len() {
        put_hex8(text[index]);
        index += 1;
    }
}

pub fn show() {
    let dbcs = [b'A', 0x81, 0x40, b'B', b'\r'];
    crate::bios::mscon::con_write_bytes(b"TPARSE CX00 KY00 V200 QU00 LF00");
    let early = parse(b"\r", rules(0, 1, 1, false, false));
    crate::bios::mscon::con_write_bytes(b" EL");
    put_hex16(early.code);
    crate::bios::mscon::con_write_bytes(b" OR");
    put_hex16(early.ordinal);
    write_rest(b"\r", early.rest);
    let done = parse(b"\r", rules(1, 1, 1, false, false));
    crate::bios::mscon::con_write_bytes(b" EL");
    put_hex16(done.code);
    crate::bios::mscon::con_write_bytes(b" OR");
    put_hex16(done.ordinal);
    write_rest(b"\r", done.rest);
    write_parsed(b"FOO BAR\r", &parse(b"FOO BAR\r", rules(0, 0, 2, false, false)));
    write_parsed(b"/X\r", &parse(b"/X\r", rules(0, 0, 1, false, false)));
    write_parsed(b"A=B\r", &parse(b"A=B\r", rules(0, 0, 1, false, false)));
    write_parsed(b"A =B\r", &parse(b"A =B\r", rules(0, 0, 2, false, false)));
    write_parsed(b"A\nB\r", &parse(b"A\nB\r", rules(0, 0, 1, false, false)));
    write_parsed(b"12/3\r", &parse(b"12/3\r", rules(0, 0, 1, false, false)));
    write_parsed(&dbcs, &parse(&dbcs, rules(0, 0, 1, false, false)));
    write_parsed(b"\"FOO BAR\"\r", &parse(b"\"FOO BAR\"\r", rules(0, 0, 2, false, false)));
    write_parsed(b", FOO\r", &parse(b", FOO\r", rules(0, 0, 2, false, false)));
    write_parsed(b", FOO\r", &parse(b", FOO\r", rules(0, 0, 2, true, false)));
    write_parsed(b"FOO\r", &parse(b"FOO\r", rules(0, 0, 1, false, true)));
    write_parsed(b"FOO\r", &parse(b"FOO\r", rules(1, 0, 1, false, false)));
    write_parsed(b"\n\r", &parse(b"\n\r", rules(0, 1, 1, false, false)));
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

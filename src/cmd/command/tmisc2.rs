//! Path scan and redirection from `CMD/COMMAND/TMISC2.ASM`.
//!
//! A switch is taken before the path is classified. `?` and `*` mark the
//! path wild, and a slash counts only when it is not the switch character.
//! A lead byte hides the byte after it. Pipes and `IF` skip redirection.
//! Append seeks to the last byte and steps back over an end-of-file mark.
//! An empty file is rewound. Access denied does not fall through to create.

const SWITCHES: &[u8] = b"VBAPW";
const MARKED: u16 = 0x8000;
const WILD: u8 = 2;
const NETWORK: u16 = 65;
const DENIED: u16 = 5;
const MARK: u8 = 0x1A;

fn is_delim(byte: u8) -> bool {
    byte == b' ' || byte == b'=' || byte == b',' || byte == b';' || byte == b'\t' || byte == 0x0A
}

fn switch_bit(letter: u8) -> Option<u16> {
    let folded = if letter.is_ascii_lowercase() { letter - 0x20 } else { letter };
    let mut remaining = SWITCHES.len() as u16;
    let mut index = 0usize;
    while index < SWITCHES.len() {
        remaining -= 1;
        if SWITCHES[index] == folded {
            return Some(1u16 << remaining);
        }
        index += 1;
    }
    None
}

fn path_char(switch_char: u8, byte: u8) -> bool {
    if switch_char != b'/' && byte == b'/' {
        return true;
    }
    byte == b'\\'
}

fn take_switches(text: &[u8], switch_char: u8) -> (u16, usize) {
    let mut index = 0usize;
    let mut bits = 0u16;
    loop {
        while index < text.len() && is_delim(text[index]) {
            index += 1;
        }
        if index >= text.len() || text[index] != switch_char {
            break;
        }
        bits |= MARKED;
        index += 1;
        while index < text.len() && is_delim(text[index]) {
            index += 1;
        }
        if index >= text.len() || text[index] == b'\r' {
            break;
        }
        let letter = text[index];
        index += 1;
        if let Some(bit) = switch_bit(letter) {
            bits |= bit;
        }
    }
    (bits, index)
}

struct PathScan {
    switches: u16,
    directories: u8,
    wild: u8,
    stop: u8,
    rest: [u8; 12],
    rest_len: usize,
}

fn scan_path(text: &[u8], switch_char: u8, lead: Option<(u8, u8)>) -> PathScan {
    let (switches, mut index) = take_switches(text, switch_char);
    let mut left = text.len().saturating_sub(index);
    let mut directories = 0u8;
    let mut wild = 0u8;
    while left != 0 {
        left -= 1;
        let byte = text[index];
        index += 1;
        if let Some((low, high)) = lead
            && byte >= low
            && byte <= high
        {
            if left != 0 {
                left -= 1;
                index += 1;
            }
            continue;
        }
        if path_char(switch_char, byte) {
            directories = directories.wrapping_add(1);
        }
        if byte == b'?' || byte == b'*' {
            wild |= WILD;
        }
        if is_delim(byte) || byte == switch_char {
            let stop = if is_delim(byte) { 1 } else { 2 };
            let rest_from = index;
            return finish(switches, directories, wild, stop, text, rest_from);
        }
    }
    finish(switches, directories, wild, 0, text, text.len())
}

fn finish(switches: u16, directories: u8, wild: u8, stop: u8, text: &[u8], from: usize) -> PathScan {
    let mut rest = [0u8; 12];
    let mut rest_len = 0usize;
    let mut index = from;
    while index < text.len() && rest_len < rest.len() {
        rest[rest_len] = text[index];
        rest_len += 1;
        index += 1;
    }
    PathScan { switches, directories, wild, stop, rest, rest_len }
}

fn argument_present(length: u8, text: &[u8]) -> bool {
    if length == 0 {
        return false;
    }
    let mut index = 0usize;
    loop {
        let byte = if index < text.len() { text[index] } else { b'\r' };
        index += 1;
        if !is_delim(byte) || byte == b';' {
            return byte != b'\r';
        }
        if index > text.len() {
            return false;
        }
    }
}

fn text_length(text: &[u8]) -> u16 {
    let mut count = 0u16;
    let mut index = 0usize;
    loop {
        let byte = if index < text.len() { text[index] } else { 0 };
        index += 1;
        count = count.wrapping_add(1);
        if byte == 0 {
            return count;
        }
    }
}

fn to_source(text: &[u8]) -> (u16, [u8; 16]) {
    let mut stored = [0u8; 16];
    let mut count = 0u16;
    let mut index = 0usize;
    loop {
        let byte = if index < text.len() { text[index] } else { 0 };
        index += 1;
        if byte == 0 {
            break;
        }
        stored[count as usize] = byte;
        count = count.wrapping_add(1);
    }
    stored[count as usize] = b'\r';
    (count, stored)
}

fn input_redirect(pipe: bool, conditional: bool, named: bool, error: Option<u16>) -> u8 {
    if pipe || conditional || !named {
        return 0;
    }
    match error {
        None => 1,
        Some(NETWORK) => 0x41,
        Some(code) => code as u8,
    }
}

struct Output {
    pipe: bool,
    conditional: bool,
    named: bool,
    append: bool,
    device: bool,
    open_error: Option<u16>,
    read_count: u16,
    read_byte: u8,
    create_error: Option<u16>,
}

fn output_redirect(request: &Output) -> u8 {
    if request.pipe || request.conditional || !request.named {
        return 0;
    }
    if request.append {
        if let Some(code) = request.open_error {
            if code == DENIED {
                return 0x05;
            }
            if code == NETWORK {
                return 0x41;
            }
            return match request.create_error {
                None => 0x06,
                Some(NETWORK) => 0x41,
                Some(_) => 0x09,
            };
        }
        if request.device {
            return 1;
        }
        if request.read_count != 1 {
            return 4;
        }
        return if request.read_byte == MARK { 2 } else { 3 };
    }
    match request.create_error {
        None => 0x08,
        Some(NETWORK) => 0x41,
        Some(_) => 0x09,
    }
}

fn triage(failed: bool, extended: u16, original: u16) -> u16 {
    if !failed {
        return 0;
    }
    if extended == NETWORK { extended } else { original }
}

fn write_scan(scanned: &PathScan) {
    crate::bios::mscon::con_write_bytes(b" SW");
    put_hex16(scanned.switches);
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(scanned.directories);
    crate::bios::mscon::con_write_bytes(b" IN");
    put_hex8(scanned.wild);
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex8(scanned.stop);
    if scanned.stop != 0 {
        crate::bios::mscon::con_write_bytes(b" RS");
        let mut index = 0usize;
        while index < scanned.rest_len {
            put_hex8(scanned.rest[index]);
            index += 1;
        }
    }
}

fn write_move(label: &[u8], text: &[u8]) {
    let (count, stored) = to_source(text);
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < count as usize + 1 {
        put_hex8(stored[index]);
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex16(count);
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"TMISC2");
    write_scan(&scan_path(b"/V FOO\\*.*", b'/', None));
    write_scan(&scan_path(b"FOO;BAR", b'/', None));
    write_scan(&scan_path(b"A B", b'/', None));
    write_scan(&scan_path(b"FOO*.TXT", b'/', None));
    write_scan(&scan_path(b"A\\B?", b'/', None));
    write_scan(&scan_path(&[0x81, b'\\', b'A'], b'/', None));
    write_scan(&scan_path(b"FOO/BAR", b'/', None));
    write_scan(&scan_path(b"/v/a FILE", b'/', None));
    write_scan(&scan_path(&[0x81, b'\\', b'A'], b'/', Some((0x81, 0x9F))));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex8(u8::from(!argument_present(0, b"")));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex8(u8::from(!argument_present(4, b"   \r")));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex8(u8::from(!argument_present(2, b" ;")));
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex8(u8::from(!argument_present(4, b" FOO")));
    crate::bios::mscon::con_write_bytes(b" SL");
    put_hex8(text_length(b"AB\0") as u8);
    crate::bios::mscon::con_write_bytes(b" SL");
    put_hex8(text_length(b"\0") as u8);
    write_move(b" MV", b"AB\0");
    write_move(b" MV", b"\0");
    let inputs = [
        (true, false, false, None),
        (false, true, true, Some(2u16)),
        (false, false, false, None),
        (false, false, true, Some(65)),
        (false, false, true, Some(2)),
        (false, false, true, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
        (false, false, false, None),
    ];
    let outputs = [
        Output { pipe: true, conditional: false, named: false, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: true, named: true, append: true, device: false, open_error: None, read_count: 1, read_byte: b'A', create_error: None },
        Output { pipe: false, conditional: false, named: false, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: false, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: false, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: false, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: true, open_error: None, read_count: 1, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: None, read_count: 1, read_byte: 0x1A, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: None, read_count: 1, read_byte: b'A', create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: Some(5), read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: Some(65), read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: Some(2), read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: true, device: false, open_error: Some(2), read_count: 0, read_byte: 0, create_error: Some(3) },
        Output { pipe: false, conditional: false, named: true, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: None },
        Output { pipe: false, conditional: false, named: true, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: Some(2) },
        Output { pipe: false, conditional: false, named: true, append: false, device: false, open_error: None, read_count: 0, read_byte: 0, create_error: Some(65) },
    ];
    let mut pair = 0usize;
    while pair < outputs.len() {
        let (pipe, conditional, named, error) = inputs[pair];
        crate::bios::mscon::con_write_bytes(b" IN");
        put_hex8(input_redirect(pipe, conditional, named, error));
        crate::bios::mscon::con_write_bytes(b" OU");
        put_hex8(output_redirect(&outputs[pair]));
        pair += 1;
    }
    crate::bios::mscon::con_write_bytes(b" TC");
    put_hex16(triage(false, 65, 2));
    crate::bios::mscon::con_write_bytes(b" TC");
    put_hex16(triage(true, 65, 2));
    crate::bios::mscon::con_write_bytes(b" TC");
    put_hex16(triage(true, 2, 2));
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

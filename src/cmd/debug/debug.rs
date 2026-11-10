//! Command entry and file loading from `CMD/DEBUG/DEBUG.ASM`.
//!
//! A blank line asks for another command. Any other first character is
//! measured from 'A'. Below 'A' or past 'Z' is an error. The index is that
//! distance times two. The system-version build is off, so X runs the
//! expanded-memory command. A parity flag is cleared and no parity text is
//! shown. Quit increments its flag and selects the current process with
//! function 50h. The terminate vector is 22h through function 25h.
//!
//! A failed message load exits with function 4Ch. The program end is the
//! code paragraph plus the rounded data length plus the distance from the
//! original segment. Forty columns shrink the dump row. An Asian blank
//! outside quotes becomes two spaces. A named file sets the low bit of the
//! name flag. Aborting our own program resets the disk.

const COMMANDS: [&[u8]; 26] = [
    b"AS", b"ER", b"CM", b"DU", b"EN", b"FL", b"GO", b"HX", b"IN", b"ER", b"ER", b"LD", b"MV",
    b"NM", b"OU", b"ZT", b"QT", b"RG", b"SE", b"TR", b"UN", b"ER", b"WR", b"EM", b"ER", b"ER",
];
const SET_VECTOR: u16 = (37 << 8) | 0x22;
const CURRENT_PROCESS: u8 = 80;
const DATA_END: u16 = 0x067B;
const LEAD_BYTES: &[u8] = &[0x81, 0x9F, 0xE0, 0xFC, 0x00];

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

fn program_end(code: u16, origin: u16) -> u16 {
    let paragraphs = DATA_END.wrapping_add(15) >> 4;
    code.wrapping_add(paragraphs).wrapping_add(code.wrapping_sub(origin))
}

fn stack_frame(remaining: u16) -> (u16, u16, u16) {
    let grown = remaining.wrapping_add(0x100);
    let saved = grown.wrapping_sub(2);
    let reported = saved.wrapping_sub(0x100);
    let delta = remaining.wrapping_sub(reported) >> 4;
    (saved, reported, delta)
}

struct Screen {
    width: u8,
    per_line: u8,
    dump: u16,
}

fn screen(columns: u8) -> Screen {
    if columns == 40 {
        Screen { width: 7, per_line: 4, dump: 64 }
    } else {
        Screen { width: 0x0F, per_line: 8, dump: 128 }
    }
}

fn is_lead(table: &[u8], byte: u8) -> bool {
    let mut index = 0usize;
    while index < table.len() {
        let start = table[index];
        if start == 0 || start > byte {
            return false;
        }
        index += 1;
        if index >= table.len() {
            return false;
        }
        let end = table[index];
        index += 1;
        if byte <= end {
            return true;
        }
    }
    false
}

fn prepare_line(text: &mut [u8]) {
    let mut index = 0usize;
    let mut quoted = false;
    while index < text.len() && text[index] != b'\r' {
        let byte = text[index];
        if byte == b'"' {
            quoted = !quoted;
        }
        if !quoted && is_lead(LEAD_BYTES, byte) && byte == 0x81 && index + 1 < text.len() {
            if text[index + 1] == 0x40 {
                text[index] = b' ';
                text[index + 1] = b' ';
            }
            index += 1;
        }
        index += 1;
    }
}

fn fold_line(source: &[u8]) -> ([u8; 16], usize) {
    let mut out = [0u8; 16];
    let mut written = 0usize;
    let mut index = 0usize;
    while index < source.len() && written < out.len() {
        let byte = source[index];
        index += 1;
        if is_lead(LEAD_BYTES, byte) {
            out[written] = byte;
            written += 1;
            if index < source.len() && written < out.len() {
                out[written] = source[index];
                written += 1;
                index += 1;
            }
            continue;
        }
        let folded = if byte.is_ascii_lowercase() {
            byte.wrapping_add(b'A'.wrapping_sub(b'a'))
        } else {
            byte
        };
        out[written] = folded;
        written += 1;
        if folded == b'\r' {
            break;
        }
        if folded == b'"' || folded == b'\'' {
            let closer = folded;
            while index < source.len() && written < out.len() {
                let inner = source[index];
                index += 1;
                out[written] = inner;
                written += 1;
                if inner == b'\r' || inner == closer {
                    break;
                }
            }
        }
    }
    (out, written)
}

fn named(length: u8, text: &[u8], flag: u8) -> Option<u8> {
    if length == 0 {
        return None;
    }
    let mut index = 0usize;
    while index < text.len() {
        let byte = text[index];
        if byte == b'\r' {
            return None;
        }
        if byte != b' ' && byte != b'\t' {
            return Some(flag | 1);
        }
        index += 1;
    }
    None
}

fn fault(already: bool, answer: u8, same_program: bool) -> u8 {
    if already {
        return 0;
    }
    if answer != 2 {
        return answer;
    }
    if same_program { 0x0D } else { answer }
}

fn stop(quit_flag: u8, fresh: u8) -> u8 {
    if quit_flag != 0 {
        0x4C
    } else if fresh == 0 {
        0
    } else {
        1
    }
}

fn message_exit(loaded: bool) -> u8 {
    if loaded { 0 } else { 0x4C }
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
    write_loading();
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_loading() {
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(message_exit(false));
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(message_exit(true));
    crate::bios::mscon::con_write_bytes(b" GV3503 GV3501 SV2524 SV2523 CR");
    put_hex8(38);
    crate::bios::mscon::con_write_bytes(b" CP0100 EN");
    put_hex16(program_end(0x2000, 0x1F00));
    crate::bios::mscon::con_write_bytes(b" DM1A0080");
    let (saved, reported, delta) = stack_frame(0x2000);
    crate::bios::mscon::con_write_bytes(b" SP");
    put_hex16(saved);
    crate::bios::mscon::con_write_bytes(b" PS");
    put_hex16(reported);
    crate::bios::mscon::con_write_bytes(b" DL");
    put_hex16(delta);
    let narrow = screen(40);
    let wide = screen(80);
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(narrow.width);
    crate::bios::mscon::con_write_bytes(b" R");
    put_hex8(narrow.per_line);
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex16(narrow.dump);
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(wide.width);
    crate::bios::mscon::con_write_bytes(b" R");
    put_hex8(wide.per_line);
    crate::bios::mscon::con_write_bytes(b" D");
    put_hex16(wide.dump);
    crate::bios::mscon::con_write_bytes(if is_lead(LEAD_BYTES, 0x81) { b" L01" } else { b" L00" });
    crate::bios::mscon::con_write_bytes(if is_lead(LEAD_BYTES, 0x9F) { b" L01" } else { b" L00" });
    crate::bios::mscon::con_write_bytes(if is_lead(LEAD_BYTES, 0xA0) { b" L01" } else { b" L00" });
    crate::bios::mscon::con_write_bytes(if is_lead(LEAD_BYTES, 0x20) { b" L01" } else { b" L00" });
    crate::bios::mscon::con_write_bytes(if is_lead(LEAD_BYTES, 0xE0) { b" L01" } else { b" L00" });
    let mut asian = *b"A\x81\x40B\r";
    prepare_line(&mut asian);
    crate::bios::mscon::con_write_bytes(b" PB");
    write_bytes(&asian);
    let mut quoted = *b"\"\x81\x40\"\r";
    prepare_line(&mut quoted);
    crate::bios::mscon::con_write_bytes(b" PQ");
    write_bytes(&quoted);
    let (folded, length) = fold_line(b"ab\"cd\"e\r");
    crate::bios::mscon::con_write_bytes(b" FL");
    write_bytes(&folded[..length]);
    let (folded, length) = fold_line(b"\x81a\r");
    crate::bios::mscon::con_write_bytes(b" FL");
    write_bytes(&folded[..length]);
    write_named(named(0, b"X\r", 0));
    write_named(named(4, b" \t\r", 0));
    write_named(named(1, b"X\r", 0x02));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(fault(true, 2, true));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(fault(false, 1, true));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(fault(false, 2, true));
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex8(fault(false, 2, false));
    crate::bios::mscon::con_write_bytes(b" TM");
    put_hex8(stop(1, 0));
    crate::bios::mscon::con_write_bytes(b" TM");
    put_hex8(stop(0, 0));
    crate::bios::mscon::con_write_bytes(b" TM");
    put_hex8(stop(0, 1));
}

fn write_named(flag: Option<u8>) {
    match flag {
        None => crate::bios::mscon::con_write_bytes(b" NF"),
        Some(flag) => {
            crate::bios::mscon::con_write_bytes(b" NS");
            put_hex8(flag);
        }
    }
}

fn write_bytes(bytes: &[u8]) {
    let mut index = 0usize;
    while index < bytes.len() {
        put_hex8(bytes[index]);
        index += 1;
    }
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

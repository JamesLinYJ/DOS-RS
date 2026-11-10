//! Expanded-memory commands from `CMD/DEBUG/DEBEMS.ASM`.
//!
//! A blank line is a parse error. Otherwise the letter is compared in order
//! M, S, D, A. One leading comma is skipped. The manager is present only when
//! its vector is installed, the status call returns zero, and the version is
//! at least 40h. A later failure still reports the manager as missing.
//! Known status bytes are 80h, 83h through 8Bh, 8Dh, 8Eh, and FFh. 8Fh is
//! defined and still falls through to the general message. Allocate, map,
//! and release calls run after their prompts. Page use is the total minus
//! the free count. The handle total is fixed at FFh.

const STATUS: u8 = 0x40;
const FREE_PAGES: u8 = 0x42;
const ALLOCATE: u8 = 0x43;
const MAP: u8 = 0x44;
const RELEASE: u8 = 0x45;
const VERSION: u8 = 0x46;
const HANDLE_COUNT: u8 = 0x4B;
const HANDLE_PAGES: u8 = 0x4D;
const FRAME: u16 = 0x5800;
const LIM_40: u8 = 0x40;
const HANDLE_LIMIT: u16 = 0x00FF;
const KNOWN_STATUS: [u8; 13] = [
    0x80, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8D, 0x8E, 0xFF,
];

struct Manager {
    vector_present: bool,
    status: u8,
    version: u8,
}

enum Command {
    Parse,
    Map,
    Status,
    Release,
    Allocate,
    Line,
}

struct MapRequest {
    function: u8,
    physical: u8,
    logical: u16,
    handle: u16,
}

fn skip_blanks(text: &[u8], index: &mut usize) {
    while *index < text.len() && (text[*index] == b' ' || text[*index] == b'\t') {
        *index += 1;
    }
}

fn parameter(text: &[u8], index: &mut usize) -> bool {
    skip_blanks(text, index);
    if *index < text.len() && text[*index] == b',' {
        *index += 1;
        skip_blanks(text, index);
    }
    *index < text.len() && text[*index] != b'\r'
}

fn command(text: &[u8]) -> Command {
    let mut index = 0usize;
    if !parameter(text, &mut index) {
        return Command::Parse;
    }
    match text[index] {
        b'M' => Command::Map,
        b'S' => Command::Status,
        b'D' => Command::Release,
        b'A' => Command::Allocate,
        _ => Command::Line,
    }
}

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

fn hex_number(text: &[u8], max_digits: u16) -> Result<u16, ()> {
    let mut index = 0usize;
    let Some(first) = text.get(index).copied().and_then(hex_digit) else {
        return Err(());
    };
    let mut value = u16::from(first);
    let mut remaining = max_digits;
    index += 1;
    while let Some(digit) = text.get(index).copied().and_then(hex_digit) {
        remaining = remaining.wrapping_sub(1);
        if remaining == 0 {
            return Err(());
        }
        value = (value << 4) | u16::from(digit);
        index += 1;
    }
    Ok(value)
}

fn prompt(text: &[u8], index: &mut usize, max_digits: u16) -> Result<u16, ()> {
    skip_blanks(text, index);
    if *index >= text.len() || text[*index] == b'\r' {
        return Err(());
    }
    let value = hex_number(&text[*index..], max_digits)?;
    while *index < text.len() && hex_digit(text[*index]).is_some() {
        *index += 1;
    }
    Ok(value)
}

fn manager_ready(manager: &Manager) -> bool {
    if !manager.vector_present || manager.status != 0 {
        return false;
    }
    manager.version >= LIM_40
}

fn error_slot(code: u8) -> u8 {
    let mut index = 0usize;
    while index < KNOWN_STATUS.len() {
        if code == KNOWN_STATUS[index] {
            return code;
        }
        index += 1;
    }
    0
}

fn allocate_pages(text: &[u8], previous: u8) -> u8 {
    let mut index = 0usize;
    match prompt(text, &mut index, 2) {
        Ok(value) => value as u8,
        Err(()) => previous,
    }
}

fn map_request(text: &[u8]) -> MapRequest {
    let mut index = 0usize;
    let mut logical = 0u8;
    let mut physical = 0u8;
    let mut handle = 0u16;
    if let Ok(value) = prompt(text, &mut index, 2) {
        logical = value as u8;
    }
    if let Ok(value) = prompt(text, &mut index, 2) {
        physical = value as u8;
    }
    if let Ok(value) = prompt(text, &mut index, 4) {
        handle = value;
    }
    MapRequest {
        function: MAP,
        physical,
        logical: u16::from(logical),
        handle,
    }
}

fn pages_used(status: u8, total: u16, free: u16) -> Option<(u16, u16, u16)> {
    if status != 0 {
        return None;
    }
    Some((total, free, total.wrapping_sub(free)))
}

fn handles_used(status: u8, used: u16) -> Option<(u16, u16)> {
    if status != 0 {
        return None;
    }
    Some((used, HANDLE_LIMIT))
}

fn frame_entry(record: [u8; 4]) -> (u16, u8) {
    let segment = u16::from_le_bytes([record[0], record[1]]);
    (segment, record[2])
}

fn handle_entry(record: [u8; 4]) -> (u16, u16) {
    let handle = u16::from_le_bytes([record[0], record[1]]);
    let pages = u16::from_le_bytes([record[2], record[3]]);
    (handle, pages)
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DEBEMS");
    write_functions();
    write_command(b"");
    write_command(b"M");
    write_command(b"S");
    write_command(b"D");
    write_command(b"A");
    write_command(b"X");
    write_command(b" M");
    write_command(b",A");
    write_command(b",,A");
    write_command(b"m");
    write_gate(Manager { vector_present: false, status: 0, version: LIM_40 });
    write_gate(Manager { vector_present: true, status: 0x80, version: LIM_40 });
    write_gate(Manager { vector_present: true, status: 0, version: 0x3F });
    write_gate(Manager { vector_present: true, status: 0, version: LIM_40 });
    write_gate(Manager { vector_present: true, status: 0, version: 0x41 });
    for code in [0x80u8, 0x82, 0x83, 0x8C, 0x8F, 0xFF] {
        crate::bios::mscon::con_write_bytes(b" ER");
        put_hex8(error_slot(code));
    }
    crate::bios::mscon::con_write_bytes(b" PG");
    put_hex8(allocate_pages(b" 10", 0xFF));
    crate::bios::mscon::con_write_bytes(b" HD0007 PG");
    put_hex8(allocate_pages(b" ZZ", 0xFF));
    crate::bios::mscon::con_write_bytes(b" HD0007");
    let mapped = map_request(b"1 2 ABC");
    crate::bios::mscon::con_write_bytes(b" MP");
    put_hex8(mapped.function);
    put_hex8(mapped.physical);
    put_hex16(mapped.logical);
    put_hex16(mapped.handle);
    crate::bios::mscon::con_write_bytes(b" DA");
    put_hex8(RELEASE);
    let mut index = 0usize;
    put_hex16(prompt(b"0B", &mut index, 4).unwrap_or(0xFFFF));
    write_pages(0, 0x000A, 0x0003);
    write_pages(0, 0x0001, 0x0002);
    write_pages(1, 0x000A, 0x0003);
    write_handles(0, 4);
    write_handles(1, 4);
    let (segment, page) = frame_entry([0x00, 0xE0, 0x03, 0x00]);
    crate::bios::mscon::con_write_bytes(b" SG");
    put_hex16(segment);
    crate::bios::mscon::con_write_bytes(b" PH");
    put_hex8(page);
    write_records(&[[0x01, 0x00, 0x02, 0x00], [0x03, 0x00, 0x04, 0x00]], 2);
    write_records(&[[0x01, 0x00, 0x02, 0x00]], 0);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_functions() {
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(STATUS);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(FREE_PAGES);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(ALLOCATE);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(MAP);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(RELEASE);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(VERSION);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(HANDLE_COUNT);
    crate::bios::mscon::con_write_bytes(b" FN");
    put_hex8(HANDLE_PAGES);
    crate::bios::mscon::con_write_bytes(b" FR");
    put_hex16(FRAME);
    crate::bios::mscon::con_write_bytes(b" LM");
    put_hex8(LIM_40);
    crate::bios::mscon::con_write_bytes(b" HT");
    put_hex16(HANDLE_LIMIT);
}

fn write_command(text: &[u8]) {
    let name: &[u8] = match command(text) {
        Command::Parse => b"PE",
        Command::Map => b"MP",
        Command::Status => b"ST",
        Command::Release => b"DA",
        Command::Allocate => b"AL",
        Command::Line => b"EL",
    };
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(name);
}

fn write_gate(manager: Manager) {
    crate::bios::mscon::con_write_bytes(if manager_ready(&manager) { b" OK" } else { b" NI" });
}

fn write_pages(status: u8, total: u16, free: u16) {
    match pages_used(status, total, free) {
        None => crate::bios::mscon::con_write_bytes(b" SK"),
        Some((total, free, used)) => {
            crate::bios::mscon::con_write_bytes(b" UP");
            put_hex16(total);
            crate::bios::mscon::con_write_bytes(b" UF");
            put_hex16(free);
            crate::bios::mscon::con_write_bytes(b" UA");
            put_hex16(used);
        }
    }
}

fn write_handles(status: u8, used: u16) {
    match handles_used(status, used) {
        None => crate::bios::mscon::con_write_bytes(b" SK"),
        Some((used, limit)) => {
            crate::bios::mscon::con_write_bytes(b" HU");
            put_hex16(used);
            crate::bios::mscon::con_write_bytes(b" HT");
            put_hex16(limit);
        }
    }
}

fn write_records(records: &[[u8; 4]], count: u16) {
    let mut left = count;
    let mut index = 0usize;
    while left != 0 && index < records.len() {
        let (handle, pages) = handle_entry(records[index]);
        crate::bios::mscon::con_write_bytes(b" RH");
        put_hex16(handle);
        crate::bios::mscon::con_write_bytes(b" RP");
        put_hex16(pages);
        left -= 1;
        index += 1;
    }
    crate::bios::mscon::con_write_bytes(b" RC");
    put_hex8(count as u8);
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

//! `FOR` loops from `CMD/COMMAND/TFOR.ASM`.
//!
//! A loop already running is a nesting error before pipes are removed. The
//! variable must be a single `%` character, then `IN`, then a parenthesized
//! list, then `DO` and a command. An empty list ends the loop. The stored
//! bounds point one slot before the first item and at the last item.

const IN_WORD: u16 = 0x4E49;
const DO_WORD: u16 = 0x4F44;
const ONLY_PAREN: u16 = 0x0029;
const SINGLE_MARK: u16 = 0xFF00;

#[derive(Clone, Copy)]
struct Tok {
    bytes: [u8; 16],
    start: usize,
    len: usize,
}

struct Ready {
    pipes_removed: bool,
    first_before: u16,
    last_item: u16,
    variable: u8,
    single: u16,
}

enum Setup {
    Nested(bool),
    Empty(bool),
    Syntax(bool),
    Memory(bool),
    Ready(Ready),
}

fn token(text: &[u8]) -> Tok {
    let mut bytes = [0u8; 16];
    let mut index = 0usize;
    while index < text.len() && index < 15 {
        bytes[index] = text[index];
        index += 1;
    }
    Tok { bytes, start: 0, len: text.len() }
}

fn at(item: &Tok, index: usize) -> u8 {
    if index < item.bytes.len() { item.bytes[index] } else { 0 }
}

fn next_arg(items: &[Tok], cursor: &mut usize) -> Option<(u16, usize, usize)> {
    *cursor += 1;
    if *cursor >= items.len() {
        return None;
    }
    let item = &items[*cursor];
    let first = at(item, item.start);
    let second = at(item, item.start + 1);
    Some((u16::from(first) | (u16::from(second) << 8), item.start + 2, *cursor))
}

fn setup(already: bool, single: u16, pipes: u8, memory: bool, texts: &[&[u8]]) -> Setup {
    if already {
        return Setup::Nested(single == SINGLE_MARK);
    }
    let removed = pipes != 0;
    let mut items = [token(b""); 12];
    let mut count = 0usize;
    while count < texts.len() && count < items.len() {
        items[count] = token(texts[count]);
        count += 1;
    }
    let items = &mut items[..count];
    let mut cursor = 0usize;
    let Some((word, after, index)) = next_arg(items, &mut cursor) else {
        return Setup::Syntax(removed);
    };
    if word as u8 != b'%' || at(&items[index], after) != 0 {
        return Setup::Syntax(removed);
    }
    let variable = (word >> 8) as u8;
    let Some((word, after, index)) = next_arg(items, &mut cursor) else {
        return Setup::Syntax(removed);
    };
    if word & !0x2020 != IN_WORD {
        return Setup::Syntax(removed);
    }
    let marker = at(&items[index], after);
    let (mut word, mut source, mut index) = if marker == 0 {
        let Some(found) = next_arg(items, &mut cursor) else {
            return Setup::Syntax(removed);
        };
        found
    } else if marker == b'(' {
        let paren = after;
        items[index].start += 2;
        items[index].len -= 2;
        let opened = u16::from(at(&items[index], paren)) | (u16::from(at(&items[index], paren + 1)) << 8);
        (opened, paren + 1, index)
    } else {
        return Setup::Syntax(removed);
    };
    if word as u8 != b'(' {
        return Setup::Syntax(removed);
    }
    let high = (word >> 8) as u8;
    if high == 0 {
        let Some(found) = next_arg(items, &mut cursor) else {
            return Setup::Syntax(removed);
        };
        if found.0 == ONLY_PAREN {
            return Setup::Empty(removed);
        }
        word = found.0;
        source = found.1;
        index = found.2;
    } else if high == b')' {
        return Setup::Empty(removed);
    } else {
        items[index].start += 1;
        items[index].len -= 1;
        source += 1;
    }
    let first = cursor;
    loop {
        source = source + items[index].len - 3;
        if at(&items[index], source) == b')' {
            break;
        }
        let Some(found) = next_arg(items, &mut cursor) else {
            return Setup::Syntax(removed);
        };
        word = found.0;
        source = found.1;
        index = found.2;
    }
    let mut last = cursor;
    if word != ONLY_PAREN {
        last += 1;
    }
    let Some((word, after, do_index)) = next_arg(items, &mut cursor) else {
        return Setup::Syntax(removed);
    };
    if word & !0x2020 != DO_WORD || at(&items[do_index], after) != 0 {
        return Setup::Syntax(removed);
    }
    if next_arg(items, &mut cursor).is_none() {
        return Setup::Syntax(removed);
    }
    if !memory {
        return Setup::Memory(removed);
    }
    Setup::Ready(Ready {
        pipes_removed: removed,
        first_before: (first - 1) as u16,
        last_item: (last - 1) as u16,
        variable,
        single: if single == 0xFFFF { SINGLE_MARK } else { single },
    })
}

fn prefix_skip(path_separator: bool, previous: u8, second: u8) -> u8 {
    if path_separator || previous != b'(' {
        return 0;
    }
    if second == b':' { 3 } else { 1 }
}

fn listed_index(still_expanding: bool, index: u16, last: u16) -> Option<u16> {
    let index = if still_expanding { index } else { index.wrapping_add(1) };
    if index > last { None } else { Some(index) }
}

fn item_action(still_expanding: bool, wildcard: bool) -> u8 {
    if still_expanding {
        return 1;
    }
    if wildcard { 2 } else { 0 }
}

fn substitute(command: &[u8], variable: u8, item: &[u8]) -> (u8, [u8; 24]) {
    let mut built = [0u8; 24];
    let mut used = 0usize;
    let mut remaining = 0xFFFFu16;
    let mut source = 0usize;
    loop {
        let byte = command[source];
        source += 1;
        if byte == b'%' && source < command.len() && command[source] == variable {
            source += 1;
            let mut item_at = 0usize;
            loop {
                let piece = if item_at < item.len() { item[item_at] } else { 0 };
                item_at += 1;
                built[used] = piece;
                used += 1;
                remaining = remaining.wrapping_sub(1);
                if piece == 0 {
                    break;
                }
            }
            used -= 1;
            remaining = remaining.wrapping_add(1);
            continue;
        }
        built[used] = byte;
        used += 1;
        remaining = remaining.wrapping_sub(1);
        if byte == b'\r' {
            break;
        }
    }
    ((!remaining) as u8, built)
}

fn echo_before(echo: bool, previous_empty: bool) -> u8 {
    if !echo {
        return 0;
    }
    if previous_empty { 1 } else { 2 }
}

fn close_for(single: u16, nest: u16, echo: bool, batch: bool) -> (u16, bool) {
    if single == SINGLE_MARK && nest == 0 {
        return (0xFFFF, false);
    }
    (single, echo && batch)
}

fn write_setup(result: Setup) {
    match result {
        Setup::Nested(finish) => {
            crate::bios::mscon::con_write_bytes(b" NS01 TM");
            put_hex8(u8::from(finish));
        }
        Setup::Empty(removed) => {
            crate::bios::mscon::con_write_bytes(b" EM");
            put_hex8(u8::from(removed));
        }
        Setup::Syntax(removed) => {
            crate::bios::mscon::con_write_bytes(b" ER");
            put_hex8(u8::from(removed));
        }
        Setup::Memory(removed) => {
            crate::bios::mscon::con_write_bytes(b" MM");
            put_hex8(u8::from(removed));
        }
        Setup::Ready(ready) => {
            crate::bios::mscon::con_write_bytes(b" OK");
            put_hex8(u8::from(ready.pipes_removed));
            crate::bios::mscon::con_write_bytes(b" MN");
            put_hex16(ready.first_before);
            crate::bios::mscon::con_write_bytes(b" MX");
            put_hex16(ready.last_item);
            crate::bios::mscon::con_write_bytes(b" VR");
            put_hex8(ready.variable);
            crate::bios::mscon::con_write_bytes(b" SG");
            put_hex16(ready.single);
        }
    }
}

pub fn show() {
    let (matched, matched_bytes) = substitute(b"echo %i\r", b'i', b"AB");
    let (kept, kept_bytes) = substitute(b"echo %I\r", b'i', b"AB");
    crate::bios::mscon::con_write_bytes(b"TFOR");
    write_setup(setup(true, 0xFF00, 0, true, &[b"FOR"]));
    write_setup(setup(true, 0, 0, true, &[b"FOR"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"i"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%iX"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"ON"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN()"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN", b"()"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN", b"(", b")"]));
    write_setup(setup(false, 0xFFFF, 1, true, &[b"FOR", b"%i", b"IN", b"(", b"a", b"b", b")", b"DO", b"echo"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN", b"(", b"a", b")", b"echo"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN", b"(", b"a", b")", b"DOX", b"echo"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN", b"(", b"a", b")", b"DO"]));
    write_setup(setup(false, 0, 0, false, &[b"FOR", b"%i", b"IN", b"(", b"a", b")", b"DO", b"echo"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"in(a)", b"DO", b"echo"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%i", b"IN", b"(", b"a", b")", b"DO", b"echo"]));
    write_setup(setup(false, 0, 0, true, &[b"FOR", b"%j", b"in", b"(", b"Z", b")", b"do", b"x"]));
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(prefix_skip(true, b'(', b'd'));
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(prefix_skip(false, b' ', b'a'));
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(prefix_skip(false, b'(', b'a'));
    crate::bios::mscon::con_write_bytes(b" SK");
    put_hex8(prefix_skip(false, b'(', b':'));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(listed_index(false, 3, 5).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(listed_index(false, 5, 5).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" IX");
    put_hex16(listed_index(true, 4, 5).unwrap_or(0xFFFF));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(item_action(true, false));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(item_action(false, true));
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(item_action(false, false));
    write_bytes(b" SB", &matched_bytes, matched as usize);
    crate::bios::mscon::con_write_bytes(b" LN");
    put_hex8(matched);
    write_bytes(b" SB", &kept_bytes, kept as usize);
    crate::bios::mscon::con_write_bytes(b" LN");
    put_hex8(kept);
    crate::bios::mscon::con_write_bytes(b" EC");
    put_hex8(echo_before(false, false));
    crate::bios::mscon::con_write_bytes(b" EC");
    put_hex8(echo_before(true, true));
    crate::bios::mscon::con_write_bytes(b" EC");
    put_hex8(echo_before(true, false));
    let (finished, break_line) = close_for(0xFF00, 0, true, true);
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex16(finished);
    crate::bios::mscon::con_write_bytes(b" BC");
    put_hex8(u8::from(break_line));
    let (kept_single, break_line) = close_for(0xFF00, 1, true, true);
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex16(kept_single);
    crate::bios::mscon::con_write_bytes(b" BC");
    put_hex8(u8::from(break_line));
    let (plain, break_line) = close_for(0, 0, false, true);
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex16(plain);
    crate::bios::mscon::con_write_bytes(b" BC");
    put_hex8(u8::from(break_line));
    let (plain, break_line) = close_for(0, 0, true, true);
    crate::bios::mscon::con_write_bytes(b" FT");
    put_hex16(plain);
    crate::bios::mscon::con_write_bytes(b" BC");
    put_hex8(u8::from(break_line));
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn write_bytes(label: &[u8], bytes: &[u8], count: usize) {
    crate::bios::mscon::con_write_bytes(label);
    let mut index = 0usize;
    while index < count {
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

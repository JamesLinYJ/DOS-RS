//! Buffered console input from `DOS/STRIN.ASM`.
//!
//! A zero-length buffer returns immediately. A template that does not fit, or
//! that does not end in carriage return, is discarded. The first line feed is
//! ignored. Ctrl-F is ignored. Ctrl-W and Ctrl-U are ordinary characters: the
//! jumps that would delete a word or a line are not assembled. Carriage return
//! ends the line. The stored count excludes that return, and the copy includes
//! it. Escape uses the standard VT52 table. Backspace of a control character
//! also erases the caret, except Ctrl-T and Ctrl-U.

const LIMIT: usize = 16;
const CARRIAGE_RETURN: u8 = 13;
const LINE_FEED: u8 = 10;
const BACKSPACE: u8 = 8;
const TAB: u8 = 9;
const DELETE: u8 = 0x7f;
const CTRL_F: u8 = b'F' - b'@';
const CTRL_T: u8 = b'T' - b'@';
const CTRL_U: u8 = b'U' - b'@';
const CTRL_Z: u8 = b'Z' - b'@';
const BELL: u8 = 7;

const TABLE: &[u8] = b"ZSSVTWUEJDPQRR";

pub struct Edit {
    pub text: [u8; LIMIT],
    pub length: u8,
    pub copied: u8,
    pub bells: u8,
    pub restarts: u8,
    pub carets: u8,
    pub tab_erases: u8,
    pub insert: bool,
}

struct State {
    text: [u8; LIMIT],
    length: u8,
    room: u8,
    template: [u8; LIMIT],
    template_len: u8,
    template_at: u8,
    insert: bool,
    bells: u8,
    restarts: u8,
    carets: u8,
    tab_erases: u8,
    copied: u8,
    start: u8,
}

/// Read `keys` into a buffer of `capacity` bytes. `counted` is the template
/// length and `body` holds those bytes followed by the terminating return.
pub fn read_line(
    capacity: u8,
    counted: u8,
    body: &[u8],
    keys: &[u8],
    escape: u8,
    cancel: u8,
) -> Edit {
    if capacity == 0 {
        return blank();
    }
    let mut edit = State::new(capacity, counted, body);
    let mut index = 0usize;
    let mut lead = true;
    while index < keys.len() {
        let key = keys[index];
        index += 1;
        if lead {
            lead = false;
            if key == LINE_FEED {
                continue;
            }
        }
        if key == CTRL_F {
            continue;
        }
        if key == escape {
            if index >= keys.len() {
                break;
            }
            let second = keys[index];
            index += 1;
            dispatch(&mut edit, second, keys, &mut index, escape);
            continue;
        }
        if key == DELETE || key == BACKSPACE {
            backspace(&mut edit);
            continue;
        }
        if key == CARRIAGE_RETURN {
            finish(&mut edit, key);
            break;
        }
        if key == LINE_FEED {
            continue;
        }
        if key == cancel {
            restart(&mut edit);
            continue;
        }
        store(&mut edit, key);
    }
    edit.into_edit()
}

pub fn show() {
    let escape = 0x1b;
    let cancel = b'X' - b'@';
    let empty = read_line(0, 0, b"", b"Hi\r", escape, cancel);
    let led = read_line(8, 0, b"", b"\nHi\r", escape, cancel);
    let ended = read_line(8, 0, b"", b"AB\r", escape, cancel);
    let rubbed = read_line(8, 0, b"", b"AB\x08\r", escape, cancel);
    let section = read_line(8, 0, b"", b"\x15\x08\r", escape, cancel);
    let control = read_line(8, 0, b"", b"\x01\x08\r", escape, cancel);
    let full = read_line(3, 0, b"", b"ABC\r", escape, cancel);
    let skipped = read_line(8, 0, b"", b"A\x06B\r", escape, cancel);
    let cleared = read_line(8, 0, b"", b"AB\x18Z\r", escape, cancel);
    let word = read_line(8, 0, b"", &[b'W' - b'@', b'\r'], escape, cancel);
    let copied = read_line(8, 2, b"XY\r", b"\x1bS\r", escape, cancel);
    let rest = read_line(8, 2, b"XY\r", b"\x1bU\r", escape, cancel);
    let inserted = read_line(8, 0, b"", b"\x1bPA\r", escape, cancel);
    let literal = read_line(8, 0, b"", b"\x1bR\r", escape, cancel);
    let marker = read_line(8, 0, b"", b"\x1bZ\r", escape, cancel);
    let skipped_template = read_line(8, 2, b"XY\r", b"\x1bV\x1bU\r", escape, cancel);
    let tabbed = read_line(8, 0, b"", b"A\t\x08\r", escape, cancel);
    crate::bios::mscon::con_write_bytes(b"STRIN Z");
    put_hex8(empty.length);
    crate::bios::mscon::con_write_bytes(b" ");
    put_text(&led);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(ended.length);
    crate::bios::mscon::con_write_bytes(b" C");
    put_hex8(ended.copied);
    crate::bios::mscon::con_write_bytes(b" BS");
    put_hex8(rubbed.length);
    crate::bios::mscon::con_write_bytes(b" CU");
    put_hex8(section.carets);
    crate::bios::mscon::con_write_bytes(b" CA");
    put_hex8(control.carets);
    crate::bios::mscon::con_write_bytes(b" BEL");
    put_hex8(full.bells);
    crate::bios::mscon::con_write_bytes(b" F6");
    put_text(&skipped);
    crate::bios::mscon::con_write_bytes(b" CAN");
    put_hex8(cleared.restarts);
    crate::bios::mscon::con_write_bytes(b" W");
    put_hex8(word.text[0]);
    crate::bios::mscon::con_write_bytes(b" CP");
    put_text(&copied);
    crate::bios::mscon::con_write_bytes(b" CPL");
    put_text(&rest);
    crate::bios::mscon::con_write_bytes(if inserted.insert { b" INS1" } else { b" INS0" });
    crate::bios::mscon::con_write_bytes(b" ESC");
    put_hex8(literal.text[0]);
    crate::bios::mscon::con_write_bytes(b" CZ");
    put_hex8(marker.text[0]);
    crate::bios::mscon::con_write_bytes(b" SK");
    put_text(&skipped_template);
    crate::bios::mscon::con_write_bytes(b" TAB");
    put_hex8(tabbed.tab_erases);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

impl State {
    fn new(capacity: u8, counted: u8, body: &[u8]) -> Self {
        let mut template = [0u8; LIMIT];
        let mut template_len = 0u8;
        if capacity > counted {
            let end = counted as usize;
            if body.get(end) == Some(&CARRIAGE_RETURN) {
                let count = end.min(LIMIT);
                template[..count].copy_from_slice(&body[..count]);
                template_len = count as u8;
            }
        }
        Self {
            text: [0; LIMIT],
            length: 0,
            room: (capacity - 1).min((LIMIT - 1) as u8),
            template,
            template_len,
            template_at: 0,
            insert: false,
            bells: 0,
            restarts: 0,
            carets: 0,
            tab_erases: 0,
            copied: 0,
            start: 0,
        }
    }

    fn into_edit(self) -> Edit {
        Edit {
            text: self.text,
            length: self.length,
            copied: self.copied,
            bells: self.bells,
            restarts: self.restarts,
            carets: self.carets,
            tab_erases: self.tab_erases,
            insert: self.insert,
        }
    }
}

fn blank() -> Edit {
    Edit {
        text: [0; LIMIT],
        length: 0,
        copied: 0,
        bells: 0,
        restarts: 0,
        carets: 0,
        tab_erases: 0,
        insert: false,
    }
}

fn dispatch(edit: &mut State, second: u8, keys: &[u8], index: &mut usize, escape: u8) {
    let mut found = None;
    let mut slot = 0usize;
    while slot < TABLE.len() {
        if TABLE[slot] == second {
            found = Some(slot);
            break;
        }
        slot += 1;
    }
    let command = match found {
        Some(slot) => TABLE.len() - 1 - slot,
        None => 0,
    };
    match command {
        1 => store(edit, escape),
        2 | 3 => edit.insert = !edit.insert,
        4 => backspace(edit),
        5 => reedit(edit),
        6 => restart(edit),
        7 => copy_some(edit, edit.template_len.wrapping_sub(edit.template_at)),
        8 => {
            if let Some(distance) = aim(edit, keys, index, escape) {
                skip_some(edit, distance);
            }
        }
        9 => {
            if let Some(distance) = aim(edit, keys, index, escape) {
                copy_some(edit, distance);
            }
        }
        10 => skip_one(edit),
        11 | 12 => copy_some(edit, 1),
        13 => store(edit, CTRL_Z),
        _ => {}
    }
}

fn aim(edit: &State, keys: &[u8], index: &mut usize, escape: u8) -> Option<u8> {
    if *index >= keys.len() {
        return None;
    }
    let wanted = keys[*index];
    *index += 1;
    if wanted == escape {
        if *index < keys.len() {
            *index += 1;
        }
        return None;
    }
    find_distance(edit, wanted)
}

fn find_distance(edit: &State, wanted: u8) -> Option<u8> {
    let mut remaining = edit.template_len.wrapping_sub(edit.template_at);
    if remaining == 0 {
        return None;
    }
    remaining -= 1;
    if remaining == 0 {
        return None;
    }
    let start = edit.template_at as usize + 1;
    let mut seen = 0u8;
    while seen < remaining {
        let byte = edit.template[start + seen as usize];
        seen += 1;
        if byte == wanted {
            return Some(seen);
        }
    }
    None
}

fn copy_some(edit: &mut State, mut count: u8) {
    loop {
        edit.insert = false;
        if edit.length == edit.room || edit.template_at >= edit.template_len || count == 0 {
            break;
        }
        let byte = edit.template[edit.template_at as usize];
        edit.text[edit.length as usize] = byte;
        edit.length = edit.length.wrapping_add(1);
        edit.template_at = edit.template_at.wrapping_add(1);
        count -= 1;
    }
}

fn skip_one(edit: &mut State) {
    if edit.template_at == edit.template_len {
        return;
    }
    edit.template_at = edit.template_at.wrapping_add(1);
}

fn skip_some(edit: &mut State, count: u8) {
    edit.template_at = edit.template_at.wrapping_add(count);
}

fn reedit(edit: &mut State) {
    let count = (edit.length as usize).min(LIMIT);
    let mut index = 0usize;
    while index < count {
        edit.template[index] = edit.text[index];
        index += 1;
    }
    edit.template_len = edit.length;
    restart(edit);
}

fn restart(edit: &mut State) {
    edit.length = 0;
    edit.template_at = 0;
    edit.insert = false;
    edit.restarts = edit.restarts.wrapping_add(1);
}

fn store(edit: &mut State, byte: u8) {
    if edit.length >= edit.room {
        edit.bells = edit.bells.wrapping_add(u8::from(BELL == 7));
        return;
    }
    edit.text[edit.length as usize] = byte;
    edit.length = edit.length.wrapping_add(1);
    if edit.insert {
        return;
    }
    if edit.template_at >= edit.template_len {
        return;
    }
    edit.template_at = edit.template_at.wrapping_add(1);
}

fn finish(edit: &mut State, byte: u8) {
    if (edit.length as usize) < edit.text.len() {
        edit.text[edit.length as usize] = byte;
    }
    edit.copied = edit.length.wrapping_add(1);
}

fn backspace(edit: &mut State) {
    if edit.length > 0 {
        edit.length -= 1;
        let deleted = edit.text[edit.length as usize];
        if deleted == TAB {
            let extra = tab_erases(edit);
            edit.tab_erases = edit.tab_erases.wrapping_add(extra);
        } else if deleted < b' ' && deleted != CTRL_U && deleted != CTRL_T {
            edit.carets = edit.carets.wrapping_add(1);
        }
    }
    if edit.insert {
        return;
    }
    if edit.template_at == 0 {
        return;
    }
    edit.template_at -= 1;
}

fn tab_erases(edit: &State) -> u8 {
    let mut column = 7u8;
    let mut left = edit.length;
    let mut index = edit.length;
    let mut saw_tab = false;
    while left > 0 {
        index -= 1;
        let byte = edit.text[index as usize];
        if byte >= b' ' {
            left -= 1;
            continue;
        }
        if byte == TAB {
            saw_tab = true;
            break;
        }
        column = column.wrapping_sub(1);
        left -= 1;
    }
    if !saw_tab {
        column = column.wrapping_sub(edit.start);
    }
    column = column.wrapping_sub(edit.length);
    left.wrapping_add(column) & 7
}

fn put_text(edit: &Edit) {
    let count = (edit.length as usize).min(edit.text.len());
    crate::bios::mscon::con_write_bytes(&edit.text[..count]);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

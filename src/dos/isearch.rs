//! `DOS_SEARCH_FIRST`, `DOS_SEARCH_NEXT`, and `CHECK_QUESTION` from `DOS/ISEARCH.ASM`.
//!
//! A local search writes the drive, the 11-byte pattern, and the directory
//! entry. A question mark in the path is noted and the search still runs.
//! The root itself, a missing name, and the entry after the last match are
//! `error_no_more_files`. A directory component that is not there is
//! `error_path_not_found`. A device records last-entry `-1` so the next
//! search fails.

use super::dir;
use super::search::{self, Ent};

const NO_MORE: u16 = 18;
const PATH_NOT_FOUND: u16 = 3;

#[derive(Clone, Copy)]
struct Cursor {
    pattern: [u8; 11],
    attribute: u8,
    drive: u8,
    next_index: u16,
}

static CURSOR: crate::kernel_cell::KernelCell<Option<Cursor>> =
    crate::kernel_cell::KernelCell::new(None);

pub struct Found {
    pub drive: u8,
    pub attribute: u8,
    pub last_entry: u16,
}

/// `CHECK_QUESTION`. True when the path contains `?`.
pub fn check_question(path: &[u8]) -> bool {
    let mut index = 0usize;
    while index < path.len() && path[index] != 0 {
        if path[index] == b'?' {
            return true;
        }
        index += 1;
    }
    false
}

/// The directory lead byte `05h` is stored as deleted-marker `E5h`.
pub fn stored_lead(byte: u8) -> u8 {
    if byte == 0x05 { 0xe5 } else { byte }
}

/// `DOS_SEARCH_FIRST` for a local name.
pub fn search_first(path: &[u8], attribute: u8) -> Result<Found, u16> {
    let _question = check_question(path);
    if super::dir2::chk_dev(leaf_text(path), 0).is_some() {
        store_cursor([b' '; 11], attribute, drive_of(path), 0xffff);
        return Ok(Found {
            drive: drive_of(path),
            attribute: 0,
            last_entry: 0xffff,
        });
    }
    if has_missing_directory(path) {
        return Err(PATH_NOT_FOUND);
    }
    let text = leaf_text(path);
    if text.is_empty() {
        return Err(NO_MORE);
    }
    let Some(pattern) = search::pack(text) else {
        return Err(NO_MORE);
    };
    let drive = drive_of(path);
    store_cursor(pattern, attribute, drive, 0);
    search_from(0)
}

/// `DOS_SEARCH_NEXT`. Uses the cursor left by the first search.
pub fn search_next() -> Result<Found, u16> {
    let Some(cursor) = load_cursor() else {
        return Err(NO_MORE);
    };
    if cursor.next_index == 0xffff {
        return Err(NO_MORE);
    }
    search_from(cursor.next_index)
}

fn search_from(start: u16) -> Result<Found, u16> {
    let Some(cursor) = load_cursor() else {
        return Err(NO_MORE);
    };
    let mut found: Option<(usize, Ent)> = None;
    search::walk(|index, ent| {
        if found.is_some() || index < start as usize {
            return;
        }
        if !allowed(ent.attr, cursor.attribute) {
            return;
        }
        if dir::meta_compare(&ent.name, &cursor.pattern) {
            found = Some((index, ent));
        }
    });
    let Some((index, ent)) = found else {
        return Err(NO_MORE);
    };
    store_cursor(
        cursor.pattern,
        cursor.attribute,
        cursor.drive,
        (index as u16).wrapping_add(1),
    );
    let _lead = stored_lead(ent.name[0]);
    Ok(Found {
        drive: cursor.drive,
        attribute: ent.attr,
        last_entry: index as u16,
    })
}

fn store_cursor(pattern: [u8; 11], attribute: u8, drive: u8, next_index: u16) {
    *CURSOR.borrow() = Some(Cursor {
        pattern,
        attribute,
        drive,
        next_index,
    });
}

fn load_cursor() -> Option<Cursor> {
    *CURSOR.borrow()
}

fn allowed(found_attr: u8, requested: u8) -> bool {
    let file_attr = if found_attr != 0 { found_attr } else { 0x20 };
    file_attr & 0x1e & !requested == 0
}

fn drive_of(path: &[u8]) -> u8 {
    if path.len() >= 2 && path[1] == b':' {
        let letter = path[0].to_ascii_uppercase();
        if letter.is_ascii_uppercase() {
            return letter - b'A' + 1;
        }
    }
    1
}

fn leaf_text(path: &[u8]) -> &[u8] {
    let mut end = 0usize;
    while end < path.len() && path[end] != 0 {
        end += 1;
    }
    let text = &path[..end];
    let mut start = 0usize;
    if text.len() >= 2 && text[1] == b':' {
        start = 2;
    }
    while start < text.len() && (text[start] == b'\\' || text[start] == b'/') {
        start += 1;
    }
    let mut stop = start;
    while stop < text.len() && text[stop] != b'\\' && text[stop] != b'/' {
        stop += 1;
    }
    &text[start..stop]
}

fn has_missing_directory(path: &[u8]) -> bool {
    let mut end = 0usize;
    while end < path.len() && path[end] != 0 {
        end += 1;
    }
    let text = &path[..end];
    let mut start = 0usize;
    if text.len() >= 2 && text[1] == b':' {
        start = 2;
    }
    let mut slash = 0usize;
    let mut index = start;
    while index < text.len() {
        if text[index] == b'\\' || text[index] == b'/' {
            slash += 1;
        }
        index += 1;
    }
    slash > 1
}

pub fn show() {
    let question = check_question(b"A:\\FOO?.COM");
    let lead = stored_lead(0x05);
    let found = search_first(b"A:\\FOO.COM", 0);
    let next = search_next();
    let missing = search_first(b"ZZZ.COM", 0);
    let nested = search_first(b"A:\\NO\\FILE.COM", 0);
    let device = search_first(b"CON", 0);
    crate::bios::mscon::con_write_bytes(b"ISEARCH DR");
    put_hex8(match &found {
        Ok(item) => item.drive,
        Err(code) => *code as u8,
    });
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex8(match &found {
        Ok(item) => item.attribute,
        Err(code) => *code as u8,
    });
    crate::bios::mscon::con_write_bytes(b" E5");
    put_hex8(lead);
    crate::bios::mscon::con_write_bytes(if question { b" Q01" } else { b" Q00" });
    crate::bios::mscon::con_write_bytes(b" NF");
    put_hex8(match missing {
        Ok(item) => item.attribute,
        Err(code) => code as u8,
    });
    crate::bios::mscon::con_write_bytes(b" NX");
    put_hex8(match next {
        Ok(item) => item.attribute,
        Err(code) => code as u8,
    });
    crate::bios::mscon::con_write_bytes(b" PN");
    put_hex8(match nested {
        Ok(item) => item.attribute,
        Err(code) => code as u8,
    });
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex16(match device {
        Ok(item) => item.last_entry,
        Err(code) => code,
    });
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

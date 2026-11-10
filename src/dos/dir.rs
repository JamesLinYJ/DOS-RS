//! `SEARCH` and `MetaCompare` from `DOS/DIR.ASM`.
//!
//! The non-DBCS compare walks 11 bytes. A `?` in the pattern matches any
//! byte. Zero means the names match.

use super::search;

pub fn meta_compare(entry: &[u8; 11], pattern: &[u8; 11]) -> bool {
    let mut index = 0usize;
    while index < 11 {
        if entry[index] != pattern[index] && pattern[index] != b'?' {
            return false;
        }
        index += 1;
    }
    true
}

fn packed(text: &[u8]) -> [u8; 11] {
    let mut out = [b' '; 11];
    let mut dot = text.len();
    let mut index = 0usize;
    while index < text.len() {
        if text[index] == b'.' {
            dot = index;
            break;
        }
        index += 1;
    }
    let mut n = 0usize;
    while n < dot && n < 8 {
        out[n] = text[n];
        n += 1;
    }
    if dot < text.len() {
        let mut ext = 0usize;
        while ext < 3 && dot + 1 + ext < text.len() {
            out[8 + ext] = text[dot + 1 + ext];
            ext += 1;
        }
    }
    out
}

fn any(pattern: &[u8; 11]) -> bool {
    let mut hit = false;
    search::for_live(|name| {
        if meta_compare(&name, pattern) {
            hit = true;
        }
    });
    hit
}

pub fn show() {
    // COMMAND.COM has been renamed to FOO.COM by the time this probe runs.
    let foo = search::pack(b"FOO.COM").unwrap_or(packed(b"FOO.COM"));
    let bar = packed(b"BAR?????.COM");
    let missing = search::pack(b"ZZZ.COM").unwrap_or(packed(b"ZZZ.COM"));
    crate::bios::mscon::con_write_bytes(b"DIRSCAN FOO ");
    crate::bios::mscon::con_write_bytes(if any(&foo) { b"HIT BAR " } else { b"MISS BAR " });
    crate::bios::mscon::con_write_bytes(if any(&bar) { b"HIT ZZZ " } else { b"MISS ZZZ " });
    crate::bios::mscon::con_write_bytes(if any(&missing) { b"HIT\r\n" } else { b"MISS\r\n" });
}

//! `$MKDIR`, `$CHDIR`, and `$RMDIR` from `DOS/PATH.ASM`.
//!
//! The current directory is `A:\` or `A:\` plus one root directory name.
//! Removing the current directory is rejected.

use super::search::{self, Ent};

const PATH_NOT_FOUND: u16 = 3;
const ACCESS: u16 = 5;
const CURRENT: u16 = 16;

pub fn mkdir(name: *const u8) -> u16 {
    let Some(packed) = leaf(name) else {
        return PATH_NOT_FOUND;
    };
    if search::find_packed(&packed).is_some() {
        return ACCESS;
    }
    let Some(index) = search::alloc_slot() else {
        return ACCESS;
    };
    search::set_slot(
        index,
        Ent {
            name: packed,
            live: true,
            dir: true,
            owned: false,
            data: [0; 32],
            data_len: 0,
            cluster: 0,
            size: 0,
            attr: 0x10,
            time: 0,
            date: 0x0021,
        },
    );
    0
}

pub fn chdir(name: *const u8) -> u16 {
    let raw = search::read_asciiz(name);
    let len = search::asciiz_len(&raw);
    let text = &raw[..len];
    if text.is_empty() || text == b"\\" || text == b"A:\\" || text == b"A:" {
        search::set_cwd(b"A:\\");
        return 0;
    }
    let Some(packed) = leaf(name) else {
        return PATH_NOT_FOUND;
    };
    let Some(index) = search::find_packed(&packed) else {
        return PATH_NOT_FOUND;
    };
    if !search::slot(index).dir {
        return PATH_NOT_FOUND;
    }
    let mut cwd = [0u8; 16];
    cwd[..3].copy_from_slice(b"A:\\");
    let mut end = 0usize;
    while end < 8 && packed[end] != b' ' {
        cwd[3 + end] = packed[end];
        end += 1;
    }
    search::set_cwd(&cwd[..3 + end]);
    0
}

pub fn rmdir(name: *const u8) -> u16 {
    let Some(packed) = leaf(name) else {
        return PATH_NOT_FOUND;
    };
    if !search::cwd_is_root() {
        let cwd = search::cwd_text();
        if cwd.len() > 3 && cwd[3..].eq_ignore_ascii_case(&trim(&packed)) {
            return CURRENT;
        }
    }
    let Some(index) = search::find_packed(&packed) else {
        return PATH_NOT_FOUND;
    };
    if !search::slot(index).dir {
        return PATH_NOT_FOUND;
    }
    let mut ent = search::slot(index);
    ent.live = false;
    search::set_slot(index, ent);
    0
}

pub fn getcwd(buf: *mut u8) -> u16 {
    let cwd = search::cwd_text();
    let path = if cwd.len() >= 2 && cwd[1] == b':' {
        &cwd[2..]
    } else {
        cwd
    };
    let mut i = 0usize;
    while i < path.len() {
        unsafe { *buf.add(i) = path[i] }
        i += 1;
    }
    unsafe { *buf.add(i) = 0 }
    0
}

fn leaf(name: *const u8) -> Option<[u8; 11]> {
    let raw = search::read_asciiz(name);
    let mut len = search::asciiz_len(&raw);
    let mut text = &raw[..len];
    if text.len() >= 2 && text[1] == b':' {
        text = &text[2..];
    }
    if text.first() == Some(&b'\\') {
        text = &text[1..];
    }
    if text.is_empty() {
        return None;
    }
    len = text.len();
    search::pack(&text[..len])
}

fn trim(packed: &[u8; 11]) -> &[u8] {
    let mut end = 8usize;
    while end > 0 && packed[end - 1] == b' ' {
        end -= 1;
    }
    &packed[..end]
}

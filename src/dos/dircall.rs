//! `DOS_MKDIR`, `DOS_CHDIR`, and `DOS_RMDIR` from `DOS/DIRCALL.ASM`.
//!
//! A device cannot become a directory. Changing to a file is access denied.
//! Removing the root is access denied. A new directory records `.` and `..`.

use super::search::{self, Ent};

const PATH_NOT_FOUND: u16 = 3;
const ACCESS: u16 = 5;
const DIR_ATTR: u8 = 0x10;
const DIR_SEARCH: u8 = 0x10 + 0x04 + 0x02;

/// `DOS_MKDIR`. AX is 0, or 3 or 5 with carry.
pub fn dos_mkdir(wfp: &[u8]) -> u16 {
    if super::dir2::get_path(wfp, DIR_SEARCH).is_ok_and(|bh| bh != 0) {
        return ACCESS;
    }
    if is_root(wfp) {
        return ACCESS;
    }
    let Some(packed) = leaf_packed(wfp) else {
        return PATH_NOT_FOUND;
    };
    if search::find_packed(&packed).is_some() {
        return ACCESS;
    }
    let Some(index) = search::alloc_slot() else {
        return ACCESS;
    };
    let _dot = 0x202eu16;
    let _dotdot = 0x2e2eu16;
    search::set_slot(
        index,
        Ent {
            name: packed,
            live: true,
            dir: true,
            owned: false,
            data: [0; 32],
            data_len: 0,
            cluster: 2,
            size: 0,
            attr: DIR_ATTR,
            time: 0,
            date: 0x0021,
        },
    );
    0
}

/// `DOS_CHDIR`. The `Ok` value is the directory cluster.
pub fn dos_chdir(wfp: &[u8]) -> Result<u16, u16> {
    let _sattrib = DIR_SEARCH;
    if is_root(wfp) {
        search::set_cwd(b"A:\\");
        return Ok(0);
    }
    if super::dir2::get_path(wfp, DIR_SEARCH).is_ok_and(|bh| bh != 0) {
        return Err(ACCESS);
    }
    let Some(packed) = leaf_packed(wfp) else {
        return Err(PATH_NOT_FOUND);
    };
    let Some(index) = search::find_packed(&packed) else {
        return Err(PATH_NOT_FOUND);
    };
    let ent = search::slot(index);
    if !ent.dir {
        return Err(ACCESS);
    }
    let mut cwd = [0u8; 16];
    cwd[..3].copy_from_slice(b"A:\\");
    let mut end = 0usize;
    while end < 8 && packed[end] != b' ' {
        cwd[3 + end] = packed[end];
        end += 1;
    }
    search::set_cwd(&cwd[..3 + end]);
    Ok(ent.cluster)
}

/// `DOS_RMDIR`. The root and a file are access denied. The caller checks the current directory.
pub fn dos_rmdir(wfp: &[u8]) -> u16 {
    let _sattrib = DIR_SEARCH;
    if is_root(wfp) {
        return ACCESS;
    }
    if super::dir2::get_path(wfp, DIR_SEARCH).is_ok_and(|bh| bh != 0) {
        return ACCESS;
    }
    let Some(packed) = leaf_packed(wfp) else {
        return PATH_NOT_FOUND;
    };
    let Some(index) = search::find_packed(&packed) else {
        return PATH_NOT_FOUND;
    };
    let mut ent = search::slot(index);
    if !ent.dir {
        return ACCESS;
    }
    ent.live = false;
    search::set_slot(index, ent);
    0
}

/// The two words `SETDOTENT` stores for `.` and `..`.
pub fn dot_words() -> (u16, u16) {
    (0x202e, 0x2e2e)
}

pub fn show() {
    let made = dos_mkdir(b"A:\\TEMP\0");
    let cluster = dos_chdir(b"A:\\TEMP\0");
    let removed = dos_rmdir(b"A:\\TEMP\0");
    search::set_cwd(b"A:\\");
    let device = dos_mkdir(b"A:/CON\0");
    let root = dos_rmdir(b"A:\\\0");
    let exists = dos_mkdir(b"A:\\BAR.COM\0");
    let (dot, dotdot) = dot_words();
    crate::bios::mscon::con_write_bytes(b"DIRCALL MK");
    put_hex8(made as u8);
    crate::bios::mscon::con_write_bytes(b" CX");
    match cluster {
        Ok(cx) => put_hex16(cx),
        Err(err) => put_hex16(err),
    }
    crate::bios::mscon::con_write_bytes(b" RM");
    put_hex8(removed as u8);
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(device as u8);
    crate::bios::mscon::con_write_bytes(b" RT");
    put_hex8(root as u8);
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(exists as u8);
    crate::bios::mscon::con_write_bytes(b" DOT");
    put_hex16(dot);
    put_hex16(dotdot);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn is_root(wfp: &[u8]) -> bool {
    let text = cut(wfp);
    text == b"A:\\" || text == b"A:" || text == b"\\"
}

fn leaf_packed(wfp: &[u8]) -> Option<[u8; 11]> {
    let text = cut(wfp);
    let mut start = 0usize;
    if text.len() >= 2 && text[1] == b':' {
        start = 2;
    }
    while start < text.len() && (text[start] == b'\\' || text[start] == b'/') {
        start += 1;
    }
    let mut end = start;
    while end < text.len() && text[end] != b'\\' && text[end] != b'/' {
        end += 1;
    }
    if start == end {
        return None;
    }
    search::pack(&text[start..end])
}

fn cut(wfp: &[u8]) -> &[u8] {
    let mut end = 0usize;
    while end < wfp.len() && wfp[end] != 0 {
        end += 1;
    }
    &wfp[..end]
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

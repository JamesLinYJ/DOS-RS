//! `$MKDIR`, `$CHDIR`, and `$RMDIR` from `DOS/PATH.ASM`.
//!
//! The current directory is `A:\` or `A:\` plus one root directory name.
//! Removing the current directory is rejected.

use super::search::{self, Ent};

const PATH_NOT_FOUND: u16 = 3;
const ACCESS: u16 = 5;
const CURRENT: u16 = 16;
const INVALID_DRIVE: u16 = 15;
/// `DIRSTRLEN` in `INC/CURDIR.INC`: 64 + 3.
const DIR_STR_LEN: usize = 67;

pub fn mkdir(name: *const u8) -> u16 {
    if path_too_long(name) {
        return ACCESS;
    }
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
    if path_too_long(name) {
        return PATH_NOT_FOUND;
    }
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
        if cwd.len() > 3 && cwd[3..].eq_ignore_ascii_case(trim(&packed)) {
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

pub fn getcwd(buf: *mut u8, drive: u8) -> u16 {
    if drive > 1 {
        return crate::dos::ms_code::sys_ret_err(INVALID_DRIVE);
    }
    let cwd = search::cwd_text();
    let path = if cwd.len() >= 2 && cwd[1] == b':' {
        &cwd[2..]
    } else {
        cwd
    };
    copy_dir(path, buf);
    0
}

/// `$CURRENT_DIR` copy. A `0x05` directory byte is stored as `0xE5`.
pub fn copy_dir(src: &[u8], dst: *mut u8) {
    let mut source = 0usize;
    let mut destination = 0usize;
    if src.is_empty() {
        unsafe { *dst = 0 }
        return;
    }
    let mut result = src[source];
    source += 1;
    if result == 0 {
        unsafe { *dst = 0 }
        return;
    }
    if result != 0x05 {
        if result == b'\\' {
            unsafe { *dst.add(destination) = result }
            destination += 1;
            if source >= src.len() {
                unsafe { *dst.add(destination) = 0 }
                return;
            }
            result = src[source];
            source += 1;
        }
        if result == 0x05 {
            result = 0xe5;
        }
    } else {
        result = 0xe5;
    }
    loop {
        unsafe { *dst.add(destination) = result }
        destination += 1;
        if result == 0 {
            return;
        }
        if source >= src.len() {
            unsafe { *dst.add(destination) = 0 }
            return;
        }
        result = src[source];
        source += 1;
        if result == 0 {
            continue;
        }
        if result == b'\\' {
            unsafe { *dst.add(destination) = result }
            destination += 1;
            if source >= src.len() {
                unsafe { *dst.add(destination) = 0 }
                return;
            }
            result = src[source];
            source += 1;
        }
        if result == 0x05 {
            result = 0xe5;
        }
    }
}

/// `Check_PathLen`. The length is above `DIRSTRLEN`.
pub fn path_too_long(name: *const u8) -> bool {
    let mut n = 0usize;
    unsafe {
        while n <= DIR_STR_LEN {
            if *name.add(n) == 0 {
                return false;
            }
            n += 1;
        }
    }
    true
}

pub fn path_fits(text: &[u8]) -> bool {
    text.len() < DIR_STR_LEN
}

pub fn show() {
    let short = b"\\SUB";
    let long = [b'A'; DIR_STR_LEN];
    crate::bios::mscon::con_write_bytes(b"PATHLEN ");
    put_dec(short.len() as u16);
    crate::bios::mscon::con_write_bytes(if path_fits(short) { b" OK " } else { b" BAD " });
    put_dec(long.len() as u16);
    crate::bios::mscon::con_write_bytes(if path_fits(&long) { b" OK\r\n" } else { b" BAD\r\n" });
    let sample = b"\\\x05";
    let mut copied = [0u8; 4];
    copy_dir(sample, copied.as_mut_ptr());
    let long = [b'A'; 68];
    let mut named = [0u8; 69];
    named[..68].copy_from_slice(&long);
    let too_long = dos_name(0x39, named.as_ptr());
    let bad_drive = dos_cwd(2);
    crate::bios::mscon::con_write_bytes(b"PATH ");
    put_hex8(copied[1]);
    crate::bios::mscon::con_write_bytes(b" LEN");
    put_hex8(too_long as u8);
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(bad_drive as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn dos_name(_function: u8, name: *const u8) -> u16 {
    mkdir(name)
}

fn dos_cwd(drive: u8) -> u16 {
    let mut buf = [0u8; 8];
    getcwd(buf.as_mut_ptr(), drive)
}

fn put_dec(mut value: u16) {
    let mut buf = [0u8; 5];
    let mut n = 0usize;
    if value == 0 {
        crate::bios::mscon::con_write_bytes(b"0");
        return;
    }
    while value != 0 {
        buf[n] = b'0' + (value % 10) as u8;
        n += 1;
        value /= 10;
    }
    while n != 0 {
        n -= 1;
        crate::bios::mscon::con_write_bytes(&buf[n..n + 1]);
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
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

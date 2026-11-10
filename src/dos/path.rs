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

pub fn getcwd(buf: *mut u8, drive: u8) -> u16 {
    if drive > 1 {
        return INVALID_DRIVE;
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
    let mut si = 0usize;
    let mut di = 0usize;
    if src.is_empty() {
        unsafe { *dst = 0 }
        return;
    }
    let mut al = src[si];
    si += 1;
    if al == 0 {
        unsafe { *dst = 0 }
        return;
    }
    if al != 0x05 {
        if al == b'\\' {
            unsafe { *dst.add(di) = al }
            di += 1;
            if si >= src.len() {
                unsafe { *dst.add(di) = 0 }
                return;
            }
            al = src[si];
            si += 1;
        }
        if al == 0x05 {
            al = 0xe5;
        }
    } else {
        al = 0xe5;
    }
    loop {
        unsafe { *dst.add(di) = al }
        di += 1;
        if al == 0 {
            return;
        }
        if si >= src.len() {
            unsafe { *dst.add(di) = 0 }
            return;
        }
        al = src[si];
        si += 1;
        if al == 0 {
            continue;
        }
        if al == b'\\' {
            unsafe { *dst.add(di) = al }
            di += 1;
            if si >= src.len() {
                unsafe { *dst.add(di) = 0 }
                return;
            }
            al = src[si];
            si += 1;
        }
        if al == 0x05 {
            al = 0xe5;
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

pub fn show() {
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

fn dos_name(ah: u8, name: *const u8) -> u16 {
    let mut eax = u32::from(ah) << 8;
    let edx = name as u32;
    unsafe {
        core::arch::asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            clobber_abi("C"),
        );
    }
    eax as u16
}

fn dos_cwd(drive: u8) -> u16 {
    let mut eax = 0x4700u32;
    let mut buf = [0u8; 8];
    let esi = buf.as_mut_ptr() as u32;
    let edx = u32::from(drive);
    unsafe {
        core::arch::asm!(
            "mov esi, {p:e}",
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            p = in(reg) esi,
            clobber_abi("C"),
        );
    }
    eax as u16
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

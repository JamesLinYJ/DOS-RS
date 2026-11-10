//! `CATALOG` and `ERASE` from `CMD/COMMAND/TCMD1A.ASM` and `TCMD1B.ASM`.
//!
//! The root starts as the boot image. `DIR` prints message 1048 and the
//! live 8.3 names. `DEL` and `ERASE` mark a matching name deleted.

use crate::bios::mscon::con_write_bytes;

const IMAGE: &[u8] = include_bytes!("../../../disks/msload.img");

#[derive(Clone, Copy)]
struct Ent {
    name: [u8; 11],
    live: bool,
    dir: bool,
}

static mut ROOT: [Ent; 16] = [Ent {
    name: [0; 11],
    live: false,
    dir: false,
}; 16];
static mut READY: bool = false;
static mut CWD: [u8; 32] = *b"A:\\\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
static mut CWD_LEN: usize = 3;

fn ensure() {
    unsafe {
        if READY {
            return;
        }
        let root = 3 * 512;
        let mut index = 0usize;
        while index < 16 {
            let entry = &IMAGE[root + index * 32..root + index * 32 + 32];
            if entry[0] == 0 {
                break;
            }
            if entry[0] != 0xe5 && entry[11] & 0x08 == 0 {
                let mut name = [0u8; 11];
                name.copy_from_slice(&entry[..11]);
                ROOT[index].name = name;
                ROOT[index].live = true;
            }
            index += 1;
        }
        READY = true;
    }
}

pub fn dir_cmd(tail: &[u8]) {
    if !blank(tail) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    ensure();
    con_write_bytes(b"Directory of  ");
    con_write_bytes(unsafe { &CWD[..CWD_LEN] });
    con_write_bytes(b"\r\n");
    if unsafe { CWD_LEN != 3 } {
        return;
    }
    let mut index = 0usize;
    while index < 16 {
        let (live, name, dir) = unsafe { (ROOT[index].live, ROOT[index].name, ROOT[index].dir) };
        if live {
            con_write_bytes(&name[..8]);
            con_write_bytes(b" ");
            if dir {
                con_write_bytes(b"<DIR>");
            } else {
                con_write_bytes(&name[8..11]);
            }
            con_write_bytes(b"\r\n");
        }
        index += 1;
    }
}

pub fn mkdir_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() || !blank(skip_token(tail)) {
        con_write_bytes(b"Invalid directory\r\n");
        return;
    }
    let Some(packed) = pack_name(name) else {
        con_write_bytes(b"Invalid directory\r\n");
        return;
    };
    ensure();
    if find_live(&packed).is_some() {
        con_write_bytes(b"Directory already exists\r\n");
        return;
    }
    let mut index = 0usize;
    while index < 16 {
        if unsafe { !ROOT[index].live } {
            unsafe {
                ROOT[index].name = packed;
                ROOT[index].live = true;
                ROOT[index].dir = true;
            }
            return;
        }
        index += 1;
    }
    con_write_bytes(b"Unable to create directory\r\n");
}

pub fn chdir_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() || name == b"\\" {
        unsafe {
            CWD[..3].copy_from_slice(b"A:\\");
            CWD_LEN = 3;
        }
        return;
    }
    let Some(packed) = pack_name(name) else {
        con_write_bytes(b"Invalid directory\r\n");
        return;
    };
    ensure();
    let Some(index) = find_live(&packed) else {
        con_write_bytes(b"Invalid directory\r\n");
        return;
    };
    if unsafe { !ROOT[index].dir } {
        con_write_bytes(b"Invalid directory\r\n");
        return;
    }
    unsafe {
        CWD[..3].copy_from_slice(b"A:\\");
        let n = packed.len().min(8);
        let mut end = 0usize;
        while end < n && packed[end] != b' ' {
            CWD[3 + end] = packed[end];
            end += 1;
        }
        CWD_LEN = 3 + end;
    }
}

pub fn rmdir_cmd(tail: &[u8]) {
    let name = token(tail);
    let Some(packed) = pack_name(name) else {
        con_write_bytes(b"Invalid path, not directory,\r\nor directory not empty\r\n");
        return;
    };
    ensure();
    if unsafe { CWD_LEN > 3 } {
        let cur = unsafe { &CWD[3..CWD_LEN] };
        if cur.eq_ignore_ascii_case(&trim_name(&packed)) {
            con_write_bytes(b"Invalid path, not directory,\r\nor directory not empty\r\n");
            return;
        }
    }
    let Some(index) = find_live(&packed) else {
        con_write_bytes(b"Invalid path, not directory,\r\nor directory not empty\r\n");
        return;
    };
    if unsafe { !ROOT[index].dir } {
        con_write_bytes(b"Invalid path, not directory,\r\nor directory not empty\r\n");
        return;
    }
    unsafe { ROOT[index].live = false }
}

fn find_live(packed: &[u8; 11]) -> Option<usize> {
    let mut index = 0usize;
    while index < 16 {
        if unsafe { ROOT[index].live && ROOT[index].name.eq_ignore_ascii_case(packed) } {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn trim_name(packed: &[u8; 11]) -> &[u8] {
    let mut end = 8usize;
    while end > 0 && packed[end - 1] == b' ' {
        end -= 1;
    }
    &packed[..end]
}

pub fn copy_cmd(tail: &[u8]) {
    let src = token(tail);
    let rest = skip_token(tail);
    let dst = token(rest);
    if src.is_empty() || dst.is_empty() || !blank(skip_token(rest)) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    let (Some(from), Some(to)) = (pack_name(src), pack_name(dst)) else {
        con_write_bytes(b"File not found\r\n");
        return;
    };
    if from.eq_ignore_ascii_case(&to) {
        con_write_bytes(b"File cannot be copied onto itself\r\n");
        return;
    }
    ensure();
    let mut found = false;
    let mut index = 0usize;
    while index < 16 {
        if unsafe { ROOT[index].live && ROOT[index].name.eq_ignore_ascii_case(&from) } {
            found = true;
        }
        if unsafe { ROOT[index].live && ROOT[index].name.eq_ignore_ascii_case(&to) } {
            con_write_bytes(b"File cannot be copied onto itself\r\n");
            return;
        }
        index += 1;
    }
    if !found {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    index = 0;
    while index < 16 {
        if unsafe { !ROOT[index].live } {
            unsafe {
                ROOT[index].name = to;
                ROOT[index].live = true;
                ROOT[index].dir = false;
            }
            return;
        }
        index += 1;
    }
    con_write_bytes(b"File creation error\r\n");
}

pub fn rename_cmd(tail: &[u8]) {
    let src = token(tail);
    let rest = skip_token(tail);
    let dst = token(rest);
    if src.is_empty() || dst.is_empty() || !blank(skip_token(rest)) {
        con_write_bytes(b"Duplicate file name or file not found\r\n");
        return;
    }
    let (Some(from), Some(to)) = (pack_name(src), pack_name(dst)) else {
        con_write_bytes(b"Duplicate file name or file not found\r\n");
        return;
    };
    ensure();
    let mut index = 0usize;
    while index < 16 {
        let live = unsafe { ROOT[index].live };
        if live && unsafe { ROOT[index].name.eq_ignore_ascii_case(&to) } {
            con_write_bytes(b"Duplicate file name or file not found\r\n");
            return;
        }
        index += 1;
    }
    index = 0;
    while index < 16 {
        let live = unsafe { ROOT[index].live };
        if live && unsafe { ROOT[index].name.eq_ignore_ascii_case(&from) } {
            unsafe { ROOT[index].name = to }
            return;
        }
        index += 1;
    }
    con_write_bytes(b"Duplicate file name or file not found\r\n");
}

pub fn erase_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() || !blank(skip_token(tail)) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    let Some(packed) = pack_name(name) else {
        con_write_bytes(b"File not found\r\n");
        return;
    };
    ensure();
    let mut index = 0usize;
    while index < 16 {
        let live = unsafe { ROOT[index].live };
        if live && unsafe { ROOT[index].name.eq_ignore_ascii_case(&packed) } {
            unsafe { ROOT[index].live = false }
            return;
        }
        index += 1;
    }
    con_write_bytes(b"File not found\r\n");
}

fn pack_name(name: &[u8]) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let mut dot = name.len();
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    if dot == 0 || dot > 8 {
        return None;
    }
    let mut n = 0usize;
    while n < dot {
        out[n] = up(name[n]);
        n += 1;
    }
    if dot < name.len() {
        let mut e = 0usize;
        while e < 3 && dot + 1 + e < name.len() {
            out[8 + e] = up(name[dot + 1 + e]);
            e += 1;
        }
    }
    Some(out)
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - 0x20
    } else {
        byte
    }
}

fn token(tail: &[u8]) -> &[u8] {
    let tail = skip_space(tail);
    let mut end = 0usize;
    while end < tail.len()
        && tail[end] != b' '
        && tail[end] != b'\t'
        && tail[end] != b'\r'
        && tail[end] != b'\n'
        && tail[end] != 0
    {
        end += 1;
    }
    &tail[..end]
}

fn skip_token(tail: &[u8]) -> &[u8] {
    let tail = skip_space(tail);
    let mut end = 0usize;
    while end < tail.len()
        && tail[end] != b' '
        && tail[end] != b'\t'
        && tail[end] != b'\r'
        && tail[end] != b'\n'
        && tail[end] != 0
    {
        end += 1;
    }
    &tail[end..]
}

fn skip_space(tail: &[u8]) -> &[u8] {
    let mut i = 0usize;
    while i < tail.len() && (tail[i] == b' ' || tail[i] == b'\t') {
        i += 1;
    }
    &tail[i..]
}

fn blank(tail: &[u8]) -> bool {
    for &byte in tail {
        if byte == b'\r' || byte == b'\n' || byte == 0 {
            return true;
        }
        if byte != b' ' && byte != b'\t' {
            return false;
        }
    }
    true
}

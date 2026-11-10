//! `CATALOG` and `ERASE` from `CMD/COMMAND/TCMD1A.ASM` and `TCMD1B.ASM`.
//!
//! The names come from INT 21h AH=4Eh and AH=4Fh. Delete, rename, copy,
//! and the directory calls are the matching DOS functions.

use crate::bios::mscon::con_write_bytes;

pub fn name_exists(name: &[u8]) -> bool {
    let mut named = [0u8; 16];
    let n = name.len().min(15);
    named[..n].copy_from_slice(&name[..n]);
    let mut dta = [0u8; 48];
    set_dta(dta.as_mut_ptr());
    dos_dx(0x4e, named.as_ptr(), 0x10) == 0
}

pub fn cwd_text() -> &'static [u8] {
    crate::dos::search::cwd_text()
}

pub fn dir_cmd(tail: &[u8]) {
    if !blank(tail) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    let mut cwd = [0u8; 32];
    let _ = dos_si(0x47, cwd.as_mut_ptr());
    con_write_bytes(b"Directory of  A:");
    con_write_bytes(asciiz(&cwd));
    con_write_bytes(b"\r\n");
    if asciiz(&cwd) != b"\\" {
        return;
    }
    let mut dta = [0u8; 48];
    set_dta(dta.as_mut_ptr());
    let spec = b"*.*\0";
    let mut code = dos_dx(0x4e, spec.as_ptr(), 0x10);
    while code == 0 {
        print_dta(&dta);
        code = dos_ax(0x4f);
    }
}

pub fn mkdir_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() || !blank(skip_token(tail)) {
        con_write_bytes(b"Invalid directory\r\n");
        return;
    }
    let named = asciiz_token(name);
    let code = dos_dx(0x39, named.as_ptr(), 0);
    if code == 5 {
        con_write_bytes(b"Directory already exists\r\n");
    } else if code != 0 {
        con_write_bytes(b"Unable to create directory\r\n");
    }
}

pub fn chdir_cmd(tail: &[u8]) {
    let name = token(tail);
    let named = if name.is_empty() { asciiz_token(b"\\") } else { asciiz_token(name) };
    if dos_dx(0x3b, named.as_ptr(), 0) != 0 {
        con_write_bytes(b"Invalid directory\r\n");
    }
}

pub fn rmdir_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() {
        con_write_bytes(b"Invalid path, not directory,\r\nor directory not empty\r\n");
        return;
    }
    let named = asciiz_token(name);
    if dos_dx(0x3a, named.as_ptr(), 0) != 0 {
        con_write_bytes(b"Invalid path, not directory,\r\nor directory not empty\r\n");
    }
}

pub fn copy_cmd(tail: &[u8]) {
    let src = token(tail);
    let rest = skip_token(tail);
    let dst = token(rest);
    if src.is_empty() || dst.is_empty() || !blank(skip_token(rest)) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    if src.eq_ignore_ascii_case(dst) {
        con_write_bytes(b"File cannot be copied onto itself\r\n");
        return;
    }
    let source = asciiz_token(src);
    let dest = asciiz_token(dst);
    let mut dta = [0u8; 48];
    set_dta(dta.as_mut_ptr());
    if dos_dx(0x4e, source.as_ptr(), 0x10) != 0 {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    if dos_dx(0x4e, dest.as_ptr(), 0x10) == 0 {
        con_write_bytes(b"File cannot be copied onto itself\r\n");
        return;
    }
    let src_handle = open_read(source.as_ptr());
    let mut buf = [0u8; 32];
    let got = if src_handle == 0xffff {
        0
    } else {
        read_bytes(src_handle, buf.as_mut_ptr(), buf.len())
    };
    if dos_dx(0x3c, dest.as_ptr(), 0) != 0 {
        con_write_bytes(b"File creation error\r\n");
        return;
    }
    if got != 0 {
        let _ = write_bytes(5, buf.as_ptr(), got);
    }
}

pub fn rename_cmd(tail: &[u8]) {
    let src = token(tail);
    let rest = skip_token(tail);
    let dst = token(rest);
    if src.is_empty() || dst.is_empty() || !blank(skip_token(rest)) {
        con_write_bytes(b"Duplicate file name or file not found\r\n");
        return;
    }
    let source = asciiz_token(src);
    let dest = asciiz_token(dst);
    if dos_di(0x56, source.as_ptr(), dest.as_ptr()) != 0 {
        con_write_bytes(b"Duplicate file name or file not found\r\n");
    }
}

pub fn erase_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() || !blank(skip_token(tail)) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    let named = asciiz_token(name);
    if dos_dx(0x41, named.as_ptr(), 0) != 0 {
        con_write_bytes(b"File not found\r\n");
    }
}

fn print_dta(dta: &[u8]) {
    let attr = dta[0x15];
    let name = asciiz(&dta[0x1e..]);
    let mut base = [b' '; 8];
    let mut ext = [b' '; 3];
    let mut dot = name.len();
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    let mut n = 0usize;
    while n < dot && n < 8 {
        base[n] = name[n];
        n += 1;
    }
    if dot < name.len() {
        let mut e = 0usize;
        while e < 3 && dot + 1 + e < name.len() {
            ext[e] = name[dot + 1 + e];
            e += 1;
        }
    }
    con_write_bytes(&base);
    con_write_bytes(b" ");
    if attr & 0x10 != 0 {
        con_write_bytes(b"<DIR>");
    } else {
        con_write_bytes(&ext);
    }
    let time = u16::from_le_bytes([dta[0x16], dta[0x17]]);
    let mut date = u16::from_le_bytes([dta[0x18], dta[0x19]]);
    if date == 0 {
        date = 0x0021;
    }
    con_write_bytes(b" ");
    put_stamp(time, date);
    con_write_bytes(b"\r\n");
}

fn put_stamp(time: u16, date: u16) {
    let month = ((date >> 5) & 0x0f) as u8;
    let day = (date & 0x1f) as u8;
    let year = ((date >> 9) as u8).wrapping_add(80);
    put_two(month);
    con_write_bytes(b"-");
    put_two(day);
    con_write_bytes(b"-");
    put_two(year);
    con_write_bytes(b" ");
    put_two(((time >> 11) & 0x1f) as u8);
    con_write_bytes(b":");
    put_two(((time >> 5) & 0x3f) as u8);
}

fn put_two(value: u8) {
    con_write_bytes(&[b'0' + value / 10, b'0' + value % 10]);
}

/// `ATTRIB` from the external utility, driven by INT 21h AH=43h.
///
/// A bare name prints the attribute letters. `+R` sets the read-only bit.
pub fn attrib_cmd(tail: &[u8]) {
    let tail = skip_blank(tail);
    let (mask_on, mask_off, name) = if tail.first() == Some(&b'+') || tail.first() == Some(&b'-') {
        let set = tail[0] == b'+';
        let bit = match tail.get(1).copied().unwrap_or(0).to_ascii_uppercase() {
            b'R' => 0x01u8,
            b'A' => 0x20,
            b'H' => 0x02,
            b'S' => 0x04,
            _ => {
                con_write_bytes(b"Invalid parameter\r\n");
                return;
            }
        };
        let name = token(skip_token(tail));
        if set {
            (bit, 0, name)
        } else {
            (0, bit, name)
        }
    } else {
        (0, 0, token(tail))
    };
    if name.is_empty() {
        con_write_bytes(b"Parameter format not correct\r\n");
        return;
    }
    let named = asciiz_token(name);
    let Some(attr) = attr_get(named.as_ptr()) else {
        con_write_bytes(b"File not found\r\n");
        return;
    };
    let attr = (attr | mask_on) & !mask_off;
    if mask_on != 0 || mask_off != 0 {
        let _ = attr_set(named.as_ptr(), attr);
    }
    con_write_bytes(b"ATTR ");
    if attr & 0x20 != 0 {
        con_write_bytes(b"A");
    }
    if attr & 0x01 != 0 {
        con_write_bytes(b"R");
    }
    con_write_bytes(b" ");
    con_write_bytes(name);
    con_write_bytes(b"\r\n");
}

/// Open the file, store CX:DX through AH=57h AL=1, then list the directory.
pub fn stamp_command() {
    let name = b"COMMAND.COM\0";
    let handle = open_read(name.as_ptr());
    if handle == 0xffff {
        return;
    }
    // 1984-03-04 01:02. DOS date (4<<9)|(3<<5)|4, time (1<<11)|(2<<5)|1.
    file_time_set(handle, 0x0841, 0x0864);
}

fn attr_get(name: *const u8) -> Option<u8> {
    let index = crate::dos::search::lookup_asciiz(name)?;
    Some(crate::dos::search::slot(index).attr)
}

fn attr_set(name: *const u8, attr: u8) -> u16 {
    let Some(index) = crate::dos::search::lookup_asciiz(name) else {
        return 2;
    };
    crate::dos::search::set_attr(index, attr);
    0
}

fn file_time_set(handle: u16, time: u16, date: u16) {
    if let Some(index) = crate::dos::handle::opened_index(handle) {
        crate::dos::search::set_stamp(index, time, date);
    }
}

fn skip_blank(tail: &[u8]) -> &[u8] {
    let mut i = 0usize;
    while i < tail.len() && (tail[i] == b' ' || tail[i] == b'\t') {
        i += 1;
    }
    &tail[i..]
}

fn asciiz(buf: &[u8]) -> &[u8] {
    let mut n = 0usize;
    while n < buf.len() && buf[n] != 0 {
        n += 1;
    }
    &buf[..n]
}

fn asciiz_token(name: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    let n = name.len().min(15);
    out[..n].copy_from_slice(&name[..n]);
    out
}

fn open_read(name: *const u8) -> u16 {
    crate::dos::handle::open_asciiz(name).unwrap_or(0xffff)
}

fn read_bytes(handle: u16, dst: *mut u8, count: usize) -> usize {
    crate::dos::handle::read(handle, dst, count).min(count)
}

fn write_bytes(handle: u16, src: *const u8, count: usize) -> usize {
    crate::dos::handle::write(handle, src, count)
}

fn set_dta(ptr: *mut u8) {
    crate::dos::search::set_dta(ptr);
}

fn dos_ax(function: u8) -> u16 {
    if function == 0x4f {
        crate::dos::search::find_next()
    } else {
        1
    }
}

fn dos_dx(function: u8, name: *const u8, attr: u16) -> u16 {
    match function {
        0x1a => {
            crate::dos::search::set_dta(name as *mut u8);
            0
        }
        0x39 => crate::dos::path::mkdir(name),
        0x3a => crate::dos::path::rmdir(name),
        0x3b => crate::dos::path::chdir(name),
        0x3c => crate::dos::file::create(name),
        0x41 => crate::dos::file::delete(name),
        0x4e => crate::dos::search::find_first(name, attr as u8),
        _ => 1,
    }
}

fn dos_si(function: u8, buf: *mut u8) -> u16 {
    if function == 0x47 {
        crate::dos::path::getcwd(buf, 0)
    } else {
        1
    }
}

fn dos_di(function: u8, source: *const u8, dest: *const u8) -> u16 {
    if function == 0x56 {
        crate::dos::file::rename(source, dest)
    } else {
        1
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

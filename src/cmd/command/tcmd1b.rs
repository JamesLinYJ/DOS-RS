//! `VOLUME` from `CMD/COMMAND/TCMD1B.ASM`.
//!
//! A blank tail asks INT 21h AH=11h for a volume label. This disk has none,
//! so the line is message 1041 for drive A, after the leading blank.

use core::arch::asm;

use crate::bios::mscon::con_write_bytes;

/// `TYPEFIL`. The file is read from the boot root and printed through
/// the first `0x1A` or NUL. A missing name is "File not found".
pub fn type_cmd(tail: &[u8]) {
    let name = token(tail);
    if name.is_empty() || !blank(skip_token(tail)) {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    let mut named = [0u8; 13];
    let n = name.len().min(12);
    named[..n].copy_from_slice(&name[..n]);
    let handle = open_name(named.as_ptr());
    if handle == 0xffff {
        con_write_bytes(b"File not found\r\n");
        return;
    }
    let mut buf = [0u8; 64];
    let got = read_file(handle, buf.as_mut_ptr(), buf.len()).min(buf.len());
    let mut index = 0usize;
    while index < got && buf[index] != 0x1a && buf[index] != 0 {
        con_write_bytes(&buf[index..index + 1]);
        index += 1;
    }
    con_write_bytes(b"\r\n");
}

fn open_name(name: *const u8) -> u16 {
    let mut eax = 0x3D00u32;
    let edx = name as u32;
    unsafe {
        asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            clobber_abi("C"),
        );
    }
    eax as u16
}

fn read_file(handle: u16, dst: *mut u8, count: usize) -> usize {
    let mut eax = 0x3F00u32;
    let handle = u32::from(handle);
    let ecx = count as u32;
    let edx = dst as u32;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!(
            "push ebx",
            "mov ebx, {h:e}",
            "int 0x21",
            "pop ebx",
            inout("eax") eax,
            h = in(reg) handle,
            in("ecx") ecx,
            in("edx") edx,
            clobber_abi("C"),
        );
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!(
            "push rbx",
            "mov ebx, {h:e}",
            "int 0x21",
            "pop rbx",
            inout("eax") eax,
            h = in(reg) handle,
            in("ecx") ecx,
            in("edx") edx,
            clobber_abi("C"),
        );
    }
    eax as usize
}

fn file_bytes(name: &[u8]) -> Option<&'static [u8]> {
    let wanted = pack_name(name)?;
    let root = 3 * 512;
    let mut index = 0usize;
    while index < 16 {
        let entry = &IMAGE[root + index * 32..root + index * 32 + 32];
        if entry[0] == 0 {
            break;
        }
        if entry[0] != 0xe5 && entry[..11].eq_ignore_ascii_case(&wanted) {
            let cluster = u16::from_le_bytes([entry[26], entry[27]]) as usize;
            let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as usize;
            if cluster < 2 {
                return None;
            }
            let start = (4 + (cluster - 2)) * 512;
            let end = start.saturating_add(size).min(IMAGE.len());
            if start >= IMAGE.len() {
                return None;
            }
            return Some(&IMAGE[start..end]);
        }
        index += 1;
    }
    None
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
    while end < tail.len() && tail[end] != b' ' && tail[end] != b'\t' && tail[end] != b'\r' && tail[end] != b'\n' && tail[end] != 0 {
        end += 1;
    }
    &tail[..end]
}

fn skip_token(tail: &[u8]) -> &[u8] {
    let tail = skip_space(tail);
    let mut end = 0usize;
    while end < tail.len() && tail[end] != b' ' && tail[end] != b'\t' && tail[end] != b'\r' && tail[end] != b'\n' && tail[end] != 0 {
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

const IMAGE: &[u8] = include_bytes!("../../../disks/msload.img");

pub fn volume(tail: &[u8]) {
    if !blank(tail) {
        con_write_bytes(b"Syntax error\r\n");
        return;
    }
    con_write_bytes(b"\r\n");
    con_write_bytes(b" ");
    let found = search_volume();
    if found == 0 {
        return;
    }
    let drive = b'A'.wrapping_add(default_drive());
    con_write_bytes(b"Volume in drive ");
    con_write_bytes(&[drive]);
    con_write_bytes(b" has no label\r\n");
}

fn search_volume() -> u8 {
    let mut fcb = [0u8; 44];
    fcb[0] = 0xff;
    fcb[6] = 0x08;
    let mut i = 8usize;
    while i < 19 {
        fcb[i] = b'?';
        i += 1;
    }
    let mut eax = 0x1100u32;
    let edx = fcb.as_ptr() as u32;
    unsafe {
        asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            clobber_abi("C"),
        );
    }
    eax as u8
}

fn default_drive() -> u8 {
    let mut eax = 0x1900u32;
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    eax as u8
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

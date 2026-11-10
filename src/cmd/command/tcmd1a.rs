//! `CATALOG` and `ERASE` from `CMD/COMMAND/TCMD1A.ASM` and `TCMD1B.ASM`.
//!
//! The names come from INT 21h AH=4Eh and AH=4Fh. Delete, rename, copy,
//! and the directory calls are the matching DOS functions.

use core::arch::asm;

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
    con_write_bytes(b"\r\n");
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

fn read_bytes(handle: u16, dst: *mut u8, count: usize) -> usize {
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
    (eax as usize).min(count)
}

fn write_bytes(handle: u16, src: *const u8, count: usize) -> usize {
    let mut eax = 0x4000u32;
    let handle = u32::from(handle);
    let ecx = count as u32;
    let edx = src as u32;
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

fn set_dta(ptr: *mut u8) {
    let _ = dos_dx(0x1a, ptr as *const u8, 0);
}

fn dos_ax(ah: u8) -> u16 {
    let mut eax = u32::from(ah) << 8;
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    eax as u16
}

fn dos_dx(ah: u8, dx: *const u8, cx: u16) -> u16 {
    let mut eax = u32::from(ah) << 8;
    let edx = dx as u32;
    let ecx = u32::from(cx);
    unsafe {
        asm!(
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            in("ecx") ecx,
            clobber_abi("C"),
        );
    }
    eax as u16
}

fn dos_si(ah: u8, si: *mut u8) -> u16 {
    let mut eax = u32::from(ah) << 8;
    let ptr = si as u32;
    unsafe {
        asm!(
            "mov esi, {p:e}",
            "int 0x21",
            inout("eax") eax,
            p = in(reg) ptr,
            clobber_abi("C"),
        );
    }
    eax as u16
}

fn dos_di(ah: u8, dx: *const u8, di: *const u8) -> u16 {
    let mut eax = u32::from(ah) << 8;
    let edx = dx as u32;
    let dest = di as u32;
    unsafe {
        asm!(
            "mov edi, {d:e}",
            "int 0x21",
            inout("eax") eax,
            in("edx") edx,
            d = in(reg) dest,
            clobber_abi("C"),
        );
    }
    eax as u16
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

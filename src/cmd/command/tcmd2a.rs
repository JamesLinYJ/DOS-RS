//! Default prompt from `CMD/COMMAND/TCMD2A.ASM`.
//!
//! `print_prompt` looks for a `PROMPT=` value. This environment has none,
//! so it prints the default drive letter from INT 21h AH=19h and `>`
//! (`SYM` in `COMEQU.ASM`).

use core::arch::asm;

const SYM: u8 = b'>';

/// `VERSION` from this file. CRLF, the version line, CRLF.
///
/// INT 21h AH=30h returns the major version in AL and the minor in AH.
/// Message 1040 substitutes those two numbers.
pub fn version() {
    let (major, minor) = dos_version();
    crate::bios::mscon::con_write_bytes(b"\r\n");
    crate::bios::mscon::con_write_bytes(super::tdata::VER_MESSAGE);
    put_dec(major);
    crate::bios::mscon::con_write_bytes(b".");
    put_dec(minor);
    crate::bios::mscon::con_write_bytes(b"\r\n\r\n");
}

fn dos_version() -> (u8, u8) {
    let mut eax: u32 = 0x3000;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    ((eax & 0xff) as u8, ((eax >> 8) & 0xff) as u8)
}

fn put_dec(mut value: u8) {
    let mut buf = [0u8; 3];
    let mut n = 0usize;
    if value == 0 {
        crate::bios::mscon::con_write_bytes(b"0");
        return;
    }
    while value != 0 {
        buf[n] = b'0' + (value % 10);
        n += 1;
        value /= 10;
    }
    while n != 0 {
        n -= 1;
        crate::bios::mscon::con_write_bytes(&buf[n..n + 1]);
    }
}

/// `PATH` from `TCMD2A.ASM`. A blank tail is `disppath`. Anything else is
/// parsed into the environment value after `PATH=`.
///
/// The environment block in `ENVDATA.ASM` starts as `PATH=` plus a NUL.
/// This slot is that value. A second token is "Too many parameters".
static mut PATH_VALUE: [u8; 67] = [0; 67];
static mut PATH_LEN: usize = 0;

pub fn path_cmd(tail: &[u8]) {
    let tail = skip_space(tail);
    if path_tail_blank(tail) {
        print_path();
        return;
    }
    let mut end = 0usize;
    while end < tail.len() {
        let byte = tail[end];
        if byte == b' ' || byte == b'\t' || byte == b'\r' || byte == b'\n' || byte == 0 {
            break;
        }
        end += 1;
    }
    if !path_tail_blank(skip_space(&tail[end..])) {
        crate::bios::mscon::con_write_bytes(b"Too many parameters\r\n");
        return;
    }
    let value = &tail[..end];
    unsafe {
        let n = value.len().min(PATH_VALUE.len());
        PATH_VALUE[..n].copy_from_slice(&value[..n]);
        PATH_LEN = n;
    }
}

pub fn copy_path(out: &mut [u8]) -> usize {
    let len = unsafe { PATH_LEN };
    let n = len.min(out.len());
    out[..n].copy_from_slice(unsafe { &PATH_VALUE[..n] });
    n
}

fn print_path() {
    let len = unsafe { PATH_LEN };
    if len == 0 {
        crate::bios::mscon::con_write_bytes(b"No Path\r\n");
        return;
    }
    crate::bios::mscon::con_write_bytes(b"PATH=");
    crate::bios::mscon::con_write_bytes(unsafe { &PATH_VALUE[..len] });
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn skip_space(tail: &[u8]) -> &[u8] {
    let mut i = 0usize;
    while i < tail.len() && (tail[i] == b' ' || tail[i] == b'\t') {
        i += 1;
    }
    &tail[i..]
}

/// `CLS` from `TCMD2A.ASM`. INT 2Fh AH=1Ah asks if ANSI.SYS is installed.
/// It is not, so the clear is INT 10h: border, scroll, cursor home.
///
/// The serial line has no video page. `CLS` is the marker that those calls
/// returned.
pub fn cls() {
    if ansi_installed() == 0xff {
        return;
    }
    video(0x0f00);
    video(0x0b00);
    video(0x0600);
    video(0x0200);
    crate::bios::mscon::con_write_bytes(b"CLS\r\n");
}

fn ansi_installed() -> u8 {
    let mut eax: u32 = 0x1a00;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x2f", inout("eax") eax, clobber_abi("C"));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x2f", inout("eax") eax, clobber_abi("C"));
    }
    (eax & 0xff) as u8
}

fn video(mut eax: u32) {
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x10", inout("eax") eax, clobber_abi("C"));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x10", inout("eax") eax, clobber_abi("C"));
    }
}

#[cfg(target_arch = "x86")]
core::arch::global_asm!(
    r#"
.section .text, "ax"
.code32
.global int2f_gate
int2f_gate:
    xor al, al
    iretd
"#
);

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    r#"
.section .text, "ax"
.code64
.global int2f_gate
int2f_gate:
    xor al, al
    iretq
"#
);

unsafe extern "C" {
    fn int2f_gate();
}

pub fn int2f_gate_addr() -> usize {
    int2f_gate as *const () as usize
}

fn path_tail_blank(tail: &[u8]) -> bool {
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

/// One trip through `print_prompt` when no `PROMPT` string is set.
pub fn print_default_prompt() {
    let drive = default_drive();
    let letter = b'A'.wrapping_add(drive);
    crate::bios::mscon::con_write_bytes(&[letter, SYM]);
}

fn default_drive() -> u8 {
    let mut eax: u32 = 0x1900;
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        asm!("int 0x21", inout("eax") eax, clobber_abi("C"));
    }
    (eax & 0xff) as u8
}

//! `$IOCTL` from `DOS/IOCTL.ASM`.
//!
//! AL=0 reads device flags for a handle. A closed handle is
//! `error_invalid_handle` (6). A disk file has no device bit, so DX is 0.
//! AL=8 asks if a block drive is removable. Drive 1 is the boot floppy.
//! Drive 3 is treated as a fixed disk. Drive 0 is not a valid unit here.

use core::arch::asm;

const INVALID_HANDLE: u16 = 6;
const INVALID_DRIVE: u16 = 15;

pub fn query(al: u8, handle: u16, drive: u8) -> (u16, u16, u8) {
    let mut eax = 0x4400u32 | u32::from(al);
    let mut edx = 0u32;
    let ebx = if al == 8 { u32::from(drive) } else { u32::from(handle) };
    #[cfg(target_arch = "x86")]
    unsafe {
        asm!(
            "push ebx",
            "mov ebx, {h:e}",
            "int 0x21",
            "pop ebx",
            inout("eax") eax,
            inout("edx") edx,
            h = in(reg) ebx,
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
            inout("edx") edx,
            h = in(reg) ebx,
            clobber_abi("C"),
        );
    }
    (eax as u16, edx as u16, eax as u8)
}

pub fn on_frame(al: u8, bx: u16) -> (u16, u16, u8) {
    if al == 0 {
        return match crate::dos::handle::opened_index(bx) {
            Some(_) => (0, 0, 0),
            None => (INVALID_HANDLE, 0, 0),
        };
    }
    if al == 8 {
        let drive = bx as u8;
        if drive == 1 {
            return (0, 0, 0);
        }
        if drive == 3 {
            return (1, 0, 1);
        }
        return (INVALID_DRIVE, 0, 0);
    }
    (1, 0, 0)
}

pub fn show() {
    let (closed, _, _) = query(0, 5, 0);
    let (_, _, removable) = query(8, 0, 1);
    let (_, _, fixed) = query(8, 0, 3);
    crate::bios::mscon::con_write_bytes(b"IOCTL H");
    put_hex8(closed as u8);
    crate::bios::mscon::con_write_bytes(b" REM");
    put_hex8(removable);
    crate::bios::mscon::con_write_bytes(b" FIX");
    put_hex8(fixed);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[HEX[(value >> 4) as usize], HEX[(value & 0xf) as usize]]);
}

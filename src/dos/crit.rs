//! Critical sections from `DOS/CRIT.ASM`.
//!
//! Each handler begins with `RET`. The sharer patches that into `PUSH AX`.
//! After the patch, enter sends INT 2Fh with AX = `8000h` plus the section,
//! and leave sends `8100h` plus the section. Disk, memory, and the SFT share
//! section 1. Devices use section 2.

const CRIT_DISK: u16 = 1;
const CRIT_DEVICE: u16 = 2;

use core::sync::atomic::{AtomicBool, Ordering};

static PATCHED: AtomicBool = AtomicBool::new(false);

fn section(base: u16, which: u16) -> u16 {
    if !PATCHED.load(Ordering::Relaxed) {
        return 0;
    }
    base + which
}

/// `EcritDisk`, also `EcritMem` and `EcritSFT`.
pub fn enter_disk() -> u16 {
    section(0x8000, CRIT_DISK)
}

/// `LcritDisk`, also `LcritMem` and `LcritSFT`.
pub fn leave_disk() -> u16 {
    section(0x8100, CRIT_DISK)
}

/// `EcritDevice`.
pub fn enter_device() -> u16 {
    section(0x8000, CRIT_DEVICE)
}

/// `LcritDevice`.
pub fn leave_device() -> u16 {
    section(0x8100, CRIT_DEVICE)
}

/// The sharer replaces the leading `RET` with `PUSH AX`.
pub fn sharer_patch() {
    PATCHED.store(true, Ordering::Relaxed);
}

pub fn show() {
    let quiet = enter_disk();
    sharer_patch();
    let enter = enter_disk();
    let leave = leave_disk();
    let dev_enter = enter_device();
    let dev_leave = leave_device();
    if quiet != 0 {
        crate::bios::mscon::con_write_bytes(b"CRITFAIL\r\n");
        return;
    }
    crate::bios::mscon::con_write_bytes(b"CRIT ");
    put_hex16(enter);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(leave);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(dev_enter);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(dev_leave);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[((value >> 12) & 0xf) as usize],
        hex[((value >> 8) & 0xf) as usize],
        hex[((value >> 4) & 0xf) as usize],
        hex[(value & 0xf) as usize],
    ]);
}

//! Initialization from `CMD/DISKCOPY/COPYINIT.ASM`.
//!
//! There is no 386 patch. Twelve substitution lists are filled from the
//! code segment. Drive zero means the current drive. A nonzero loader word
//! is an invalid drive. Redirect, server, and removable checks stop at the
//! first failure. A 720 KB drive copies only to another 720 KB drive. The
//! buffer begins on the next 512-byte boundary after the init image.

const FINE: u16 = 0xFFFF;
const INVALID: u16 = 5;
const REDIRECTED: u16 = 6;
const INCOMPATIBLE: u16 = 16;
const LETTER_BASE: u8 = 0x40;
const REMOTE: u16 = 0x1000;
const FIXED_BIT: u16 = 0x0001;
const DRIVE_720: u8 = 2;
const SUBLISTS: u8 = 12;

struct Drives {
    source: u8,
    target: u8,
    first: u8,
    second: u8,
}

fn assign(source: u8, target: u8, current: u8) -> Drives {
    let logical = current.wrapping_add(1);
    let (source, target) = if source == 0 {
        (logical, logical)
    } else if target == 0 {
        (source, logical)
    } else {
        (source, target)
    };
    Drives {
        source,
        target,
        first: LETTER_BASE.wrapping_add(source),
        second: LETTER_BASE.wrapping_add(target),
    }
}

fn loader_status(valid: u16) -> u16 {
    if valid != 0 { INVALID } else { FINE }
}

fn redirect(status: u16, failed: bool, attribute: u16) -> u16 {
    if failed {
        INVALID
    } else if attribute & REMOTE != 0 {
        REDIRECTED
    } else {
        status
    }
}

fn server(status: u16, present: bool, drive: u8, shared_failed: bool) -> (u16, u8) {
    if !present {
        return (FINE, 0);
    }
    let letter = drive.wrapping_sub(1).wrapping_add(b'A');
    let status = if shared_failed { REDIRECTED } else { status };
    (status, letter)
}

fn removable(source_ready: bool, source_kind: u16, target_ready: bool, target_kind: u16) -> u16 {
    if !source_ready || source_kind != 0 {
        return INVALID;
    }
    if !target_ready || target_kind != 0 { INVALID } else { FINE }
}

fn compatible(source_attr: u16, target_attr: u16, source_kind: u8, target_kind: u8) -> u16 {
    if source_attr & FIXED_BIT != 0 || target_attr & FIXED_BIT != 0 {
        return INVALID;
    }
    if source_kind == DRIVE_720 {
        if target_kind == DRIVE_720 { FINE } else { INCOMPATIBLE }
    } else if target_kind == DRIVE_720 {
        INCOMPATIBLE
    } else {
        FINE
    }
}

fn copy_kind(source: u8, target: u8, shared: bool) -> (u8, u8) {
    let owner = if shared { target } else { source };
    if owner != source {
        (1, source)
    } else if owner == target {
        (1, target)
    } else {
        (2, target)
    }
}

fn buffer(origin: u16, code: u16, top: u16) -> (u16, u16) {
    let mut start = origin.wrapping_add(1024) >> 4;
    start = start.wrapping_add(code);
    let low = (start as u8) & 0xE0;
    start = (start & 0xFF00) | u16::from(low);
    (start.wrapping_add(0x20), top)
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"COPYINIT SL");
    put_hex8(SUBLISTS);
    write_drives(assign(0, 0, 0));
    write_drives(assign(0, 0, 1));
    write_drives(assign(1, 0, 0));
    write_drives(assign(1, 2, 0));
    write_status(loader_status(1));
    write_status(loader_status(0));
    write_status(redirect(FINE, false, 0));
    write_status(redirect(FINE, false, REMOTE));
    write_status(redirect(FINE, true, 0));
    let (status, letter) = server(FINE, false, 1, true);
    write_status(status);
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(letter);
    let (status, letter) = server(FINE, true, 2, true);
    write_status(status);
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(letter);
    let (status, letter) = server(FINE, true, 1, false);
    write_status(status);
    crate::bios::mscon::con_write_bytes(b" LT");
    put_hex8(letter);
    write_status(removable(false, 0, true, 0));
    write_status(removable(true, 1, true, 0));
    write_status(removable(true, 0, false, 0));
    write_status(removable(true, 0, true, 0));
    write_status(compatible(FIXED_BIT, 0, 0, 0));
    write_status(compatible(0, FIXED_BIT, 0, 0));
    write_status(compatible(0, 0, 2, 2));
    write_status(compatible(0, 0, 2, 0));
    write_status(compatible(0, 0, 0, 2));
    write_status(compatible(0, 0, 0, 1));
    write_copy(1, 2, true);
    write_copy(1, 1, false);
    write_copy(1, 2, false);
    let (begin, end) = buffer(0x0100, 0x2000, 0xA000);
    crate::bios::mscon::con_write_bytes(b" BF");
    put_hex16(begin);
    crate::bios::mscon::con_write_bytes(b" BE");
    put_hex16(end);
    crate::bios::mscon::con_write_bytes(b" CB2523\r\n");
}

fn write_drives(drives: Drives) {
    crate::bios::mscon::con_write_bytes(b" S");
    put_hex8(drives.source);
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(drives.target);
    crate::bios::mscon::con_write_bytes(b" L");
    put_hex8(drives.first);
    put_hex8(drives.second);
}

fn write_status(status: u16) {
    crate::bios::mscon::con_write_bytes(b" ST");
    put_hex16(status);
}

fn write_copy(source: u8, target: u8, shared: bool) {
    let (kind, target) = copy_kind(source, target, shared);
    crate::bios::mscon::con_write_bytes(b" CT");
    put_hex8(kind);
    crate::bios::mscon::con_write_bytes(b" T");
    put_hex8(target);
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

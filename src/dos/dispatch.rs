//! Reentrancy notes from `DOS/DISPATCH.ASM`.
//!
//! The INT 21h vectors stay in `int21`. This file records which routine
//! serializes each call, and the critical sections named here: disk 1,
//! device 2, memory 4, and network 5. Section 3 is not in that list.
//! A call with no note is absent. A call whose note is blank is unlisted.
//! `*none*` means the call does not take a critical section.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    Close,
    Read,
    Write,
    ReadWrite,
    Flush,
    Open,
    SearchFirst,
    SearchNext,
    Delete,
    Create,
    Rename,
    FileInfo,
    DeviceIo,
    Abort,
    DiskInfo,
    Mkdir,
    Rmdir,
    Chdir,
    CurrentDir,
    Alloc,
    Dealloc,
    SetBlock,
    InfoAndAttr,
    CreateNew,
    None,
    Unlisted,
    Absent,
}

const SECTIONS: [u8; 4] = [1, 2, 4, 5];

const OWNERS: &[(u8, Guard)] = &[
    (0x00, Guard::Close),
    (0x01, Guard::Read),
    (0x02, Guard::Write),
    (0x03, Guard::Read),
    (0x04, Guard::Write),
    (0x05, Guard::Write),
    (0x06, Guard::ReadWrite),
    (0x07, Guard::Read),
    (0x08, Guard::Read),
    (0x09, Guard::Write),
    (0x0a, Guard::Read),
    (0x0b, Guard::Read),
    (0x0c, Guard::Read),
    (0x0d, Guard::Flush),
    (0x0e, Guard::None),
    (0x0f, Guard::Open),
    (0x10, Guard::Close),
    (0x11, Guard::SearchFirst),
    (0x12, Guard::SearchNext),
    (0x13, Guard::Delete),
    (0x14, Guard::ReadWrite),
    (0x15, Guard::ReadWrite),
    (0x16, Guard::Create),
    (0x17, Guard::Rename),
    (0x19, Guard::None),
    (0x1a, Guard::None),
    (0x1f, Guard::None),
    (0x21, Guard::ReadWrite),
    (0x22, Guard::ReadWrite),
    (0x23, Guard::FileInfo),
    (0x24, Guard::None),
    (0x25, Guard::None),
    (0x26, Guard::None),
    (0x27, Guard::ReadWrite),
    (0x28, Guard::ReadWrite),
    (0x29, Guard::None),
    (0x2a, Guard::DeviceIo),
    (0x2b, Guard::DeviceIo),
    (0x2c, Guard::DeviceIo),
    (0x2d, Guard::DeviceIo),
    (0x2e, Guard::None),
    (0x2f, Guard::None),
    (0x30, Guard::None),
    (0x31, Guard::Abort),
    (0x32, Guard::None),
    (0x33, Guard::None),
    (0x34, Guard::None),
    (0x35, Guard::None),
    (0x36, Guard::DiskInfo),
    (0x37, Guard::None),
    (0x38, Guard::None),
    (0x39, Guard::Mkdir),
    (0x3a, Guard::Rmdir),
    (0x3b, Guard::Chdir),
    (0x3c, Guard::Create),
    (0x3d, Guard::Open),
    (0x3e, Guard::Close),
    (0x3f, Guard::Read),
    (0x40, Guard::Write),
    (0x41, Guard::Delete),
    (0x42, Guard::None),
    (0x43, Guard::InfoAndAttr),
    (0x44, Guard::DeviceIo),
    (0x45, Guard::None),
    (0x46, Guard::None),
    (0x47, Guard::CurrentDir),
    (0x48, Guard::Alloc),
    (0x49, Guard::Dealloc),
    (0x4a, Guard::SetBlock),
    (0x4b, Guard::Unlisted),
    (0x4c, Guard::Abort),
    (0x4d, Guard::None),
    (0x4e, Guard::SearchFirst),
    (0x4f, Guard::SearchNext),
    (0x50, Guard::None),
    (0x51, Guard::None),
    (0x52, Guard::None),
    (0x53, Guard::None),
    (0x54, Guard::None),
    (0x55, Guard::Unlisted),
    (0x56, Guard::Rename),
    (0x57, Guard::None),
    (0x58, Guard::None),
    (0x59, Guard::None),
    (0x5a, Guard::CreateNew),
    (0x5b, Guard::CreateNew),
    (0x5c, Guard::Unlisted),
    (0x5d, Guard::Unlisted),
    (0x5e, Guard::Unlisted),
    (0x5f, Guard::Unlisted),
];

/// True when `id` appears in the critical-section list, scanned in file order.
pub fn section_listed(id: u8) -> bool {
    for section in SECTIONS {
        if section == id {
            return true;
        }
    }
    false
}

/// Who serializes `function`. The table is scanned from call 0 upward.
pub fn owner(function: u8) -> Guard {
    for (call, guard) in OWNERS {
        if *call == function {
            return *guard;
        }
        if *call > function {
            return Guard::Absent;
        }
    }
    Guard::Absent
}

pub fn show() {
    crate::bios::mscon::con_write_bytes(b"DISPATCH DSK");
    put_hex8(SECTIONS[0]);
    crate::bios::mscon::con_write_bytes(b" DEV");
    put_hex8(SECTIONS[1]);
    crate::bios::mscon::con_write_bytes(b" MEM");
    put_hex8(SECTIONS[2]);
    crate::bios::mscon::con_write_bytes(b" NET");
    put_hex8(SECTIONS[3]);
    crate::bios::mscon::con_write_bytes(if section_listed(3) { b" GAPFF" } else { b" GAP03" });
    put_call(0x0e);
    put_call(0x3f);
    put_call(0x40);
    put_call(0x48);
    put_call(0x00);
    put_call(0x2a);
    put_call(0x36);
    put_call(0x18);
    put_call(0x4b);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_call(function: u8) {
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(function);
    crate::bios::mscon::con_write_bytes(b" ");
    crate::bios::mscon::con_write_bytes(label(owner(function)));
}

fn label(guard: Guard) -> &'static [u8] {
    match guard {
        Guard::None => b"NONE",
        Guard::Read => b"READ",
        Guard::Write => b"WRITE",
        Guard::Alloc => b"ALLOC",
        Guard::Close => b"CLOSE",
        Guard::DeviceIo => b"DEVIO",
        Guard::DiskInfo => b"DINFO",
        Guard::Absent => b"MISS",
        Guard::Unlisted => b"BARE",
        _ => b"OTHER",
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

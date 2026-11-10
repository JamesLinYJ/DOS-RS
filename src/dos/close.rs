//! `DOS_CLOSE` from `DOS/CLOSE.ASM`.
//!
//! A clean file is freed without a directory update. A file that was written
//! gets the archive bit, matching the `OR dir_attr, attr_archive` in
//! `CLOSE_GO`. `$Close` in `HANDLE.ASM` is the AH=3Eh entry.

use super::search;

/// Stamp the directory entry after a write. Archive is bit 0x20.
pub fn commit(index: usize) {
    let mut ent = search::slot(index);
    ent.attr |= 0x20;
    if ent.owned {
        ent.size = u16::from(ent.data_len);
    }
    search::set_slot(index, ent);
}

/// Seek within `BAR.COM`, write one byte back, and close it.
pub fn show() {
    let name = b"BAR.COM\0";
    if chmod_set(name.as_ptr(), 0x01) != 0 {
        crate::bios::mscon::con_write_bytes(b"SEEKFAIL\r\n");
        return;
    }
    let handle = open_name(name.as_ptr());
    if handle == 0xffff {
        crate::bios::mscon::con_write_bytes(b"SEEKFAIL\r\n");
        return;
    }
    let (abs_ax, abs_dx) = lseek(handle, 0, 1, 0);
    let (end_ax, end_dx) = lseek(handle, 2, 0, 0);
    let _ = lseek(handle, 0, 0, 0);
    let mut byte = [0u8; 1];
    let _ = read_one(handle, byte.as_mut_ptr());
    let _ = lseek(handle, 0, 0, 0);
    let _ = write_one(handle, byte.as_ptr());
    let closed = close_handle(handle);
    let again = close_handle(handle);
    let attr = chmod_get(name.as_ptr()).unwrap_or(0);
    crate::bios::mscon::con_write_bytes(b"SEEK ");
    put_hex16(abs_dx);
    put_hex16(abs_ax);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex16(end_dx);
    put_hex16(end_ax);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(closed as u8);
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8(again as u8);
    crate::bios::mscon::con_write_bytes(b" AR");
    put_hex8(attr);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn open_name(name: *const u8) -> u16 {
    crate::dos::handle::open_asciiz(name).unwrap_or(0xffff)
}

fn lseek(handle: u16, method: u8, low: u16, high: u16) -> (u16, u16) {
    match crate::dos::handle::seek(handle, method, low, high) {
        Ok(pos) => (pos as u16, (pos >> 16) as u16),
        Err(code) => (code, 0),
    }
}

fn read_one(handle: u16, dst: *mut u8) -> u16 {
    crate::dos::handle::read(handle, dst, 1) as u16
}

fn write_one(handle: u16, src: *const u8) -> u16 {
    crate::dos::handle::write(handle, src, 1) as u16
}

fn close_handle(handle: u16) -> u16 {
    crate::dos::handle::close(handle)
}

fn chmod_get(name: *const u8) -> Option<u8> {
    let index = crate::dos::search::lookup_asciiz(name)?;
    Some(crate::dos::search::slot(index).attr)
}

fn chmod_set(name: *const u8, attr: u8) -> u16 {
    let Some(index) = crate::dos::search::lookup_asciiz(name) else {
        return 2;
    };
    crate::dos::search::set_attr(index, attr);
    0
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

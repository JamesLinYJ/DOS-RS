//! `DOS_DELETE` from `DOS/DELETE.ASM`.
//!
//! A missing name is file-not-found. A directory is access denied. A
//! read-only file is found and left in place unless the search attribute
//! includes read-only. A deleted entry is marked with `E5`.

use super::search;

const FILE_NOT_FOUND: u16 = 2;
const ACCESS: u16 = 5;
const ATTR_READ_ONLY: u8 = 0x01;
const DELETED: u8 = 0xe5;

/// Local delete of one directory entry. The search attribute gates read-only files.
pub fn dos_delete(name: &[u8; 11], search_attr: u8) -> u16 {
    let Some(index) = search::find_packed(name) else {
        return FILE_NOT_FOUND;
    };
    let mut ent = search::slot(index);
    if ent.dir {
        return ACCESS;
    }
    if search_attr & ATTR_READ_ONLY == 0 && ent.attr & ATTR_READ_ONLY != 0 {
        return ACCESS;
    }
    ent.name[0] = DELETED;
    ent.live = false;
    search::set_slot(index, ent);
    0
}

pub fn show() {
    let missing = search::pack(b"ZZZ.COM").unwrap();
    let temp = search::pack(b"TEMP").unwrap();
    let new_name = search::pack(b"NEW.DAT").unwrap();
    let not_found = dos_delete(&missing, 0);
    let directory = dos_delete(&temp, 0);
    let _ = super::dircall::dos_rmdir(b"A:\\TEMP\0");
    let readonly = dos_delete(&new_name, 0);
    let index = search::find_packed(&new_name).unwrap_or(0);
    if search::find_packed(&new_name).is_some() {
        let mut ent = search::slot(index);
        ent.attr &= !ATTR_READ_ONLY;
        search::set_slot(index, ent);
    }
    let removed = dos_delete(&new_name, 0);
    let mark = search::slot(index).name[0];
    crate::bios::mscon::con_write_bytes(b"DELETE NF");
    put_hex8(not_found as u8);
    crate::bios::mscon::con_write_bytes(b" DR");
    put_hex8(directory as u8);
    crate::bios::mscon::con_write_bytes(b" RO");
    put_hex8(readonly as u8);
    crate::bios::mscon::con_write_bytes(b" ");
    put_hex8(mark);
    put_hex8(removed as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

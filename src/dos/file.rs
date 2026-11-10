//! `$RENAME` and delete from `DOS/FILE.ASM`.
//!
//! Delete clears a live root entry. Rename replaces its 11-byte name when
//! the destination is not already there. Create takes the first free slot.

use super::search::{self, Ent};

const FILE_NOT_FOUND: u16 = 2;
const ACCESS: u16 = 5;

pub fn delete(name: *const u8) -> u16 {
    let Some(packed) = packed(name) else {
        return FILE_NOT_FOUND;
    };
    let Some(index) = search::find_packed(&packed) else {
        return FILE_NOT_FOUND;
    };
    let mut ent = search::slot(index);
    ent.live = false;
    search::set_slot(index, ent);
    0
}

pub fn rename(src: *const u8, dst: *const u8) -> u16 {
    let (Some(from), Some(to)) = (packed(src), packed(dst)) else {
        return FILE_NOT_FOUND;
    };
    if search::find_packed(&to).is_some() {
        return ACCESS;
    }
    let Some(index) = search::find_packed(&from) else {
        return FILE_NOT_FOUND;
    };
    let mut ent = search::slot(index);
    ent.name = to;
    search::set_slot(index, ent);
    0
}

pub fn create(name: *const u8) -> u16 {
    let Some(packed) = packed(name) else {
        return FILE_NOT_FOUND;
    };
    if search::find_packed(&packed).is_some() {
        return ACCESS;
    }
    let Some(index) = search::alloc_slot() else {
        return ACCESS;
    };
    search::set_slot(
        index,
        Ent {
            name: packed,
            live: true,
            dir: false,
            owned: true,
            data: [0; 32],
            data_len: 0,
            cluster: 0,
            size: 0,
            attr: 0x20,
            time: 0,
            date: 0x0021,
        },
    );
    crate::dos::handle::bind_owned(index);
    0
}

fn packed(name: *const u8) -> Option<[u8; 11]> {
    let raw = search::read_asciiz(name);
    let len = search::asciiz_len(&raw);
    search::pack(&raw[..len])
}

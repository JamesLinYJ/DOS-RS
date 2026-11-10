//! `DOS_CREATE` and `DOS_CREATE_NEW` from `DOS/CREATE.ASM`.
//!
//! Attributes outside the directory, ignore, and volume bits are rejected.
//! A directory or device attribute is rejected before `MakeNode`. `MakeNode`
//! error 3 is "file exists" when truncate is not allowed, and error 5 is an
//! attribute mismatch. `CRTERRTAB` maps those codes to the DOS errors.

use super::search::{self, Ent};

const ATTR_ALL: u8 = 0x16;
const ATTR_IGNORE: u8 = 0x61;
const ATTR_VOLUME: u8 = 0x08;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_DEVICE: u8 = 0x40;
const ATTR_READ_ONLY: u8 = 0x01;

/// `CRTERRTAB`. Index 0 is unused. The rest are `MakeNode` errors 1 through 7.
const CRT_ERR_TAB: [u16; 8] = [0, 5, 82, 80, 3, 5, 32, 2];

/// `DOS_CREATE` when AH is 0, and `DOS_CREATE_NEW` when truncate is not allowed.
pub fn dos_create(attr: u8, name: &[u8; 11], truncate_ok: bool) -> Result<u8, u16> {
    let mut result = attr;
    if result & !(ATTR_ALL | ATTR_IGNORE | ATTR_VOLUME) != 0 {
        return Err(5);
    }
    if result & ATTR_VOLUME != 0 {
        result = ATTR_VOLUME;
    }
    result |= ATTR_ARCHIVE;
    if result & (ATTR_DIRECTORY | ATTR_DEVICE) != 0 {
        return Err(5);
    }
    match make_node(result, name, truncate_ok) {
        Ok(made) => Ok(made),
        Err(code) => Err(CRT_ERR_TAB[code as usize]),
    }
}

/// `Set_EXT_mode`. Carry is set only when this call came from extended open.
pub fn set_ext_mode(ext_open: bool, save_bx: u16, mode: u16) -> (u16, bool) {
    if !ext_open {
        return (mode, false);
    }
    (mode | save_bx, true)
}

fn make_node(attr: u8, name: &[u8; 11], truncate_ok: bool) -> Result<u8, u8> {
    if let Some(index) = search::find_packed(name) {
        let ent = search::slot(index);
        if ent.dir {
            return Err(1);
        }
        if !truncate_ok {
            return Err(3);
        }
        if ent.attr & ATTR_READ_ONLY != 0 {
            return Err(5);
        }
        let mut next = ent;
        next.size = 0;
        next.attr = attr;
        search::set_slot(index, next);
        return Ok(attr);
    }
    let Some(index) = search::alloc_slot() else {
        return Err(2);
    };
    search::set_slot(
        index,
        Ent {
            name: *name,
            live: true,
            dir: false,
            owned: false,
            data: [0; 32],
            data_len: 0,
            cluster: 0,
            size: 0,
            attr,
            time: 0,
            date: 0x0021,
        },
    );
    Ok(attr)
}

pub fn show() {
    let new_name = search::pack(b"NEW.DAT").unwrap();
    let temp = search::pack(b"TEMP").unwrap();
    let bad_attr = dos_create(0x80, &new_name, true);
    let dir_attr = dos_create(ATTR_DIRECTORY, &new_name, true);
    let created = dos_create(0, &new_name, true);
    let truncated = dos_create(0, &new_name, true);
    let size = search::find_packed(&new_name).map(|index| search::slot(index).size);
    let exists = dos_create(0, &new_name, false);
    if let Some(index) = search::find_packed(&new_name) {
        let mut ent = search::slot(index);
        ent.attr |= ATTR_READ_ONLY;
        search::set_slot(index, ent);
    }
    let readonly = dos_create(0, &new_name, true);
    let _ = super::dircall::dos_mkdir(b"A:\\TEMP\0");
    let on_dir = dos_create(0, &temp, true);
    let (mode, carry) = set_ext_mode(true, 0x0002, 0);
    crate::bios::mscon::con_write_bytes(b"CREATE NW");
    put_hex8(match created {
        Ok(attr) => attr,
        Err(err) => err as u8,
    });
    crate::bios::mscon::con_write_bytes(if truncated.is_ok() && size == Some(0) {
        b" TR00"
    } else {
        b" TRFF"
    });
    crate::bios::mscon::con_write_bytes(b" EX");
    put_hex8(err_byte(exists));
    crate::bios::mscon::con_write_bytes(b" RO");
    put_hex8(err_byte(readonly));
    crate::bios::mscon::con_write_bytes(b" AT");
    put_hex8(err_byte(bad_attr));
    crate::bios::mscon::con_write_bytes(b" DA");
    put_hex8(err_byte(dir_attr));
    crate::bios::mscon::con_write_bytes(b" DIR");
    put_hex8(err_byte(on_dir));
    crate::bios::mscon::con_write_bytes(if carry { b" EM" } else { b" NC" });
    put_hex8(mode as u8);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn err_byte(result: Result<u8, u16>) -> u8 {
    match result {
        Ok(attr) => attr,
        Err(err) => err as u8,
    }
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

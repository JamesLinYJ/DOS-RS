//! `GET_FILE_INFO`, `SET_FILE_ATTRIBUTE`, and `GET_FAST_PATH` from `DOS/FINFO.ASM`.
//!
//! AH=43h already reads and stores the attribute byte. This is the rest of
//! the call: the time, date, and size, a directory reported with size 0, and
//! the application-error class when the new attribute is not changeable.

use super::ms_code;
use super::search;

const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_CHANGEABLE: u8 = 0x27;
const FILE_NOT_FOUND: u16 = 2;
const ACCESS: u16 = 5;
const CLASS_APP: u8 = 7;
const ACTION_ABORT: u8 = 4;
const LOCUS_UNKNOWN: u8 = 1;
const FAST_OPEN_SET: u8 = 0x01;
const FAST_YES: u8 = 0x80;

pub struct Info {
    pub attr: u8,
    pub time: u16,
    pub date: u16,
    pub size: u32,
}

/// `GET_FILE_INFO`. A device and a missing name are file-not-found.
pub fn get_file_info(name: &[u8]) -> Result<Info, u16> {
    if super::dir2::chk_dev(name, 0).is_some() {
        return Err(FILE_NOT_FOUND);
    }
    let Some(packed) = search::pack(name) else {
        return Err(FILE_NOT_FOUND);
    };
    let Some(index) = search::find_packed(&packed) else {
        return Err(FILE_NOT_FOUND);
    };
    let ent = search::slot(index);
    let size = if ent.attr & ATTR_DIRECTORY != 0 || ent.dir {
        0
    } else {
        u32::from(ent.size)
    };
    Ok(Info {
        attr: ent.attr,
        time: ent.time,
        date: ent.date,
        size,
    })
}

/// `SET_FILE_ATTRIBUTE`. Bits outside `attr_changeable` are an application error.
pub fn set_file_attribute(name: &[u8], attr: u16) -> Result<u8, u16> {
    if attr & !u16::from(ATTR_CHANGEABLE) != 0 {
        ms_code::store(ACCESS, CLASS_APP, ACTION_ABORT, LOCUS_UNKNOWN);
        return Err(ACCESS);
    }
    if super::dir2::chk_dev(name, 0).is_some() {
        ms_code::store(ACCESS, CLASS_APP, ACTION_ABORT, LOCUS_UNKNOWN);
        return Err(ACCESS);
    }
    let Some(packed) = search::pack(name) else {
        return Err(FILE_NOT_FOUND);
    };
    let Some(index) = search::find_packed(&packed) else {
        return Err(FILE_NOT_FOUND);
    };
    let mut ent = search::slot(index);
    ent.attr = (ent.attr & !ATTR_CHANGEABLE) | (attr as u8);
    search::set_slot(index, ent);
    Ok(ent.attr)
}

/// `GET_FAST_PATH`. The call sets the fastopen bit, then keeps only `Fast_yes`.
pub fn get_fast_path(flag: u8) -> u8 {
    let _during = flag | FAST_OPEN_SET;
    flag & FAST_YES
}

pub fn show() {
    let sample = get_file_info(b"FOO.COM");
    let _ = super::dircall::dos_mkdir(b"A:\\TEMP\0");
    let dir = get_file_info(b"TEMP");
    let _ = super::dircall::dos_rmdir(b"A:\\TEMP\0");
    let device = get_file_info(b"CON");
    let bad = set_file_attribute(b"FOO.COM", u16::from(ATTR_DIRECTORY));
    let (status, handle, _cx) = ms_code::get();
    let set = set_file_attribute(b"FOO.COM", 0x21);
    let fast = get_fast_path(FAST_OPEN_SET | FAST_YES);
    crate::bios::mscon::con_write_bytes(b"FINFO ");
    match sample {
        Ok(info) => {
            crate::bios::mscon::con_write_bytes(b"AT");
            put_hex8(info.attr);
            crate::bios::mscon::con_write_bytes(b" TM");
            put_hex16(info.time);
            crate::bios::mscon::con_write_bytes(b" DT");
            put_hex16(info.date);
            crate::bios::mscon::con_write_bytes(b" SZ");
            put_hex16(info.size as u16);
        }
        Err(err) => {
            crate::bios::mscon::con_write_bytes(b"ER");
            put_hex16(err);
        }
    }
    crate::bios::mscon::con_write_bytes(b" DIR");
    match dir {
        Ok(info) => put_hex8(info.attr),
        Err(err) => put_hex8(err as u8),
    }
    crate::bios::mscon::con_write_bytes(b" DV");
    put_hex8(match device {
        Ok(info) => info.attr,
        Err(err) => err as u8,
    });
    crate::bios::mscon::con_write_bytes(b" BAD");
    put_hex8(match bad {
        Ok(attr) => attr,
        Err(err) => err as u8,
    });
    crate::bios::mscon::con_write_bytes(b" CL");
    put_hex8((handle >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" AC");
    put_hex8(handle as u8);
    crate::bios::mscon::con_write_bytes(b" LC");
    put_hex8((ms_code::get().2 >> 8) as u8);
    crate::bios::mscon::con_write_bytes(b" SET");
    put_hex8(match set {
        Ok(attr) => attr,
        Err(err) => err as u8,
    });
    crate::bios::mscon::con_write_bytes(b" FO");
    put_hex8(fast);
    crate::bios::mscon::con_write_bytes(b"\r\n");
    let _ = status;
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

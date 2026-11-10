//! FCB open and sequential read from `DOS/FCBIO2.ASM`.
//!
//! `GetExtent` packs the next-record byte with the extent word. A short
//! record returns AL=3, which is `FTRIM` or `FEOF`.

use super::fcb::{packed_name, split_mut};
use super::search;

const FCB_EXTENT: usize = 12;
const FCB_RECSIZ: usize = 14;
const FCB_FILSIZ: usize = 16;
const FCB_DATE: usize = 20;
const FCB_TIME: usize = 22;
const FCB_NR: usize = 32;
const FCB_RR: usize = 33;

const FEOF: u8 = 1;
const FTRIM: u8 = 2;

pub(crate) fn rec_size(fcb: *const u8) -> usize {
    unsafe {
        let raw = u16::from_le_bytes([*fcb.add(FCB_RECSIZ), *fcb.add(FCB_RECSIZ + 1)]);
        if raw == 0 {
            128
        } else {
            raw as usize
        }
    }
}

/// `GetExtent`. DX:AX is the record number.
pub(crate) fn extent_record(fcb: *const u8) -> u32 {
    unsafe {
        let nr = *fcb.add(FCB_NR);
        let extent = u16::from_le_bytes([*fcb.add(FCB_EXTENT), *fcb.add(FCB_EXTENT + 1)]);
        let result = nr << 1;
        let cf = (extent & 1) as u8;
        let buffer = extent >> 1;
        let result = (result >> 1) | (cf << 7);
        let function = (buffer & 0xff) as u8;
        let data_low = (buffer >> 8) as u8;
        (u32::from(data_low) << 16) | (u32::from(function) << 8) | u32::from(result)
    }
}

/// `SetExtent`.
pub(crate) fn set_extent(fcb: *mut u8, record: u32) {
    let status = (record & 0xffff) as u16;
    let mut buffer = ((record >> 16) & 0xffff) as u16;
    let result = (status as u8) & 0x7f;
    let mut count = (status & 0xff00) | u16::from((status as u8) & 0x80);
    let carry = count >> 15;
    count <<= 1;
    buffer = (buffer << 1) | carry;
    let extent_lo = (count >> 8) as u8;
    let extent_hi = buffer as u8;
    unsafe {
        *fcb.add(FCB_NR) = result;
        *fcb.add(FCB_EXTENT) = extent_lo;
        *fcb.add(FCB_EXTENT + 1) = extent_hi;
    }
}

pub(crate) fn store_rr(fcb: *mut u8, record: u32, recsiz: usize) {
    let bytes = record.to_le_bytes();
    unsafe {
        *fcb.add(FCB_RR) = bytes[0];
        *fcb.add(FCB_RR + 1) = bytes[1];
        *fcb.add(FCB_RR + 2) = bytes[2];
        if recsiz < 64 || bytes[3] != 0 {
            *fcb.add(FCB_RR + 3) = bytes[3];
        }
    }
}

fn get_rr(fcb: *const u8, recsiz: usize) -> u32 {
    unsafe {
        let lo = u16::from_le_bytes([*fcb.add(FCB_RR), *fcb.add(FCB_RR + 1)]);
        let mut hi = u16::from_le_bytes([*fcb.add(FCB_RR + 2), *fcb.add(FCB_RR + 3)]);
        if recsiz >= 64 {
            hi &= 0xff;
        }
        (u32::from(hi) << 16) | u32::from(lo)
    }
}

/// AH=0Fh. Record size 0 becomes 128. AL is 0 or 0xFF.
pub fn open(fcb: *mut u8) -> u8 {
    search::ensure();
    let (base, _) = split_mut(fcb);
    let name = packed_name(fcb);
    let Some(index) = search::find_packed(&name) else {
        return 0xff;
    };
    let ent = search::slot(index);
    if ent.dir {
        return 0xff;
    }
    let size = if ent.owned {
        u32::from(ent.data_len)
    } else {
        u32::from(ent.size)
    };
    unsafe {
        if *base == 0 {
            *base = 1;
        }
        if u16::from_le_bytes([*base.add(FCB_RECSIZ), *base.add(FCB_RECSIZ + 1)]) == 0 {
            let bytes = 128u16.to_le_bytes();
            *base.add(FCB_RECSIZ) = bytes[0];
            *base.add(FCB_RECSIZ + 1) = bytes[1];
        }
        let bytes = size.to_le_bytes();
        let mut i = 0usize;
        while i < 4 {
            *base.add(FCB_FILSIZ + i) = bytes[i];
            i += 1;
        }
        let date = ent.date.to_le_bytes();
        let time = ent.time.to_le_bytes();
        *base.add(FCB_DATE) = date[0];
        *base.add(FCB_DATE + 1) = date[1];
        *base.add(FCB_TIME) = time[0];
        *base.add(FCB_TIME + 1) = time[1];
    }
    0
}

/// AH=16h. An existing file is truncated. A new file is an owned root slot.
pub fn create(fcb: *mut u8) -> u8 {
    search::ensure();
    let name = packed_name(fcb);
    if let Some(index) = search::find_packed(&name) {
        let mut ent = search::slot(index);
        if ent.dir {
            return 0xff;
        }
        ent.owned = true;
        ent.data = [0; 32];
        ent.data_len = 0;
        ent.size = 0;
        search::set_slot(index, ent);
    } else {
        let Some(index) = search::alloc_slot() else {
            return 0xff;
        };
        let mut ent = search::slot(index);
        ent.name = name;
        ent.live = true;
        ent.dir = false;
        ent.owned = true;
        ent.attr = 0x20;
        ent.data = [0; 32];
        ent.data_len = 0;
        ent.size = 0;
        search::set_slot(index, ent);
    }
    open(fcb)
}

/// AH=14h. One record from `GetExtent` into the DTA.
pub fn seq_read(fcb: *mut u8) -> u8 {
    let (base, _) = split_mut(fcb);
    transfer(fcb, extent_record(base), true)
}

/// AH=21h. One record from the random-record field.
pub fn random_read(fcb: *mut u8) -> u8 {
    let (base, _) = split_mut(fcb);
    let rec = rec_size(base);
    transfer(fcb, get_rr(base, rec), false)
}

fn transfer(fcb: *mut u8, record: u32, sequential: bool) -> u8 {
    let (base, _) = split_mut(fcb);
    let rec = rec_size(base);
    let name = packed_name(fcb);
    let dta = search::dta();
    let got = crate::dos::handle::read_at(&name, record as usize * rec, dta, rec);
    if got == 0 {
        return FEOF;
    }
    if got < rec && !dta.is_null() {
        let mut i = got;
        while i < rec {
            unsafe { *dta.add(i) = 0 }
            i += 1;
        }
    }
    let next = record + 1;
    if sequential {
        set_extent(base, next);
    } else {
        store_rr(base, next, rec);
    }
    if got < rec {
        FTRIM | FEOF
    } else {
        0
    }
}

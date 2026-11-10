//! FCB calls from `DOS/FCBIO.ASM`.
//!
//! Close returns AL=0 when the SFT cannot be regenerated. Delete removes
//! every root name the pattern matches. File length and the relative-record
//! call write the random-record field.

use super::fcb::{packed_name, split_mut};
use super::fcbio2::{extent_record, rec_size, store_rr};
use super::search::{self, name_match};

/// AH=10h. A missing SFT is still a successful close.
pub fn close(_fcb: *const u8) -> u8 {
    0
}

/// AH=13h. AL is 0xFF when nothing was deleted.
pub fn delete(fcb: *const u8) -> u8 {
    search::ensure();
    let pat = packed_name(fcb);
    let mut any = false;
    let mut index = 0usize;
    while index < 16 {
        let mut ent = search::slot(index);
        if ent.live && !ent.dir && name_match(&pat, &ent.name) {
            ent.live = false;
            search::set_slot(index, ent);
            any = true;
        }
        index += 1;
    }
    if any {
        0
    } else {
        0xff
    }
}

/// AH=17h. The new name starts at offset 0x11 of the real FCB.
pub fn rename(fcb: *mut u8) -> u8 {
    search::ensure();
    let (base, _) = split_mut(fcb);
    let src = packed_name(fcb);
    let mut dst = [b' '; 11];
    unsafe {
        let mut i = 0usize;
        while i < 11 {
            dst[i] = *base.add(0x11 + i);
            i += 1;
        }
    }
    let Some(index) = search::find_packed(&src) else {
        return 0xff;
    };
    if search::find_packed(&dst).is_some() {
        return 0xff;
    }
    let mut ent = search::slot(index);
    if ent.dir {
        return 0xff;
    }
    ent.name = dst;
    search::set_slot(index, ent);
    0
}

/// AH=23h. The random-record field becomes the length in records, rounded up.
pub fn file_length(fcb: *mut u8) -> u8 {
    search::ensure();
    let (base, _) = split_mut(fcb);
    let name = packed_name(fcb);
    let Some(index) = search::find_packed(&name) else {
        return 0xff;
    };
    let ent = search::slot(index);
    let size = if ent.owned {
        u32::from(ent.data_len)
    } else {
        u32::from(ent.size)
    };
    let rec = rec_size(base) as u32;
    if rec == 0 {
        return 0xff;
    }
    let mut records = size / rec;
    if size % rec != 0 {
        records += 1;
    }
    store_rr(base, records, rec as usize);
    0
}

/// AH=24h. The random-record field becomes the sequential position.
pub fn set_position(fcb: *mut u8) -> u8 {
    let (base, _) = split_mut(fcb);
    let record = extent_record(base);
    store_rr(base, record, rec_size(base));
    0
}

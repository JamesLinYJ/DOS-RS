//! FCB calls from `DOS/FCBIO.ASM`.
//!
//! Close succeeds when the SFT cannot be regenerated. File length writes
//! the random-record field.

use super::fcb::{packed_name, split_mut};
use super::fcbio2::{rec_size, store_rr};
use super::search;

/// AH=10h. A missing SFT is still a successful close.
pub fn close(_fcb: *const u8) -> u8 {
    0
}

/// AH=23h. The random-record field becomes the length in records, rounded up.
pub fn file_length(fcb: *mut u8) -> u8 {
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

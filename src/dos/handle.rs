//! Open and read from `DOS/HANDLE.ASM`, for the boot image.
//!
//! AH=3Dh returns handle 5 when the 8.3 name is in the root. AH=3Fh copies
//! the next bytes of that file. The file data starts at cluster 2, LBA 4.

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");

static mut OFF: usize = 0;
static mut LEFT: usize = 0;
static mut OPEN: bool = false;
static mut OWNED: bool = false;
static mut OWNED_AT: usize = 0;
static mut OWNED_POS: usize = 0;

pub fn open_asciiz(name: *const u8) -> Option<u16> {
    let mut raw = [0u8; 12];
    let mut len = 0usize;
    unsafe {
        while len < raw.len() {
            let byte = *name.add(len);
            if byte == 0 {
                break;
            }
            raw[len] = byte;
            len += 1;
        }
    }
    let packed = pack(&raw[..len])?;
    if let Some(index) = crate::dos::search::find_packed(&packed) {
        let ent = crate::dos::search::slot(index);
        if ent.dir {
            return None;
        }
        if ent.owned {
            unsafe {
                OWNED = true;
                OWNED_AT = index;
                OWNED_POS = 0;
                OPEN = true;
            }
            return Some(5);
        }
        if ent.cluster >= 2 {
            let start = (4 + (ent.cluster as usize - 2)) * 512;
            if start < IMAGE.len() {
                unsafe {
                    OFF = start;
                    LEFT = (ent.size as usize).min(IMAGE.len() - start);
                    OPEN = true;
                    OWNED = false;
                }
                return Some(5);
            }
        }
    }
    let root = 3 * 512;
    let mut index = 0usize;
    while index < 16 {
        let entry = &IMAGE[root + index * 32..root + index * 32 + 32];
        if entry[0] == 0 {
            break;
        }
        if entry[0] != 0xe5 && entry[..11].eq_ignore_ascii_case(&packed) {
            let cluster = u16::from_le_bytes([entry[26], entry[27]]) as usize;
            let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as usize;
            if cluster < 2 {
                return None;
            }
            let start = (4 + (cluster - 2)) * 512;
            if start >= IMAGE.len() {
                return None;
            }
            unsafe {
                OFF = start;
                LEFT = size.min(IMAGE.len() - start);
                OPEN = true;
                OWNED = false;
            }
            return Some(5);
        }
        index += 1;
    }
    None
}

pub fn bind_owned(index: usize) {
    unsafe {
        OWNED = true;
        OWNED_AT = index;
        OWNED_POS = 0;
        OPEN = true;
    }
}

pub fn write(handle: u16, src: *const u8, count: usize) -> usize {
    if handle != 5 || unsafe { !OPEN || !OWNED } {
        return 0;
    }
    let index = unsafe { OWNED_AT };
    let mut ent = crate::dos::search::slot(index);
    let mut n = 0usize;
    while n < count && (ent.data_len as usize) < ent.data.len() {
        ent.data[ent.data_len as usize] = unsafe { *src.add(n) };
        ent.data_len += 1;
        n += 1;
    }
    crate::dos::search::set_slot(index, ent);
    n
}

pub fn read(handle: u16, dst: *mut u8, count: usize) -> usize {
    if handle != 5 || unsafe { !OPEN } {
        return 0;
    }
    if unsafe { OWNED } {
        let ent = crate::dos::search::slot(unsafe { OWNED_AT });
        let pos = unsafe { OWNED_POS };
        let avail = (ent.data_len as usize).saturating_sub(pos);
        let n = count.min(avail);
        let mut i = 0usize;
        while i < n {
            unsafe { *dst.add(i) = ent.data[pos + i] }
            i += 1;
        }
        unsafe { OWNED_POS += n }
        return n;
    }
    let n = count.min(unsafe { LEFT });
    let off = unsafe { OFF };
    let mut i = 0usize;
    while i < n {
        unsafe { *dst.add(i) = IMAGE[off + i] }
        i += 1;
    }
    unsafe {
        OFF += n;
        LEFT -= n;
    }
    n
}

fn pack(name: &[u8]) -> Option<[u8; 11]> {
    let mut out = [b' '; 11];
    let mut dot = name.len();
    let mut i = 0usize;
    while i < name.len() {
        if name[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    if dot == 0 || dot > 8 {
        return None;
    }
    let mut n = 0usize;
    while n < dot {
        out[n] = up(name[n]);
        n += 1;
    }
    if dot < name.len() {
        let mut e = 0usize;
        while e < 3 && dot + 1 + e < name.len() {
            out[8 + e] = up(name[dot + 1 + e]);
            e += 1;
        }
    }
    Some(out)
}

fn up(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - 0x20
    } else {
        byte
    }
}

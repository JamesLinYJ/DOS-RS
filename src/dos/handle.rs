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
static mut OPEN_AT: usize = usize::MAX;
static mut POS: u32 = 0;
static mut SIZE: u32 = 0;
static mut BASE: usize = 0;
static mut DIRTY: bool = false;

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
            arm_owned(index);
            return Some(5);
        }
        if ent.cluster >= 2 {
            let start = (4 + (ent.cluster as usize - 2)) * 512;
            if start < IMAGE.len() {
                arm_image(index, start, ent.size as usize);
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
            arm_image(index, start, size);
            return Some(5);
        }
        index += 1;
    }
    None
}

pub fn bind_owned(index: usize) {
    arm_owned(index);
}

pub fn opened_index(handle: u16) -> Option<usize> {
    if handle != 5 || unsafe { !OPEN || OPEN_AT == usize::MAX } {
        return None;
    }
    Some(unsafe { OPEN_AT })
}

pub fn write(handle: u16, src: *const u8, count: usize) -> usize {
    if handle != 5 || unsafe { !OPEN || !OWNED } {
        return 0;
    }
    let index = unsafe { OWNED_AT };
    let mut ent = crate::dos::search::slot(index);
    let mut pos = unsafe { OWNED_POS };
    let mut n = 0usize;
    while n < count && pos < ent.data.len() {
        ent.data[pos] = unsafe { *src.add(n) };
        pos += 1;
        n += 1;
    }
    if (pos as u8) > ent.data_len {
        ent.data_len = pos as u8;
        ent.size = u16::from(ent.data_len);
    }
    crate::dos::search::set_slot(index, ent);
    unsafe {
        OWNED_POS = pos;
        POS = pos as u32;
        SIZE = u32::from(ent.data_len);
        if n > 0 {
            DIRTY = true;
        }
    }
    n
}

/// AH=42h. `method` 0 stores CX:DX, 1 adds it to the position, 2 adds it to the size.
/// The `Ok` value is the new position. `Err` is `error_invalid_handle` or
/// `error_invalid_function`.
pub fn seek(handle: u16, method: u8, dx: u16, cx: u16) -> Result<u32, u16> {
    if handle != 5 || unsafe { !OPEN } {
        return Err(6);
    }
    if method > 2 {
        return Err(1);
    }
    let offset = (u32::from(cx) << 16) | u32::from(dx);
    let pos = unsafe {
        match method {
            0 => offset,
            1 => POS.wrapping_add(offset),
            _ => SIZE.wrapping_add(offset),
        }
    };
    place(pos);
    Ok(pos)
}

/// AH=3Eh. A second close of the same handle is `error_invalid_handle`.
pub fn close(handle: u16) -> u16 {
    if handle != 5 || unsafe { !OPEN } {
        return 6;
    }
    let index = unsafe { OPEN_AT };
    let dirty = unsafe { DIRTY };
    if dirty {
        crate::dos::close::commit(index);
    }
    unsafe {
        OPEN = false;
        DIRTY = false;
    }
    0
}

/// Read `count` bytes of a root file at a byte offset. The handle cursor is left alone.
pub fn read_at(name: &[u8; 11], offset: usize, dst: *mut u8, count: usize) -> usize {
    if dst.is_null() || count == 0 {
        return 0;
    }
    let Some(index) = crate::dos::search::find_packed(name) else {
        return 0;
    };
    let ent = crate::dos::search::slot(index);
    if ent.dir {
        return 0;
    }
    if ent.owned {
        let len = ent.data_len as usize;
        if offset >= len {
            return 0;
        }
        let n = count.min(len - offset);
        let mut i = 0usize;
        while i < n {
            unsafe { *dst.add(i) = ent.data[offset + i] }
            i += 1;
        }
        return n;
    }
    if ent.cluster < 2 {
        return 0;
    }
    let base = (4 + (ent.cluster as usize - 2)) * 512;
    let size = ent.size as usize;
    if offset >= size || base + offset >= IMAGE.len() {
        return 0;
    }
    let n = count.min(size - offset).min(IMAGE.len() - (base + offset));
    let mut i = 0usize;
    while i < n {
        unsafe { *dst.add(i) = IMAGE[base + offset + i] }
        i += 1;
    }
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
        unsafe {
            OWNED_POS += n;
            POS += n as u32;
        }
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
        POS += n as u32;
    }
    n
}

fn arm_owned(index: usize) {
    let ent = crate::dos::search::slot(index);
    unsafe {
        OWNED = true;
        OWNED_AT = index;
        OPEN_AT = index;
        OWNED_POS = 0;
        POS = 0;
        SIZE = u32::from(ent.data_len);
        DIRTY = false;
        OPEN = true;
    }
}

fn arm_image(index: usize, start: usize, size: usize) {
    unsafe {
        OWNED = false;
        OPEN_AT = index;
        BASE = start;
        SIZE = size as u32;
        DIRTY = false;
        OPEN = true;
    }
    place(0);
}

fn place(pos: u32) {
    unsafe {
        POS = pos;
        if OWNED {
            OWNED_POS = pos as usize;
            return;
        }
        let size = SIZE as usize;
        let at = pos as usize;
        if at >= size {
            OFF = BASE.saturating_add(size);
            LEFT = 0;
        } else {
            OFF = BASE + at;
            let room = IMAGE.len().saturating_sub(OFF);
            LEFT = (size - at).min(room);
        }
    }
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

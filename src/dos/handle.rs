//! Open and read from `DOS/HANDLE.ASM`, for the boot image.
//!
//! AH=3Dh returns handle 5 when the 8.3 name is in the root. AH=3Fh copies
//! the next bytes of that file. The file data starts at cluster 2, LBA 4.

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");

static mut OFF: usize = 0;
static mut LEFT: usize = 0;
static mut OPEN: bool = false;

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
            }
            return Some(5);
        }
        index += 1;
    }
    None
}

pub fn read(handle: u16, dst: *mut u8, count: usize) -> usize {
    if handle != 5 || unsafe { !OPEN } {
        return 0;
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

//! Paged GB18030 font. Independently compressed LZ4 blocks fit a 16-bit segment.
use dosrs_realmode::{
    byte, copy_bytes,
    system::{block, dw, sd, sw, w},
};
pub const MAP: u16 = 0x7000;
const META: u16 = 0x37c0;
const RAW: u16 = 0x3320;
const CACHE: u16 = 0x7280;
// Between the four decoded pages and the ATAPI transfer buffer at 7800:0000.
const HEADER: u16 = 0x7680;
const HEADER_BYTES: u16 = 6144;
pub fn init(unit: u16, pages: u16) -> bool {
    sw(META, 0, unit);
    sw(META, 2, pages);
    sw(META, 4, 65535);
    sw(META, 24, 0);
    sw(META, 26, 0);
    let mut i = 0;
    while i < 4 {
        sw(META, 16 + i * 2, 65535);
        i += 1
    }
    let blocks = phys32(8);
    if phys32(0) != 0x31434247 || blocks == 0 || blocks > 4096 {
        return false;
    }
    sd(META, 8, phys32(4));
    sw(META, 12, blocks as u16);
    let first = phys32(12);
    let directory_end = 12 + (blocks + 1) * 4;
    if first < directory_end || first > pages as u32 * 512 {
        return false;
    }
    if first > HEADER_BYTES as u32 {
        return true;
    }
    let mut at = 0;
    while at < first as u16 {
        if !page(at >> 9) {
            return false;
        }
        let left = first as u16 - at;
        let n = if left < 512 { left } else { 512 };
        copy_bytes(RAW, 0, HEADER, at, n);
        at += n;
    }
    sw(META, 24, first as u16);
    if indexed_ranges(directory_end as u16, first as u16) {
        sw(META, 26, directory_end as u16);
    }
    true
}

// The extension sits between the offset table and the first compressed block.
// Older GBC1 files have no extension and use the original on-disk key array.
fn indexed_ranges(table: u16, end: u16) -> bool {
    if end - table < 12 || dw(HEADER, table) != 0x31524247 {
        return false;
    }
    let count = dw(HEADER, table + 8);
    if count > ((end - table - 12) >> 3) as u32 {
        return false;
    }
    let mut last = 0;
    let mut glyphs = 0;
    let mut i = 0;
    while i < count as u16 {
        let p = table + 12 + i * 8;
        let first = dw(HEADER, p);
        let length = dw(HEADER, p + 4);
        if first < last || first >= 1587600 || length == 0 || length > 1587600 - first {
            return false;
        }
        last = first + length;
        glyphs += length;
        i += 1;
    }
    let data = dw(HEADER, table + 4);
    let raw = dw(META, 8);
    data <= raw && glyphs <= ((raw - data) >> 5)
}
fn page(n: u16) -> bool {
    if w(META, 4) == n {
        return true;
    }
    if n >= w(META, 2) || !block(w(META, 0), dw(MAP, n * 4), RAW) {
        return false;
    }
    sw(META, 4, n);
    true
}
fn phys(at: u32) -> u16 {
    if page((at >> 9) as u16) {
        byte(RAW, (at & 511) as u16)
    } else {
        0
    }
}
fn phys32(at: u32) -> u32 {
    phys(at) as u32
        | (phys(at + 1) as u32) << 8
        | (phys(at + 2) as u32) << 16
        | (phys(at + 3) as u32) << 24
}
fn extended(count: u16, pos: &mut u32, end: u32) -> u16 {
    if count != 15 {
        return count;
    }
    let mut n = 15;
    while *pos < end {
        let c = phys(*pos);
        *pos += 1;
        if n > 4096 - c {
            return 65535;
        }
        n += c;
        if c != 255 {
            return n;
        }
    }
    65535
}
fn unpack(id: u16, seg: u16) -> bool {
    let tab = 12 + (id as u32) * 4;
    let (mut p, end) = if tab + 8 <= w(META, 24) as u32 {
        (dw(HEADER, tab as u16), dw(HEADER, tab as u16 + 4))
    } else {
        (phys32(tab), phys32(tab + 4))
    };
    if p >= end || end > w(META, 2) as u32 * 512 {
        return false;
    }
    let mut used = 0;
    while p < end {
        let token = phys(p);
        p += 1;
        let n = extended(token >> 4, &mut p, end);
        if n > 4096 - used || n as u32 > end - p {
            return false;
        }
        let mut left = n;
        while left > 0 {
            if !page((p >> 9) as u16) {
                return false;
            }
            let at = (p & 511) as u16;
            let room = 512 - at;
            let take = if left < room { left } else { room };
            copy_bytes(RAW, at, seg, used, take);
            used += take;
            p += take as u32;
            left -= take;
        }
        if p == end {
            break;
        }
        if end - p < 2 {
            return false;
        }
        let back = phys(p) | (phys(p + 1) << 8);
        p += 2;
        let add = extended(token & 15, &mut p, end);
        if back == 0 || back > used || add > 4092 || add + 4 > 4096 - used {
            return false;
        }
        let n = add + 4;
        copy_bytes(seg, used - back, seg, used, n);
        used += n;
    }
    let rem = dw(META, 8) - ((id as u32) << 12);
    used == if rem >= 4096 { 4096 } else { rem as u16 }
}
pub fn rb(at: u32) -> u16 {
    if at >= dw(META, 8) {
        return 0;
    }
    let id = (at >> 12) as u16;
    let slot = id & 3;
    let tag = 16 + slot * 2;
    let seg = CACHE + (slot << 8);
    if w(META, tag) != id {
        sw(META, tag, 65535);
        if !unpack(id, seg) {
            return 0;
        }
        sw(META, tag, id);
    }
    byte(seg, (at & 4095) as u16)
}
pub fn rd(at: u32) -> u32 {
    rb(at) as u32 | (rb(at + 1) as u32) << 8 | (rb(at + 2) as u32) << 16 | (rb(at + 3) as u32) << 24
}

/// Return the bitmap address in a font with the compact four-byte character index.
pub fn four_byte(code: u32) -> Option<u32> {
    let p = w(META, 26);
    if p == 0 {
        return None;
    }
    let a = (code >> 24) as u16;
    let b = (code >> 16) as u16 & 255;
    let c = (code >> 8) as u16 & 255;
    let d = code as u16 & 255;
    if a < 129 || a > 254 || b < 48 || b > 57 || c < 129 || c > 254 || d < 48 || d > 57 {
        return Some(0);
    }
    let lead = ((a - 129) * 10 + (b - 48)) as u32;
    let ordinal = (lead * 126 + (c - 129) as u32) * 10 + (d - 48) as u32;
    let mut bitmap = dw(HEADER, p + 4);
    let count = w(HEADER, p + 8);
    let mut i = 0;
    while i < count {
        let at = p + 12 + i * 8;
        let first = dw(HEADER, at);
        let n = dw(HEADER, at + 4);
        if ordinal < first {
            break;
        }
        if ordinal - first < n {
            return Some(bitmap + ((ordinal - first) << 5));
        }
        bitmap += n << 5;
        i += 1;
    }
    Some(0)
}

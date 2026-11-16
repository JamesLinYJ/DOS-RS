//! Paged GB18030 font. Independently compressed LZ4 blocks fit a 16-bit segment.
use dosrs_realmode::{
    byte, copy_bytes,
    system::{block, dw, sd, sw, w},
};
pub const MAP: u16 = 0x7000;
const META: u16 = 0x37c0;
const RAW: u16 = 0x3320;
const CACHE: u16 = 0x7280;
pub fn init(unit: u16, pages: u16) -> bool {
    sw(META, 0, unit);
    sw(META, 2, pages);
    sw(META, 4, 65535);
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
    true
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
    let mut p = phys32(tab);
    let end = phys32(tab + 4);
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

//! Direct-mapped cache for fixed BIOS disks. Writes reach the device before success.
use super::{
    byte, copy_bytes, store,
    system::{dw, sd, sw, w},
};
const META: u16 = 0x3700;
const DATA: u16 = 0x8b00;
const STAMP: u16 = 0x5344;
pub fn init() {
    sw(META, 0, STAMP);
    let kib = w(0x40, 0x13);
    sw(
        META,
        2,
        if kib >= 572 {
            32
        } else if kib >= 564 {
            16
        } else {
            0
        },
    );
    reset();
}
pub fn slots() -> u16 {
    w(META, 2)
}
pub fn reset() {
    let mut i = 0;
    while i < 128 {
        store(META, 32 + i * 5, 0);
        i += 1;
    }
    sd(META, 4, 0);
    sd(META, 8, 0);
}
pub fn enable(on: bool) {
    if on {
        init()
    } else {
        sw(META, 2, 0);
        reset();
    }
}
pub fn hits() -> u32 {
    dw(META, 4)
}
pub fn reads() -> u32 {
    dw(META, 8)
}
pub fn io(unit: u16, lba: u32, seg: u16, write: bool) -> bool {
    if w(META, 0) != STAMP || slots() == 0 {
        return super::raw_hd(unit, lba, seg, write);
    }
    let slot = ((lba as u16) ^ ((lba >> 16) as u16) ^ unit) & (slots() - 1);
    let tag = 32 + slot * 5;
    let data = DATA + (slot << 5);
    if !write && byte(META, tag) == unit && dw(META, tag + 1) == lba {
        copy_bytes(data, 0, seg, 0, 512);
        sd(META, 4, hits().wrapping_add(1));
        return true;
    }
    if !write {
        sd(META, 8, reads().wrapping_add(1));
    }
    store(META, tag, 0);
    if !super::raw_hd(unit, lba, seg, write) {
        return false;
    }
    copy_bytes(seg, 0, data, 0, 512);
    sd(META, tag + 1, lba);
    store(META, tag, unit);
    true
}

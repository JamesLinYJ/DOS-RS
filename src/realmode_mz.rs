//! Decode MZ relocations before entering a native 16-bit DOS process.
use dosrs_realmode::{self as pc, PROGRAM};

pub const FILE: u16 = 0x2000;
const IMAGE: u16 = PROGRAM + 16;
const END: u16 = 0x6000;

fn word(seg: u16, at: u16) -> u16 {
    pc::byte(seg, at) | (pc::byte(seg, at + 1) << 8)
}
fn set_word(seg: u16, at: u16, val: u16) {
    pc::store(seg, at, val);
    pc::store(seg, at + 1, val >> 8);
}

pub fn run(volume: &mut pc::disk::Volume, size: u16) -> bool {
    if size < 28 || word(FILE, 0) != 0x5a4d {
        return false;
    }
    let pages = word(FILE, 4);
    let tail = word(FILE, 2);
    if pages == 0 || pages > 128 || tail > 511 {
        return false;
    }
    let declared = ((pages as u32 - 1) << 9) + (if tail == 0 { 512 } else { tail } as u32);
    if declared != size as u32 || word(FILE, 8) > size / 16 {
        return false;
    }
    let header = word(FILE, 8) * 16;
    let table = word(FILE, 24);
    let count = word(FILE, 6);
    if header < 28 || (count != 0 && (table < 28 || table > header || count > (header - table) / 4))
    {
        return false;
    }
    let length = size - header;
    let paragraphs = length / 16 + if length % 16 == 0 { 0 } else { 1 };
    let available = END - IMAGE;
    if paragraphs > available || word(FILE, 10) > available - paragraphs {
        return false;
    }
    let cs = word(FILE, 22);
    let ip = word(FILE, 20);
    let ss = word(FILE, 14);
    let sp = word(FILE, 16);
    if ((cs as u32) << 4) + ip as u32 >= length as u32 {
        return false;
    }
    let stack = ((ss as u32) << 4) + if sp == 0 { 65536 } else { sp as u32 };
    if ss >= available || stack < 4 || stack > (available as u32) << 4 || sp == 1 {
        return false;
    }
    let mut i = 0;
    while i < length {
        pc::store(IMAGE, i, pc::byte(FILE, header + i));
        i += 1;
    }
    let mut n = 0;
    while n < count {
        let rec = table + n * 4;
        let off = word(FILE, rec);
        let seg = word(FILE, rec + 2);
        if ((seg as u32) << 4) + off as u32 + 1 >= length as u32 {
            return false;
        }
        let at = seg * 16 + off;
        set_word(IMAGE, at, word(IMAGE, at).wrapping_add(IMAGE));
        n += 1;
    }
    pc::start_exe(volume, IMAGE + cs, ip, IMAGE + ss, sp);
    true
}

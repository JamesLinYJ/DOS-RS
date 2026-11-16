//! Stream MZ programs into paragraphs; near pointers apply only to individual transfers.
use dosrs_realmode::{
    self as pc, PROGRAM,
    system::{dw, sd, sw, w},
};
pub const FILE: u16 = 0x2000;
const IMAGE: u16 = PROGRAM + 16;
const END: u16 = 0x6000;
const REQUEST: u16 = 0x35e0;
const MEMORY: u16 = 0xffff;
struct Source {
    unit: u16,
    first: u32,
    size: u32,
}
impl Source {
    fn read(&self, pos: u32, count: u16, dest: u16) -> bool {
        if pos > self.size || count as u32 > self.size - pos {
            return false;
        }
        if self.unit == MEMORY {
            if dest != FILE {
                pc::copy_bytes(FILE, pos as u16, dest, 0, count)
            }
            return true;
        }
        sd(REQUEST, 0, self.first);
        sd(REQUEST, 4, self.size);
        sd(REQUEST, 8, pos);
        sw(REQUEST, 12, count);
        sw(REQUEST, 14, dest);
        sw(REQUEST, 16, 0);
        pc::control(24, self.unit, 0, 0) != 0
    }
    fn run(&self, volume: &mut pc::disk::Volume) -> bool {
        if !self.read(0, 28, FILE) || w(FILE, 0) != 0x5a4d {
            return false;
        }
        let pages = w(FILE, 4);
        let tail = w(FILE, 2);
        if pages == 0 || tail > 511 {
            return false;
        }
        let declared = ((pages as u32 - 1) << 9) + if tail == 0 { 512 } else { tail as u32 };
        let header = (w(FILE, 8) as u32) << 4;
        let table = w(FILE, 24);
        let count = w(FILE, 6);
        if declared != self.size
            || header < 28
            || header > self.size
            || header > 65520
            || (count != 0 && (table < 28 || table as u32 + count as u32 * 4 > header))
        {
            return false;
        }
        let size = self.size - header;
        let available = END - IMAGE;
        if (size + 15) >> 4 > available as u32
            || ((size + 15) >> 4) + w(FILE, 10) as u32 > available as u32
        {
            return false;
        }
        let cs = w(FILE, 22);
        let ip = w(FILE, 20);
        let ss = w(FILE, 14);
        let sp = w(FILE, 16);
        let stack = ((ss as u32) << 4) + if sp == 0 { 65536 } else { sp as u32 };
        if ((cs as u32) << 4) + ip as u32 >= size
            || ss >= available
            || stack < 4
            || stack > (available as u32) << 4
            || sp == 1
        {
            return false;
        }
        if !self.read(0, header as u16, FILE) {
            return false;
        }
        let mut done = 0;
        while done < size {
            let n = if size - done > 16384 {
                16384
            } else {
                (size - done) as u16
            };
            if !self.read(header + done, n, IMAGE + (done >> 4) as u16) {
                return false;
            }
            done += n as u32;
        }
        let mut i = 0;
        while i < count {
            let rec = table + i * 4;
            let off = w(FILE, rec);
            let seg = w(FILE, rec + 2);
            let linear = seg as u32 * 16 + off as u32;
            if linear + 1 >= size {
                return false;
            }
            let at = IMAGE + (linear >> 4) as u16;
            let low = (linear & 15) as u16;
            sw(at, low, w(at, low).wrapping_add(IMAGE));
            i += 1;
        }
        pc::start_exe(volume, IMAGE + cs, ip, IMAGE + ss, sp);
        true
    }
}
pub fn run(volume: &mut pc::disk::Volume, size: u16) -> bool {
    Source {
        unit: MEMORY,
        first: 0,
        size: size as u32,
    }
    .run(volume)
}
pub fn open(volume: &mut pc::disk::Volume, drive: u16, original: u32, split: u16, n: u16) -> bool {
    let mut first = 0;
    let mut size = 0;
    if drive == 2 {
        if !volume.describe(&mut first, &mut size) {
            return false;
        }
    } else {
        if pc::control(23, drive, 0, 0) == 0 {
            return false;
        }
        first = dw(REQUEST, 0);
        size = dw(REQUEST, 4)
    }
    super::psp(split, n);
    volume.restore(original);
    let source = Source {
        unit: if drive == 2 { pc::hard_unit() } else { drive },
        first,
        size,
    };
    source.run(volume)
}

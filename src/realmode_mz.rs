//! Stream MZ programs into paragraphs; near pointers apply only to individual transfers.
use dosrs_realmode::{
    self as pc, PROGRAM,
    system::{dw, sd, sw, w},
};
pub const FILE: u16 = 0x2000;

use super::arena;
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
    fn run(&self, volume: &mut pc::disk::Volume, psp: u16, paragraphs: u16) -> bool {
        if !self.read(0, 28, FILE) || w(FILE, 0) != 0x5a4d {
            return false;
        }
        let pages = w(FILE, 4);
        let tail = w(FILE, 2);
        if pages == 0 || tail > 511 {
            return false;
        }
        // NE and LE files append sections beyond the DOS load module. EXEC
        // loads only the bytes declared by the MZ header, including its relocs.
        let declared = ((pages as u32 - 1) << 9) + if tail == 0 { 512 } else { tail as u32 };
        let header = (w(FILE, 8) as u32) << 4;
        let table = w(FILE, 24);
        let count = w(FILE, 6);
        if declared > self.size
            || header < 28
            || header > declared
            || header > 65520
            || (count != 0 && (table < 28 || table as u32 + count as u32 * 4 > header))
        {
            return false;
        }
        let size = declared - header;
        let load_paras = (size + 15) >> 4;
        let min = w(FILE, 10);
        let mut available = paragraphs - 16;
        if load_paras + u32::from(min) > u32::from(available) {
            return false;
        }
        // MZ min=max=0 places the module at the top of its allocation. The
        // PSP remains at the bottom; loaders use the space between PSP and CS.
        if min == 0 && w(FILE, 12) == 0 {
            available = load_paras as u16;
        }
        let image = psp + paragraphs - available;
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
            if !self.read(header + done, n, image + (done >> 4) as u16) {
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
            let at = image + (linear >> 4) as u16;
            let low = (linear & 15) as u16;
            sw(at, low, w(at, low).wrapping_add(image));
            i += 1;
        }
        pc::start_exe(volume, psp, image + cs, ip, image + ss, sp);
        true
    }
}
pub fn run(volume: &mut pc::disk::Volume, size: u16) -> bool {
    Source {
        unit: MEMORY,
        first: 0,
        size: size as u32,
    }
    .run(volume, PROGRAM, arena::end() - PROGRAM)
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
    source.run(volume, PROGRAM, arena::end() - PROGRAM)
}

pub fn child(volume: &mut pc::disk::Volume, parent: u16, ss: u16, sp: u16) -> u32 {
    let ds = w(ss, sp + 16);
    let path = w(ss, sp + 6);
    let es = w(ss, sp + 14);
    let param = w(ss, sp + 2);
    let cwd = volume.directory();
    if !super::files::name(volume, ds, path) {
        return 0x10003;
    }
    let mut first = 0;
    let mut size = 0;
    if !volume.describe(&mut first, &mut size) {
        return 0x10002;
    }
    volume.restore(cwd);
    let source = Source {
        unit: pc::hard_unit(),
        first,
        size,
    };
    let Some(env) = super::environment::child(parent, w(es, param), ds, path) else {
        return 0x10008;
    };
    let Some((child, paragraphs)) = arena::child_block() else {
        sw(env - 1, 1, 0);
        return 0x10008;
    };
    sw(env - 1, 1, child);
    pc::control(25, child, parent, child + paragraphs);
    sw(child, 0x2c, env);
    let tail = w(es, param + 2);
    let segment = w(es, param + 4);
    let length = pc::byte(segment, tail);
    if length < 127 {
        pc::copy_bytes(segment, tail, child, 0x80, length + 2);
    }
    super::files::inherit(child);
    arena::select(child);
    let data = pc::dos_tables::LIST_SEG;
    let dta = pc::dos_tables::SDA + 12;
    let saved_dta = dw(data, dta);
    sw(data, dta, 0x80);
    sw(data, dta + 2, child);
    let ok = if !source.read(0, 2, FILE) {
        false
    } else if w(FILE, 0) == 0x5a4d {
        source.run(volume, child, paragraphs)
    } else if size > 0xff00 || size + 0x102 > u32::from(paragraphs) * 16 {
        false
    } else if source.read(0, size as u16, child + 16) {
        let top = if paragraphs >= 0x1000 {
            0xfffe
        } else {
            paragraphs * 16 - 2
        };
        sw(child, top, 0);
        pc::start_exe(volume, child, child, 0x100, child, top);
        true
    } else {
        false
    };
    sd(data, dta, saved_dta);
    super::files::reap(volume, child);
    pc::copy_bytes(child, 0x0a, 0, 0x22 * 4, 12);
    arena::select(parent);
    arena::discard(child);
    if ok { 0 } else { 0x1000b }
}

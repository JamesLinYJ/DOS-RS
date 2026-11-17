//! Native COM and MZ loading. Every read has an explicit file offset and length.
use super::{arena, environment, files};
use dosrs_realmode::{
    self as pc, COMMAND, PROGRAM,
    disk::Volume,
    system::{dw, sd, sw, w},
};

const HEADER: u16 = pc::layout::KERNEL_HEADER;
const REQUEST: u16 = 0x35e0;
// Private words in the resident kernel transfer area. The PSP and live JFT
// stay allocated even when the interpreter's executable code is paged out.
const SHELL_SOURCE: u16 = 0x200;
use pc::layout::ACTIVE_SHELL;
const SHELL_DATA_END: u16 = ACTIVE_SHELL + 2;
const MEMORY: u32 = 0x10008;
const FORMAT: u32 = 0x1000b;

struct Source {
    unit: u16,
    first: u32,
    size: u32,
}
impl Source {
    fn open(vol: &mut Volume, seg: u16, at: u16) -> Result<Self, u16> {
        let mut source = Self {
            unit: 0,
            first: 0,
            size: 0,
        };
        let error = files::executable(
            vol,
            seg,
            at,
            &mut source.unit,
            &mut source.first,
            &mut source.size,
        );
        if error == 0 { Ok(source) } else { Err(error) }
    }
    fn read(&self, pos: u32, count: u16, dest: u16) -> bool {
        if pos > self.size || u32::from(count) > self.size - pos {
            return false;
        }
        if self.unit == u16::MAX {
            return pc::cdrom::fetch(self.first, self.size, pos, count, dest, 0)
                == u32::from(count);
        }
        sd(REQUEST, 0, self.first);
        sd(REQUEST, 4, self.size);
        sd(REQUEST, 8, pos);
        sw(REQUEST, 12, count);
        sw(REQUEST, 14, dest);
        sw(REQUEST, 16, 0);
        pc::control(24, self.unit, 0, 0) != 0
    }
    fn is_interpreter(&self) -> bool {
        self.unit == w(COMMAND, SHELL_SOURCE)
            && self.first == dw(COMMAND, SHELL_SOURCE + 2)
            && self.size == dw(COMMAND, SHELL_SOURCE + 6)
    }
    fn run(&self, volume: &mut Volume, psp: u16, paragraphs: u16) -> u32 {
        if self.size == 0 {
            return FORMAT;
        }
        if self.size < 2 || self.read(0, 2, HEADER) && w(HEADER, 0) != 0x5a4d {
            if self.size > 0xff00 {
                return FORMAT;
            }
            if self.size + 0x102 > u32::from(paragraphs) * 16 {
                return MEMORY;
            }
            if !self.read(0, self.size as u16, psp + 16) {
                return FORMAT;
            }
            let sp = if paragraphs >= 0x1000 {
                0xfffe
            } else {
                paragraphs * 16 - 2
            };
            sw(psp, sp, 0);
            pc::start_exe(volume, psp, psp, 0x100, psp, sp);
            return 0;
        }
        let Some(mz) = Mz::read(self) else {
            return FORMAT;
        };
        if self.is_interpreter() && mz.rust_transient() {
            return run_transient(self, &mz, volume, psp);
        }
        if mz.paras() + u32::from(mz.min) > u32::from(paragraphs - 16) {
            return MEMORY;
        }
        let available = if mz.min == 0 && mz.max == 0 {
            mz.paras() as u16
        } else {
            paragraphs - 16
        };
        let image = psp + paragraphs - available;
        let stack = u32::from(mz.ss) * 16 + if mz.sp == 0 { 65536 } else { u32::from(mz.sp) };
        if mz.ss >= available || stack < 4 || stack > u32::from(available) * 16 || mz.sp == 1 {
            return FORMAT;
        }
        if !mz.load(self, image, image.wrapping_add(mz.ss), false) {
            return FORMAT;
        }
        pc::start_exe(volume, psp, image + mz.cs, mz.ip, image + mz.ss, mz.sp);
        0
    }
}

struct Mz {
    header: u32,
    size: u32,
    table: u16,
    count: u16,
    min: u16,
    max: u16,
    cs: u16,
    ip: u16,
    ss: u16,
    sp: u16,
}
impl Mz {
    fn read(source: &Source) -> Option<Self> {
        if !source.read(0, 28, HEADER) || w(HEADER, 0) != 0x5a4d {
            return None;
        }
        let pages = w(HEADER, 4);
        let tail = w(HEADER, 2);
        if pages == 0 || tail > 511 {
            return None;
        }
        let declared = (u32::from(pages) - 1) * 512 + if tail == 0 { 512 } else { u32::from(tail) };
        let header = u32::from(w(HEADER, 8)) * 16;
        let table = w(HEADER, 24);
        let count = w(HEADER, 6);
        if declared > source.size
            || header < 28
            || header >= declared
            || count != 0 && (table < 28 || u32::from(table) + u32::from(count) * 4 > header)
        {
            return None;
        }
        let size = declared - header;
        let cs = w(HEADER, 22);
        let ip = w(HEADER, 20);
        if u32::from(cs) * 16 + u32::from(ip) >= size {
            return None;
        }
        Some(Self {
            header,
            size,
            table,
            count,
            min: w(HEADER, 10),
            max: w(HEADER, 12),
            cs,
            ip,
            ss: w(HEADER, 14),
            sp: w(HEADER, 16),
        })
    }
    fn paras(&self) -> u32 {
        (self.size + 15) >> 4
    }
    // Rust16 MZ executables use a separate SS=DS segment following the read-only
    // load module. Only COMMAND.COM, identified at boot, uses this split layout.
    fn rust_transient(&self) -> bool {
        self.cs == 0
            && self.ip == 0
            && self.min == self.max
            && self.min != 0
            && self.sp >= 4
            && u32::from(self.sp) <= u32::from(self.min) * 16
            && self.paras() == u32::from(self.ss)
    }
    fn load(&self, source: &Source, image: u16, data: u16, split: bool) -> bool {
        let mut pos = 0;
        while pos < self.size {
            let n = if self.size - pos > 16384 {
                16384
            } else {
                (self.size - pos) as u16
            };
            if !source.read(self.header + pos, n, image + (pos >> 4) as u16) {
                return false;
            }
            pos += u32::from(n);
        }
        let mut i = 0;
        while i < self.count {
            let rec = (i & 127) * 4;
            if rec == 0 {
                let remaining = self.count - i;
                let entries = if remaining < 128 { remaining } else { 128 };
                if !source.read(
                    u32::from(self.table) + u32::from(i) * 4,
                    entries * 4,
                    HEADER,
                ) {
                    return false;
                }
            }
            let linear = u32::from(w(HEADER, rec + 2)) * 16 + u32::from(w(HEADER, rec));
            if linear + 1 >= self.size {
                return false;
            }
            let at = image + (linear >> 4) as u16;
            let off = (linear & 15) as u16;
            let relative = w(at, off);
            let relocated = if split && relative >= self.ss {
                data.wrapping_add(relative - self.ss)
            } else {
                image.wrapping_add(relative)
            };
            sw(at, off, relocated);
            i += 1;
        }
        true
    }
}

fn run_transient(source: &Source, mz: &Mz, vol: &mut Volume, psp: u16) -> u32 {
    // The linker emits pure code followed by a separate SS=DS segment. Use
    // the unoccupied portion of the loader's reserved kernel region for the
    // latter, in EXEC nesting order. Further interpreters use linked UMBs
    // when available, then conventional paragraphs in their own PSP block.
    let metadata = psp + 16;
    let previous_data = w(COMMAND, SHELL_DATA_END);
    let reserved = previous_data >= pc::KERNEL
        && previous_data <= pc::layout::KERNEL_LIMIT
        && mz.min <= pc::layout::KERNEL_LIMIT - previous_data;
    let mut largest = 0;
    if mz.min > 0xffee {
        return MEMORY;
    }
    let upper = if reserved {
        None
    } else {
        arena::upper_block(mz.min)
    };
    let resident = if reserved || upper.is_some() {
        17
    } else {
        17 + mz.min
    };
    if arena::request(0x4a00, resident, psp, &mut largest) != 0 {
        return MEMORY;
    }
    let data = if reserved {
        previous_data
    } else {
        upper.unwrap_or(metadata + 1)
    };
    let size = mz.ss;
    let Some(code) = arena::code_block(size, 0) else {
        return MEMORY;
    };
    if !mz.load(source, code, data, true) {
        return FORMAT;
    }
    let previous = w(COMMAND, ACTIVE_SHELL);
    sw(metadata, 0, code);
    sw(metadata, 2, size);
    sw(metadata, 4, data);
    sw(metadata, 6, mz.min);
    if reserved {
        sw(COMMAND, SHELL_DATA_END, data + mz.min);
    }
    sw(COMMAND, ACTIVE_SHELL, psp);
    pc::start_exe(vol, psp, code + mz.cs, mz.ip, data, mz.sp);
    sw(COMMAND, ACTIVE_SHELL, previous);
    sw(COMMAND, SHELL_DATA_END, previous_data);
    let mut room = 0;
    arena::request(0x4900, 0, code, &mut room);
    0
}
fn resume(psp: u16) {
    let meta = psp + 16;
    let code = w(meta, 0);
    let count = w(meta, 2);
    let source = Source {
        unit: w(COMMAND, SHELL_SOURCE),
        first: dw(COMMAND, SHELL_SOURCE + 2),
        size: dw(COMMAND, SHELL_SOURCE + 6),
    };
    if arena::code_block(count, code) == Some(code) {
        if let Some(mz) = Mz::read(&source) {
            if mz.rust_transient() && mz.ss == count && mz.load(&source, code, w(meta, 4), true) {
                sw(COMMAND, ACTIVE_SHELL, psp);
                return;
            }
        }
    }
    pc::print("Cannot reload the command interpreter\r\n");
    pc::halt()
}

// The DOSK header is still resident at the boot handoff but subsequent MZ
// loading reuses this sector. Snapshot the selected kernel extent immediately.
pub(super) fn boot_memory() {
    #[cfg(rust16_cpu = "286")]
    let entry = 4;
    #[cfg(rust16_cpu = "386")]
    let entry = 10;
    let n = w(HEADER, entry + 4);
    let paragraphs = if w(HEADER, entry + 2) & 0xc000 == 0xc000 {
        n
    } else if n != 0 {
        ((n - 1) >> 4) + 1
    } else {
        0
    };
    let end = if paragraphs != 0 && paragraphs <= pc::layout::KERNEL_LIMIT - pc::KERNEL {
        pc::KERNEL + paragraphs
    } else {
        pc::layout::KERNEL_LIMIT
    };
    sw(COMMAND, SHELL_DATA_END, end);
}

pub fn interpreter(vol: &mut Volume, boot: u16) -> bool {
    pc::command("A:\\COMMAND.COM");
    pc::store(COMMAND, 0, 65 + boot);
    let Ok(source) = Source::open(vol, COMMAND, 0) else {
        return false;
    };
    sw(COMMAND, SHELL_SOURCE, source.unit);
    sd(COMMAND, SHELL_SOURCE + 2, source.first);
    sd(COMMAND, SHELL_SOURCE + 6, source.size);
    sw(COMMAND, ACTIVE_SHELL, 0);
    pc::control(25, PROGRAM, PROGRAM, arena::end());
    environment::root(PROGRAM);
    sd(
        pc::dos_tables::LIST_SEG,
        pc::dos_tables::SDA + 12,
        (u32::from(PROGRAM) << 16) | 0x80,
    );
    sw(PROGRAM, 0x80, 0x2003);
    sw(PROGRAM, 0x82, 0x502f);
    pc::store(PROGRAM, 0x84, 13);
    source.run(vol, PROGRAM, arena::end() - PROGRAM) == 0
}

pub fn child(vol: &mut Volume, parent: u16, ss: u16, sp: u16) -> u32 {
    let ds = w(ss, sp + 16);
    let path = w(ss, sp + 6);
    let es = w(ss, sp + 14);
    let param = w(ss, sp + 2);
    let source = match Source::open(vol, ds, path) {
        Ok(source) => source,
        Err(e) => return 0x10000 | u32::from(e),
    };
    let suspend = w(COMMAND, ACTIVE_SHELL) == parent;
    if suspend {
        let mut room = 0;
        if arena::request(0x4900, 0, w(parent + 16, 0), &mut room) != 0 {
            return MEMORY;
        }
        sw(COMMAND, ACTIVE_SHELL, 0);
    }
    let result = spawn(&source, vol, parent, ds, path, es, param);
    if suspend {
        resume(parent);
    }
    result
}
fn spawn(
    source: &Source,
    vol: &mut Volume,
    parent: u16,
    ds: u16,
    path: u16,
    es: u16,
    param: u16,
) -> u32 {
    let Some(env) = environment::child(parent, w(es, param), ds, path) else {
        return MEMORY;
    };
    let Some((child, paragraphs)) = arena::child_block() else {
        sw(env - 1, 1, 0);
        return MEMORY;
    };
    sw(env - 1, 1, child);
    pc::control(25, child, parent, child + paragraphs);
    sw(child, 0x2c, env);
    let tail = w(es, param + 2);
    let seg = w(es, param + 4);
    let len = pc::byte(seg, tail);
    if len <= 126 && tail <= u16::MAX - len - 1 {
        pc::copy_bytes(seg, tail, child, 0x80, len + 2);
    }
    let mut at = 6;
    while at <= 10 {
        let dest = 0x5c + (at - 6) * 4;
        let fcb = w(es, param + at);
        let segment = w(es, param + at + 2);
        if segment != 0 && fcb <= 0xfff0 {
            pc::copy_bytes(segment, fcb, child, dest, 16);
        }
        at += 4;
    }
    files::inherit(child);
    arena::select(child);
    let l = pc::dos_tables::LIST_SEG;
    let d = pc::dos_tables::SDA + 12;
    let saved = dw(l, d);
    sd(l, d, (u32::from(child) << 16) | 0x80);
    let result = source.run(vol, child, paragraphs);
    sd(l, d, saved);
    files::reap(child);
    pc::copy_bytes(child, 0x0a, 0, 0x22 * 4, 12);
    arena::select(parent);
    arena::discard(child);
    result
}

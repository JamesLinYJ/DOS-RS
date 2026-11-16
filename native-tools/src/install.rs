//! Install directly from the boot medium onto a new DOS primary partition.
//! Publish its boot sector only after both FATs, directory and payload are on disk.
use crate::media_set::{CONFIG_BUFFER, Package};
use crate::{
    capacity,
    partition::{self, TAG, Target},
    text,
};
use dosrs_realmode::{
    byte, hd, store,
    system::{File, Media, dw, sd, sw, w},
};
const BUF: u16 = 0x3400;
const DIR: u16 = 0x3480;
fn clear(s: u16) {
    let mut i = 0;
    while i < 512 {
        store(s, i, 0);
        i += 1;
    }
}
fn template(media: &Media, off: u32) -> bool {
    if let Some(f) = media.lookup(b"BOOT    BIN") {
        return media.fetch(&f, off, 512, BUF, 0);
    }
    false
}
struct Layout {
    base: u32,
    length: u32,
    wide: bool,
    shift: u16,
    fat: u32,
    data: u32,
    root: u32,
    span: u32,
    count: u32,
}
impl Layout {
    fn new(base: u32, length: u32) -> Self {
        let wide = length >= 70000;
        let mut shift = 0;
        if wide {
            while shift < 7 && length >> shift > 0x003f_ffff {
                shift += 1;
            }
        } else if length > 65000 {
            shift = 1;
        }
        let reserved = if wide { 32 } else { 1 };
        let roots = if wide { 0 } else { 32 };
        let entries = if wide { 7 } else { 8 };
        // Round up from an upper bound on the cluster count.
        let span =
            (((length - reserved - roots) >> shift) + 2 + ((1u32 << entries) - 1)) >> entries;
        let root = base + reserved + 2 * span;
        let data = root + roots;
        let count = (length - (data - base)) >> shift;
        Self {
            base,
            length,
            wide,
            shift,
            fat: base + reserved,
            data,
            root,
            span,
            count,
        }
    }
    fn first_file(&self) -> u32 {
        if self.wide {
            2 + ((2 + (1u32 << self.shift) - 1) >> self.shift)
        } else {
            2
        }
    }
    fn sector(&self, c: u32) -> u32 {
        self.data + ((c - 2) << self.shift)
    }
    fn dir(&self) -> u32 {
        if self.wide { self.data } else { self.root }
    }
}
const SYSTEM_COUNT: u16 = 16;
const FILE_COUNT: u16 = SYSTEM_COUNT + 2;
const EXTENTS: u16 = 1024;
pub(crate) fn name(i: u16) -> &'static [u8] {
    match i {
        0 => b"IO      SYS",
        1 => b"MSDOS   SYS",
        2 => b"COMMAND COM",
        3 => b"SETUP   EXE",
        4 => b"FDISK   EXE",
        5 => b"BOOT    BIN",
        6 => b"GLYPHS  FNT",
        7 => b"LANG    DAT",
        8 => b"OFL     TXT",
        9 => b"WELCOME TXT",
        10 => b"SMARTDRVEXE",
        11 => b"INPUT   DAT",
        12 => b"LICENSE TXT",
        13 => b"NOTICE  TXT",
        14 => b"EDIT    EXE",
        15 => b"CHECKSUMDAT",
        16 => b"CONFIG  SYS",
        17 => b"AUTOEXECBAT",
        _ => b"",
    }
}
fn label(s: u16, p: u16, v: &[u8]) {
    let mut i = 0;
    while i < v.len() {
        store(s, p + i as u16, v[i] as u16);
        i += 1;
    }
}
fn fat(l: &Layout, last: u32) -> bool {
    let per = if l.wide { 128 } else { 256 };
    let width = if l.wide { 4 } else { 2 };
    let mut sector = 0;
    while sector < l.span {
        clear(BUF);
        let mut j = 0;
        let mut row = 0;
        while j < per {
            let c = (sector << if l.wide { 7 } else { 8 }) + j as u32;
            if c >= last {
                break;
            }
            let mut val = c + 1;
            if l.wide && c >= 2 && c < l.first_file() - 1 {
                val = c + 1
            } else if c == 0 {
                val = 0x0fff_fff8
            } else if c == 1 || (l.wide && c == l.first_file() - 1) {
                val = 0x0fff_ffff
            } else {
                while row < FILE_COUNT && c >= dw(DIR, EXTENTS + row * 4) {
                    row += 1;
                }
                if row < FILE_COUNT && c + 1 == dw(DIR, EXTENTS + row * 4) {
                    val = 0x0fff_ffff
                }
            }
            if width == 4 {
                sd(BUF, j * 4, val)
            } else {
                sw(BUF, j * 2, val as u16)
            }
            j += 1;
        }
        if !hd(l.fat + sector, BUF, true) || !hd(l.fat + l.span + sector, BUF, true) {
            return false;
        }
        sector += 1;
    }
    true
}
fn bpb(l: &Layout, io: u32, io_blocks: u16, src: &Media) -> bool {
    if !template(src, 512) {
        return false;
    }
    label(BUF, 3, b"DOS     ");
    sw(BUF, 11, 512);
    store(BUF, 13, 1 << l.shift);
    sw(BUF, 14, if l.wide { 32 } else { 1 });
    store(BUF, 16, 2);
    sw(BUF, 17, if l.wide { 0 } else { 512 });
    store(BUF, 21, 248);
    sw(BUF, 24, 63);
    sw(BUF, 26, 16);
    sd(BUF, 28, l.base);
    if l.length < 65536 {
        sw(BUF, 19, l.length as u16)
    } else {
        sd(BUF, 32, l.length)
    }
    let at = if l.wide {
        sd(BUF, 36, l.span);
        sd(BUF, 44, 2);
        sw(BUF, 48, 1);
        sw(BUF, 50, 6);
        64
    } else {
        sw(BUF, 22, l.span as u16);
        36
    };
    store(BUF, at, 128);
    store(BUF, at + 2, 41);
    sd(BUF, at + 3, TAG);
    label(BUF, at + 7, b"NO NAME    ");
    label(BUF, at + 18, if l.wide { b"FAT32   " } else { b"FAT16   " });
    sd(BUF, 416, io);
    sw(BUF, 420, io_blocks);
    if l.wide && !hd(l.base + 6, BUF, true) {
        return false;
    }
    hd(l.base, BUF, true)
}
pub fn partition(src: &Package, t: &Target) -> bool {
    if !matches!(t.state, partition::NEW | partition::SPACE) || !t.same(&partition::inspect()) {
        return false;
    }
    // Keep existing boot records on shared disks. On a new disk check the gap before the partition too.
    let first = if t.shared { t.first } else { 1 };
    if !partition::is_blank(first, t.first + t.size - first, src.probe_buffer_free())
        || !template(&src.boot, 0)
    {
        return false;
    }
    partition::publish(t, BUF)
}
pub fn install(src: &Package, t: &Target, settings: &crate::startup::Settings) -> bool {
    if t.state != partition::PREPARED || !t.same(&partition::inspect()) {
        return false;
    }
    let l = Layout::new(t.first, t.size);
    if l.count < 4085 || (l.wide && l.count < 65525) {
        return false;
    }
    let mut next = l.first_file();
    let mut i = 0;
    clear(DIR);
    clear(DIR + 32);
    let mut io_count = 0;
    while i < FILE_COUNT {
        let f = if i < SYSTEM_COUNT {
            let Some(srcfile) = src.lookup(i) else {
                return false;
            };
            srcfile
        } else {
            File {
                first: 0,
                size: settings.render(&src.boot, i - SYSTEM_COUNT, CONFIG_BUFFER) as u32,
                contiguous: false,
            }
        };
        if f.size == 0 {
            return false;
        }
        let clusters = ((f.size - 1) >> (9 + l.shift)) + 1;
        if clusters > l.count + 2 - next {
            return false;
        }
        let index = i * 32;
        label(DIR, index, name(i));
        store(DIR, index + 11, if i < 2 { 7 } else { 32 });
        sw(DIR, index + 26, next as u16);
        sw(DIR, index + 20, (next >> 16) as u16);
        sd(DIR, index + 28, f.size);
        let sectors = ((f.size - 1) >> 9) + 1;
        if i == 0 {
            if sectors > 127 {
                return false;
            }
            io_count = sectors as u16;
        }
        crate::ui::progress_begin(i, FILE_COUNT, name(i), f.size);
        if i < SYSTEM_COUNT {
            if !transfer(src, i, &f, l.sector(next)) {
                return false;
            }
        } else {
            let mut j = 0;
            while j < sectors {
                if !hd(l.sector(next) + j, CONFIG_BUFFER + (j as u16) * 32, true) {
                    return false;
                }
                j += 1
            }
        }
        next += clusters;
        sd(DIR, EXTENTS + i * 4, next);
        crate::ui::progress(i + 1, FILE_COUNT, name(i));
        i += 1;
    }
    if !fat(&l, next) {
        return false;
    }
    clear(BUF);
    let roots = if l.wide {
        ((l.first_file() - 2) as u16) << l.shift
    } else {
        32
    };
    let mut j = 2;
    while j < roots {
        if !hd(l.dir() + j as u32, BUF, true) {
            return false;
        }
        j += 1;
    }
    if !hd(l.dir(), DIR, true) || !hd(l.dir() + 1, DIR + 32, true) {
        return false;
    }
    if l.wide {
        clear(BUF);
        sd(BUF, 0, 0x41615252);
        sd(BUF, 484, 0x61417272);
        sd(BUF, 488, l.count + 2 - next);
        sd(BUF, 492, next);
        sw(BUF, 510, 0xaa55);
        if !hd(l.base + 1, BUF, true) || !hd(l.base + 7, BUF, true) {
            return false;
        }
    }
    bpb(&l, l.sector(l.first_file()), io_count, &src.boot)
}
fn transfer(src: &Package, index: u16, f: &File, lba: u32) -> bool {
    let mut off = 0;
    while off < f.size {
        let remaining = f.size - off;
        let n = if remaining < 512 {
            remaining as u16
        } else {
            512
        };
        if n < 512 {
            clear(BUF)
        }
        if !src.fetch(index, f, off, n, BUF) || !hd(lba + (off >> 9), BUF, true) {
            return false;
        }
        off += n as u32;
        crate::ui::progress_bytes(off);
    }
    true
}
pub fn report() {
    if !hd(0, BUF, false) {
        text("Disk unavailable\r\n");
        return;
    }
    text("Disk ");
    number((dosrs_realmode::hard_unit() - 127) as u32);
    text("   ");
    number(capacity() >> 11);
    text(" MiB\r\n");
    if w(BUF, 510) != 0xaa55 {
        text("No partition table\r\n");
        return;
    }
    let mut i = 0;
    while i < 4 {
        let p = 446 + i * 16;
        let t = byte(BUF, p + 4);
        if t != 0 {
            text("  ");
            number(i as u32 + 1);
            text("  type ");
            number(t as u32);
            text("  LBA ");
            number(dw(BUF, p + 8));
            text("  sectors ");
            number(dw(BUF, p + 12));
            text("\r\n");
        }
        i += 1;
    }
}
pub fn number(mut n: u32) {
    let mut buf = [0u8; 10];
    let mut i = 0;
    loop {
        let mut q = 0u32;
        let mut r = 0u16;
        let mut bit = 32;
        while bit > 0 {
            bit -= 1;
            r = (r << 1) | ((n >> bit) & 1) as u16;
            if r >= 10 {
                r -= 10;
                q |= 1u32 << bit;
            }
        }
        buf[i] = 48 + r as u8;
        i += 1;
        n = q;
        if n == 0 {
            break;
        }
    }
    while i > 0 {
        i -= 1;
        crate::put(buf[i] as u16);
    }
}

pub fn verify(src: &Package, t: &Target, repair: bool) -> bool {
    let Some(dst) = Media::partition(dosrs_realmode::hard_unit(), t.first, t.size) else {
        return false;
    };
    let other = 0x3520;
    // Validate every name, size and chain before changing an installed payload.
    let mut pass = 0;
    while pass < 2 {
        let mut i = 0;
        while i < SYSTEM_COUNT {
            let Some(a) = src.lookup(i) else {
                return false;
            };
            let Some(mut b) = dst.lookup(name(i)) else {
                return false;
            };
            if a.size == 0 || a.size != b.size {
                return false;
            }

            dst.check_extent(&mut b);
            if !b.contiguous {
                return false;
            }
            if pass != 0 {
                crate::ui::progress_begin(i, SYSTEM_COUNT, name(i), a.size);
                let mut off = 0;
                while off < a.size {
                    let n = if a.size - off < 512 {
                        (a.size - off) as u16
                    } else {
                        512
                    };
                    if !src.fetch(i, &a, off, n, BUF) || !dst.fetch(&b, off, n, other, 0) {
                        return false;
                    }
                    let mut j = 0;
                    while j < n && byte(BUF, j) == byte(other, j) {
                        j += 1;
                    }
                    if j < n {
                        if !repair {
                            return false;
                        }
                        let Some(lba) = dst.file_sector(&b, off >> 9) else {
                            return false;
                        };
                        dosrs_realmode::copy_bytes(BUF, 0, other, 0, n);
                        if !hd(lba, other, true)
                            || !dosrs_realmode::raw_hd(dst.unit, lba, other, false)
                        {
                            return false;
                        }
                        j = 0;
                        while j < n {
                            if byte(BUF, j) != byte(other, j) {
                                return false;
                            }
                            j += 1;
                        }
                    }
                    off += n as u32;
                    crate::ui::progress_bytes(off);
                }
                crate::ui::progress(i + 1, SYSTEM_COUNT, name(i));
            }
            i += 1;
        }
        pass += 1;
    }
    // Validate on-disk bytes against the manifest after copying or repairing.
    dosrs_realmode::cache::reset();
    preflight(&Package::new(dst)) != 0
}

fn sum_mod(a: u16, b: u16) -> u16 {
    let c = a.wrapping_add(b);
    if c < a {
        c + 1
    } else if c == 65535 {
        0
    } else {
        c
    }
}
pub fn preflight(src: &Package) -> u32 {
    let Some(manifest) = src.lookup(SYSTEM_COUNT - 1) else {
        return 0;
    };
    if manifest.size != 289
        || !src.fetch(SYSTEM_COUNT - 1, &manifest, 0, 289, DIR)
        || dw(DIR, 0) != 0x31565344
    {
        return 0;
    }
    let mut total = manifest.size;
    let mut i = 0;
    while i < SYSTEM_COUNT - 1 {
        let mut row = 4;
        while row < 289 {
            let mut j = 0;
            while j < 11 && byte(DIR, row + j) == name(i)[j as usize] as u16 {
                j += 1;
            }
            if j == 11 {
                break;
            }
            row += 19;
        }
        if row >= 289 {
            return 0;
        }
        let Some(f) = src.lookup(i) else {
            return 0;
        };
        if f.size == 0 || f.size > 16 * 1024 * 1024 || f.size != dw(DIR, row + 11) {
            return 0;
        }

        let mut pos = 0;
        let mut a = 0;
        let mut b = 0;
        while pos < f.size {
            let n = if f.size - pos < 512 {
                (f.size - pos) as u16
            } else {
                512
            };
            if !src.fetch(i, &f, pos, n, BUF) {
                return 0;
            }
            if n & 1 != 0 {
                store(BUF, n, 0)
            }
            let mut j = 0;
            while j < n {
                a = sum_mod(a, w(BUF, j));
                b = sum_mod(b, a);
                j += 2;
            }
            pos += n as u32;
        }
        if a != w(DIR, row + 15) || b != w(DIR, row + 17) {
            return 0;
        }
        total += f.size;
        i += 1;
    }
    total
}

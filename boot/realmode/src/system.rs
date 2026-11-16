//! Read boot files using BIOS sectors, including fragmented files above 64 KiB.
use super::{byte, copy_bytes, store};
pub const SCRATCH: u16 = 0x3200;
const FATBUF: u16 = 0x3240;
pub fn w(s: u16, p: u16) -> u16 {
    byte(s, p) | (byte(s, p + 1) << 8)
}
pub fn dw(s: u16, p: u16) -> u32 {
    w(s, p) as u32 | ((w(s, p + 2) as u32) << 16)
}
pub fn sw(s: u16, p: u16, v: u16) {
    store(s, p, v);
    store(s, p + 1, v >> 8);
}
pub fn sd(s: u16, p: u16, v: u32) {
    sw(s, p, v as u16);
    sw(s, p + 2, (v >> 16) as u16);
}
pub fn block(unit: u16, lba: u32, seg: u16) -> bool {
    if unit >= 128 {
        super::cache::io(unit, lba, seg, false)
    } else {
        lba < 2880 && super::read_floppy(unit, lba as u16, seg)
    }
}

#[derive(Clone, Copy)]
pub struct Media {
    pub unit: u16,
    fat: u32,
    data: u32,
    root: u32,
    dir_sectors: u16,
    shift: u16,
    kind: u16,
    limit: u32,
}
#[derive(Clone, Copy)]
pub struct File {
    pub first: u32,
    pub size: u32,
    pub contiguous: bool,
}
impl Media {
    pub fn open(hard: bool) -> Option<Self> {
        Self::device(if hard {
            super::hard_unit()
        } else {
            if super::boot_unit() == 1 { 1 } else { 0 }
        })
    }
    pub fn device(unit: u16) -> Option<Self> {
        store(FATBUF, 520, 0);
        let hard = unit >= 128;
        if !block(unit, 0, SCRATCH) {
            return None;
        }
        let mut start = 0;
        let mut length = u32::MAX;
        if hard && (w(SCRATCH, 11) != 512 || (byte(SCRATCH, 0) != 0xeb && byte(SCRATCH, 0) != 0xe9))
        {
            let mut p = 446;
            while p < 510 {
                if matches!(byte(SCRATCH, p + 4), 4 | 6 | 11 | 12 | 14)
                    && (start == 0 || byte(SCRATCH, p) == 128)
                {
                    start = dw(SCRATCH, p + 8);
                    length = dw(SCRATCH, p + 12);
                    if byte(SCRATCH, p) == 128 {
                        break;
                    }
                }
                p += 16;
            }
            if start == 0 || !block(unit, start, SCRATCH) {
                return None;
            }
        }
        return Self::parse(unit, start, length);
    }
    pub fn partition(unit: u16, start: u32, length: u32) -> Option<Self> {
        store(FATBUF, 520, 0);
        if !block(unit, start, SCRATCH) {
            return None;
        }
        Self::parse(unit, start, length)
    }
    fn parse(unit: u16, start: u32, length: u32) -> Option<Self> {
        if w(SCRATCH, 510) != 0xaa55 || w(SCRATCH, 11) != 512 {
            return None;
        }
        let spc = byte(SCRATCH, 13);
        if spc == 0 || spc > 128 || spc & (spc - 1) != 0 {
            return None;
        }
        let mut shift = 0;
        while (1u16 << shift) < spc {
            shift += 1;
        }
        let total = if w(SCRATCH, 19) > 0 {
            w(SCRATCH, 19) as u32
        } else {
            dw(SCRATCH, 32)
        };
        let reserved = w(SCRATCH, 14) as u32;
        let copies = byte(SCRATCH, 16);
        if total > length || reserved == 0 || copies == 0 || copies > 4 || start > u32::MAX - total
        {
            return None;
        }
        let short = w(SCRATCH, 22);
        let span = if short != 0 {
            short as u32
        } else {
            dw(SCRATCH, 36)
        };
        if span == 0 || span > total >> 2 && copies > 1 {
            return None;
        }
        let mut root_rel = reserved;
        let mut i = 0;
        while i < copies {
            if root_rel >= total || span >= total - root_rel {
                return None;
            }
            root_rel += span;
            i += 1;
        }
        let dir_sectors = (w(SCRATCH, 17) as u32 + 15) >> 4;
        let first = root_rel + dir_sectors;
        if first >= total {
            return None;
        }
        let count = (total - first) >> shift;
        let kind = if count < 4085 {
            12
        } else if count < 65525 {
            16
        } else {
            32
        };
        let root = if kind == 32 {
            dw(SCRATCH, 44)
        } else {
            start + root_rel
        };
        let mut fat = start + reserved;
        if kind == 32 && w(SCRATCH, 40) & 128 != 0 {
            let index = w(SCRATCH, 40) & 15;
            if index >= copies {
                return None;
            }
            fat += span * index as u32;
        }
        Some(Self {
            unit,
            fat,
            data: start + first,
            root,
            dir_sectors: dir_sectors as u16,
            shift,
            kind,
            limit: count + 1,
        })
    }
    fn valid(&self, c: u32) -> bool {
        c >= 2 && c <= self.limit
    }
    fn fat_page(&self, lba: u32) -> bool {
        if byte(FATBUF, 520) == 1 && byte(FATBUF, 521) == self.unit && dw(FATBUF, 522) == lba {
            return true;
        }
        store(FATBUF, 520, 0);
        if !block(self.unit, lba, FATBUF) {
            return false;
        }
        store(FATBUF, 520, 1);
        store(FATBUF, 521, self.unit);
        sd(FATBUF, 522, lba);
        true
    }
    fn next(&self, c: u32) -> u32 {
        let n = if self.kind == 12 {
            c + (c >> 1)
        } else if self.kind == 16 {
            c << 1
        } else {
            c << 2
        };
        let lba = self.fat + (n >> 9);
        let p = (n & 511) as u16;
        if !self.fat_page(lba) {
            return 0;
        }
        if self.kind == 32 {
            return dw(FATBUF, p) & 0x0fff_ffff;
        }
        let lo = byte(FATBUF, p);
        if p == 511 && !self.fat_page(lba + 1) {
            return 0;
        }
        let v = lo | (byte(FATBUF, (p + 1) & 511) << 8);
        if self.kind == 16 {
            v as u32
        } else if c & 1 == 0 {
            (v & 4095) as u32
        } else {
            (v >> 4) as u32
        }
    }
    pub fn lookup(&self, name: &[u8]) -> Option<File> {
        if name.len() != 11 {
            return None;
        }
        let mut cluster = self.root;
        let mut visits = 0u32;
        loop {
            let (base, span) = if self.kind == 32 {
                if !self.valid(cluster) || visits >= self.limit {
                    return None;
                }
                (
                    self.data + ((cluster - 2) << self.shift),
                    1u16 << self.shift,
                )
            } else {
                (self.root, self.dir_sectors)
            };
            let mut j = 0;
            while j < span {
                if !block(self.unit, base + j as u32, SCRATCH) {
                    return None;
                }
                let mut p = 0;
                while p < 512 {
                    let c = byte(SCRATCH, p);
                    if c == 0 {
                        return None;
                    }
                    if c != 229 && byte(SCRATCH, p + 11) & 0x18 == 0 {
                        let mut i = 0;
                        while i < 11 && byte(SCRATCH, p + i) == name[i as usize] as u16 {
                            i += 1;
                        }
                        if i == 11 {
                            let first = w(SCRATCH, p + 26) as u32
                                | if self.kind == 32 {
                                    (w(SCRATCH, p + 20) as u32) << 16
                                } else {
                                    0
                                };
                            let size = dw(SCRATCH, p + 28);
                            if size > 0 && !self.valid(first) {
                                return None;
                            }
                            return Some(File {
                                first,
                                size,
                                contiguous: false,
                            });
                        }
                    }
                    p += 32;
                }
                j += 1;
            }
            if self.kind != 32 {
                return None;
            }
            cluster = self.next(cluster);
            visits += 1;
        }
    }
    pub fn map_sectors(&self, f: &File, seg: u16, max: u16) -> u16 {
        if f.size == 0 || f.size > max as u32 * 512 {
            return 0;
        }
        let total = ((f.size - 1) >> 9) as u16 + 1;
        let mut c = f.first;
        let mut n = 0;
        while n < total {
            if !self.valid(c) {
                return 0;
            }
            let base = self.data + ((c - 2) << self.shift);
            let mut i = 0;
            while i < (1 << self.shift) && n < total {
                sd(seg, n * 4, base + i as u32);
                i += 1;
                n += 1;
            }
            if n < total {
                c = self.next(c);
            }
        }
        total
    }
    pub fn check_extent(&self, f: &mut File) {
        if f.size == 0 {
            return;
        }
        let n = (f.size - 1) >> (9 + self.shift);
        if n > self.limit - f.first {
            return;
        }
        let mut i = 0;
        while i < n {
            if self.next(f.first + i) != f.first + i + 1 {
                return;
            }
            i += 1;
        }
        f.contiguous = true;
    }
    pub fn file_sector(&self, f: &File, sector: u32) -> Option<u32> {
        if f.size == 0 || sector > ((f.size - 1) >> 9) {
            return None;
        }
        let skip = sector >> self.shift;
        let mut c = f.first;
        if f.contiguous {
            c += skip
        } else {
            let mut i = 0;
            while i < skip && self.valid(c) {
                c = self.next(c);
                i += 1;
            }
        }
        if !self.valid(c) {
            return None;
        }
        Some(self.data + ((c - 2) << self.shift) + (sector & ((1u32 << self.shift) - 1)))
    }
    pub fn fetch(&self, f: &File, offset: u32, n: u16, dest: u16, at: u16) -> bool {
        if offset > f.size || n as u32 > f.size - offset || at as u32 + n as u32 > 65536 {
            return false;
        }
        let skip = offset >> (9 + self.shift);
        let mut c = f.first;
        if f.contiguous {
            c += skip
        } else {
            let mut i = 0;
            while i < skip && self.valid(c) {
                c = self.next(c);
                i += 1;
            }
        }
        let mask = (1u16 << self.shift) - 1;
        let mut sec = ((offset >> 9) as u16) & mask;
        let mut inside = (offset & 511) as u16;
        let mut copied = 0;
        while copied < n {
            if !self.valid(c) {
                return false;
            }
            let lba = self.data + ((c - 2) << self.shift) + sec as u32;
            if !block(self.unit, lba, SCRATCH) {
                return false;
            }
            let mut take = 512 - inside;
            if take > n - copied {
                take = n - copied
            }
            copy_bytes(SCRATCH, inside, dest, at + copied, take);
            copied += take;
            inside = 0;
            if sec == mask && copied < n {
                sec = 0;
                c = if f.contiguous { c + 1 } else { self.next(c) }
            } else {
                sec += 1
            }
        }
        true
    }
}

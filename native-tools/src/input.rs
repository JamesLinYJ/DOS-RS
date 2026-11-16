//! Table-driven GB18030 input for the real-mode command line.
use crate::{key, put, text};
use dosrs_realmode::{
    self as pc,
    system::{File, Media, dw, w},
};
const BUF: u16 = 0x3520;
struct Index {
    media: Media,
    file: File,
    dense: u16,
    pairs: u16,
    ranges: u16,
    syllables: u16,
}
impl Index {
    fn open() -> Option<Self> {
        let media = Media::device(pc::boot_unit())?;
        let mut file = media.lookup(b"INPUT   DAT")?;
        media.check_extent(&mut file);
        if !media.fetch(&file, 0, 10, BUF, 0) {
            return None;
        }
        let dense = match dw(BUF, 0) {
            0x31494247 => 0,
            0x32494247 => 0x9fa6 - 0x4e00,
            _ => return None,
        };
        Some(Self {
            dense,
            media,
            file,
            pairs: w(BUF, 4),
            ranges: w(BUF, 6),
            syllables: w(BUF, 8),
        })
    }
    fn get(&self, pos: u32, n: u16) -> bool {
        self.media.fetch(&self.file, pos, n, BUF, 0)
    }
    fn pairs_at(&self) -> u32 {
        10 + (self.dense as u32) * 2
    }
    fn ranges_at(&self) -> u32 {
        self.pairs_at() + (self.pairs as u32) * 4
    }
    fn syllables_at(&self) -> u32 {
        self.ranges_at() + (self.ranges as u32) * 8
    }
    fn encode(&self, cp: u32, buf: &mut [u8; 4]) -> usize {
        if cp < 32 || cp == 127 || cp > 0x10ffff || (cp >= 0xd800 && cp <= 0xdfff) {
            return 0;
        }
        if cp < 128 {
            buf[0] = cp as u8;
            return 1;
        }
        if cp >= 0x4e00 && cp - 0x4e00 < (self.dense as u32) {
            if !self.get(10 + (cp - 0x4e00) * 2, 2) {
                return 0;
            }
            buf[0] = pc::byte(BUF, 0) as u8;
            buf[1] = pc::byte(BUF, 1) as u8;
            return 2;
        }
        let mut pointer;
        if cp < 65536 {
            let mut lo = 0;
            let mut hi = self.pairs;
            while lo < hi {
                let m = lo + ((hi - lo) >> 1);
                if !self.get(self.pairs_at() + (m as u32) * 4, 4) {
                    return 0;
                }
                let val = w(BUF, 0) as u32;
                if val < cp {
                    lo = m + 1
                } else if val > cp {
                    hi = m
                } else {
                    buf[0] = pc::byte(BUF, 2) as u8;
                    buf[1] = pc::byte(BUF, 3) as u8;
                    return 2;
                }
            }
            lo = 0;
            hi = self.ranges;
            loop {
                if lo >= hi {
                    return 0;
                }
                let m = lo + ((hi - lo) >> 1);
                if !self.get(self.ranges_at() + (m as u32) * 8, 8) {
                    return 0;
                }
                let a = w(BUF, 0) as u32;
                if cp < a {
                    hi = m
                } else if cp > w(BUF, 2) as u32 {
                    lo = m + 1
                } else {
                    pointer = dw(BUF, 4) + cp - a;
                    break;
                }
            }
        } else {
            pointer = 189000 + cp - 65536
        }
        buf[3] = 48 + divide(&mut pointer, 10);
        buf[2] = 129 + divide(&mut pointer, 126);
        buf[1] = 48 + divide(&mut pointer, 10);
        buf[0] = 129 + pointer as u8;
        4
    }
    fn pinyin(&self) -> Option<u32> {
        let mut name = [0u8; 8];
        let mut n = 0;
        loop {
            let c = key();
            if c == 27 {
                return None;
            }
            if c == 13 {
                break;
            }
            if c == 8 && n > 0 {
                n -= 1;
                name[n] = 0;
                text("\x08 \x08");
                continue;
            }
            if c >= 97 && c <= 122 && n < 7 {
                name[n] = c as u8;
                n += 1;
                put(c)
            }
        }
        if n == 0 {
            return None;
        }
        let base = self.syllables_at();
        let mut lo = 0;
        let mut hi = self.syllables;
        loop {
            if lo >= hi {
                return None;
            }
            let m = lo + ((hi - lo) >> 1);
            if !self.get(base + m as u32 * 12, 12) {
                return None;
            }
            let mut i = 0;
            while i < 8 && name[i] == pc::byte(BUF, i as u16) as u8 {
                i += 1;
            }
            if i < 8 {
                if name[i] > pc::byte(BUF, i as u16) as u8 {
                    lo = m + 1
                } else {
                    hi = m
                }
                continue;
            }
            let index = w(BUF, 8);
            let count = w(BUF, 10);
            let mut page = 0;
            let mut codes = [0u16; 9];
            loop {
                text("\r\n");
                let mut j = 0;
                while j < 9 && page + j < count {
                    let at = base
                        + self.syllables as u32 * 12
                        + (index as u32 + page as u32 + j as u32) * 2;
                    if !self.get(at, 2) {
                        return None;
                    }
                    let cp = w(BUF, 0);
                    codes[j as usize] = cp;
                    put(49 + j);
                    let mut bytes = [0u8; 4];
                    let size = self.encode(cp as u32, &mut bytes);
                    let mut k = 0;
                    while k < size {
                        put(bytes[k] as u16);
                        k += 1;
                    }
                    put(32);
                    j += 1;
                }
                text("  [ / ]   Esc");
                let c = key();
                if c >= 49 && c < 49 + j {
                    return Some(codes[(c - 49) as usize] as u32);
                }
                if c == 27 {
                    return None;
                }
                if c == 93 && page + 9 < count {
                    page += 9
                }
                if c == 91 && page >= 9 {
                    page -= 9
                }
            }
        }
    }
}
fn divide(n: &mut u32, d: u16) -> u8 {
    let mut bit = 32;
    let mut q = 0u32;
    let mut r = 0u16;
    while bit > 0 {
        bit -= 1;
        r = (r << 1) | ((*n >> bit) & 1) as u16;
        if r >= d {
            r -= d;
            q |= 1u32 << bit;
        }
    }
    *n = q;
    r as u8
}
fn unicode() -> Option<u32> {
    text("\r\nU+");
    let mut cp = 0u32;
    let mut n = 0;
    loop {
        let c = key();
        if c == 27 {
            return None;
        }
        if c == 13 {
            return if n > 0 { Some(cp) } else { None };
        }
        if c == 8 && n > 0 {
            cp >>= 4;
            n -= 1;
            text("\x08 \x08");
            continue;
        }
        let digit = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => continue,
        };
        if n < 6 {
            cp = (cp << 4) | digit as u32;
            n += 1;
            put(c);
        }
    }
}
pub trait Labels {
    fn pinyin(&self);
    fn unavailable(&self);
}
pub struct ShellLabels;
impl Labels for ShellLabels {
    fn pinyin(&self) {
        text("Pinyin");
    }
    fn unavailable(&self) {
        text("INPUT.DAT unavailable");
    }
}
pub fn compose(keycode: u16, bytes: &mut [u8; 4]) -> usize {
    compose_with(keycode, bytes, &ShellLabels)
}
pub fn compose_with<L: Labels>(keycode: u16, bytes: &mut [u8; 4], labels: &L) -> usize {
    let Some(table) = Index::open() else {
        text("\r\n");
        labels.unavailable();
        return 0;
    };
    let cp = if keycode == 16 {
        text("\r\n");
        labels.pinyin();
        text(" > ");
        table.pinyin()
    } else {
        unicode()
    };
    if let Some(cp) = cp {
        table.encode(cp, bytes)
    } else {
        0
    }
}

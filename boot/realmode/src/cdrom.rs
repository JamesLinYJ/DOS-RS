//! Read-only ISO 9660 paths and DOS search records over the resident ATAPI sector service.
use super::{
    byte, control, copy_bytes,
    disk::{match_mask, pack_mask, unpack_name},
    optical::BUF,
    store,
    system::{dw, sd, sw},
};
const REQUEST: u16 = 0x35e0;
pub fn probe() -> bool {
    byte(0x3800, 270) != 0 && control(16, 0, 0, 0) != 0
}
pub fn present() -> bool {
    control(17, 0, 0, 0) != 0
}
pub fn letter() -> u16 {
    let n = byte(0x40, 0x75);
    2 + if n == 0 { 1 } else { n }
}
const STATE: u16 = 0x35c0;
const SHORT: u16 = 0x3c0; // 11-byte scratch in COMMAND, beyond both pathname buffers.
fn read(lba: u32) -> bool {
    lba < capacity() && control(18, lba as u16, (lba >> 16) as u16, 0) != 0
}
pub struct Entry {
    pub lba: u32,
    pub size: u32,
    pub attr: u16,
    pub time: u16,
    pub date: u16,
}
fn root() -> Entry {
    Entry {
        lba: dw(STATE, 4),
        size: dw(STATE, 8),
        attr: 17,
        time: 0,
        date: 0,
    }
}
fn entry(p: u16) -> Entry {
    let flags = byte(BUF, p + 25);
    let year = byte(BUF, p + 18);
    Entry {
        lba: dw(BUF, p + 2) + u32::from(byte(BUF, p + 1)),
        size: dw(BUF, p + 10),
        attr: 1 | ((flags & 1) << 1) | ((flags & 2) << 3),
        time: (byte(BUF, p + 21) << 11) | (byte(BUF, p + 22) << 5) | (byte(BUF, p + 23) >> 1),
        date: if (80..=207).contains(&year) {
            ((year - 80) << 9) | (byte(BUF, p + 19) << 5) | byte(BUF, p + 20)
        } else {
            0
        },
    }
}
struct Records<'a> {
    dir: &'a Entry,
    pos: u32,
    sector: u32,
    skipping_extents: bool,
}
impl<'a> Records<'a> {
    fn new(dir: &'a Entry, pos: u32) -> Self {
        Self {
            dir,
            pos,
            sector: u32::MAX,
            skipping_extents: false,
        }
    }
    fn next(&mut self) -> Result<Option<u16>, u16> {
        while self.pos < self.dir.size {
            let lba = self.dir.lba + (self.pos >> 11);
            if self.sector != lba {
                if !read(lba) {
                    return Err(30);
                }
                self.sector = lba;
            }
            let p = (self.pos & 2047) as u16;
            let n = byte(BUF, p);
            if n == 0 {
                let skip = u32::from(2048 - p);
                if self.dir.size - self.pos <= skip {
                    break;
                }
                self.pos += skip;
                continue;
            }
            if n < 34
                || n > 2048 - p
                || u32::from(n) > self.dir.size - self.pos
                || byte(BUF, p + 32) == 0
                || 33 + byte(BUF, p + 32) > n
            {
                return Err(30);
            }
            self.pos += u32::from(n);
            // Associated files, interleaved files and multi-extent records cannot
            // be opened by the contiguous-extent DOS read interface.
            let start = dw(BUF, p + 2);
            let extra = u32::from(byte(BUF, p + 1));
            let len = dw(BUF, p + 10);
            let blocks = (len >> 11) + u32::from(len & 2047 != 0);
            if start > capacity()
                || extra > capacity() - start
                || blocks > capacity() - start - extra
            {
                return Err(30);
            }
            let flags = byte(BUF, p + 25);
            let continued = self.skipping_extents;
            self.skipping_extents = flags & 128 != 0;
            if !continued && !self.skipping_extents && flags & 4 == 0 && byte(BUF, p + 26) == 0 {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }
}
fn name_end(p: u16) -> u16 {
    let start = p + 33;
    let mut end = start;
    while end - start < byte(BUF, p + 32) && byte(BUF, end) != 59 {
        end += 1;
    }
    if end > start && byte(BUF, end - 1) == 46 {
        end -= 1;
    }
    end
}
/// Input is the absolute path produced by the drive's CDS canonicalizer.
pub fn lookup(seg: u16, mut at: u16, end: u16) -> Option<Entry> {
    if !present() {
        return None;
    }
    let mut result = root();
    while at < end {
        if byte(seg, at) == 92 {
            at += 1;
            continue;
        }
        if result.attr & 16 == 0 {
            return None;
        }
        let start = at;
        while at < end && byte(seg, at) != 92 {
            at += 1;
        }
        let mut records = Records::new(&result, 0);
        loop {
            let p = records.next().ok()??;
            let base = p + 33;
            if name_end(p) - base != at - start {
                continue;
            }
            let mut i = 0;
            while i < at - start && byte(seg, start + i) == byte(BUF, base + i) {
                i += 1;
            }
            if i == at - start {
                result = entry(p);
                break;
            }
        }
    }
    Some(result)
}
pub fn capacity() -> u32 {
    dw(STATE, 16)
}
pub fn volume_info(seg: u16, at: u16) -> bool {
    if !read(dw(STATE, 12)) {
        return false;
    }
    sd(seg, at + 2, 0); // ISO 9660 has a volume identifier but no DOS serial number.
    copy_bytes(BUF, 40, seg, at + 6, 11);
    let mut i = 0;
    while i < 8 {
        store(seg, at + 17 + i, u16::from(b"CDFS    "[usize::from(i)]));
        i += 1;
    }
    true
}
pub fn first(seg: u16, start: u16, end: u16, dest: u16, at: u16, wanted: u16, drive: u16) -> u16 {
    let mut leaf = end;
    while leaf > start && byte(seg, leaf - 1) != 92 {
        leaf -= 1;
    }
    let Some(dir) = lookup(seg, start, leaf) else {
        return 3;
    };
    if dir.attr & 16 == 0 || !pack_mask(seg, leaf, end, dest, at + 1) {
        return 3;
    }
    store(dest, at, wanted);
    sd(dest, at + 12, dir.lba);
    sd(dest, at + 16, 0);
    store(dest, at + 20, 0xf0 | drive);
    advance(&dir, dest, at)
}
pub fn next(dest: u16, at: u16) -> u16 {
    let lba = dw(dest, at + 12);
    // The mandatory first directory record describes the directory itself.
    if !present()
        || lba >= capacity()
        || !read(lba)
        || byte(BUF, 0) < 34
        || byte(BUF, 32) != 1
        || byte(BUF, 33) != 0
    {
        return 18;
    }
    let dir = entry(0);
    if dir.lba != lba || dir.attr & 16 == 0 {
        return 18;
    }
    advance(&dir, dest, at)
}
fn advance(dir: &Entry, dest: u16, at: u16) -> u16 {
    let mut records = Records::new(dir, dw(dest, at + 16));
    let wanted = byte(dest, at);
    loop {
        let p = match records.next() {
            Ok(Some(p)) => p,
            Ok(None) => {
                store(dest, at + 20, 0);
                return 18;
            }
            Err(e) => return e,
        };
        let start = p + 33;
        let e = entry(p);
        if wanted == 8 || e.attr & 0x16 & !wanted != 0 {
            continue;
        }
        let id = byte(BUF, start);
        if id <= 1 && byte(BUF, p + 32) == 1 {
            if dir.lba == dw(STATE, 4) {
                continue;
            }
            let mut i = 0;
            while i < 11 {
                store(super::COMMAND, SHORT + i, if i <= id { 46 } else { 32 });
                i += 1;
            }
        } else if !pack_mask(BUF, start, name_end(p), super::COMMAND, SHORT) {
            continue;
        }
        if !match_mask(dest, at + 1, super::COMMAND, SHORT) {
            continue;
        }
        sd(dest, at + 16, records.pos);
        store(dest, at + 21, e.attr);
        sw(dest, at + 22, e.time);
        sw(dest, at + 24, e.date);
        sd(dest, at + 26, e.size);
        unpack_name(super::COMMAND, SHORT, dest, at + 30);
        return 0;
    }
}

pub fn fetch(lba: u32, size: u32, pos: u32, count: u16, seg: u16, at: u16) -> u32 {
    sd(REQUEST, 0, lba);
    sd(REQUEST, 4, size);
    sd(REQUEST, 8, pos);
    sw(REQUEST, 12, count);
    sw(REQUEST, 14, seg);
    sw(REQUEST, 16, at);
    if control(22, 0, 0, 0) != 0 {
        dw(REQUEST, 20)
    } else {
        u32::MAX
    }
}

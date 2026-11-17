//! Legacy IDE ATAPI packet transport and read-only ISO 9660 directories.
use super::{
    byte, copy_bytes, store,
    system::{dw, sd, sw, w},
};
// Dedicated 2 KiB transfer buffer between the sector buffer and FAT scratch.
const BUF: u16 = 0x3020;
const STATE: u16 = 0x35c0;
unsafe extern "Rust" {
    fn read_port(p: u16) -> u16;
    fn write_port(p: u16, v: u16);
    fn write_word_port(p: u16, v: u16);
    fn read_words_port(p: u16, seg: u16, at: u16, count: u16);
}
fn inp(p: u16) -> u16 {
    unsafe { read_port(p) }
}
fn out(p: u16, v: u16) {
    unsafe { write_port(p, v) }
}
fn status(base: u16, drq: bool) -> bool {
    let mut n = 0;
    while n < 60000 {
        let s = inp(base + 7);
        if s == 0 || s == 255 {
            return false;
        }
        if s & 128 == 0 {
            if s & 1 != 0 {
                return false;
            }
            if !drq || s & 8 != 0 {
                return true;
            }
        }
        n += 1;
    }
    false
}
fn send(base: u16, word: u16) {
    unsafe { write_word_port(base, word) }
}
fn read(lba: u32) -> bool {
    let base = w(STATE, 0);
    if base == 0 {
        return false;
    }
    out(base + 6, byte(STATE, 2));
    let mut i = 0;
    while i < 4 {
        inp(base + 0x206);
        i += 1;
    }
    if !status(base, false) {
        return false;
    }
    out(base + 1, 0);
    out(base + 4, 0);
    out(base + 5, 8);
    out(base + 7, 0xa0);
    if !status(base, true) || inp(base + 2) & 3 != 1 {
        return false;
    }
    send(base, 0xa8);
    send(
        base,
        ((lba >> 24) as u16) | (((lba >> 16) as u16 & 255) << 8),
    );
    send(base, ((lba >> 8) as u16 & 255) | ((lba as u16 & 255) << 8));
    send(base, 0);
    send(base, 256);
    send(base, 0);
    let mut used = 0;
    while used < 2048 {
        if !status(base, true) || inp(base + 2) & 3 != 2 {
            return false;
        }
        let n = inp(base + 4) | (inp(base + 5) << 8);
        if n == 0 || n & 1 != 0 || n > 2048 - used {
            return false;
        }
        unsafe { read_words_port(base, BUF, used, n >> 1) };
        used += n;
    }
    status(base, false)
}
pub fn probe() -> bool {
    sw(STATE, 0, 0);
    let mut n = 0;
    while n < 4 {
        let base = if n < 2 { 0x1f0 } else { 0x170 };
        let unit = if n & 1 == 0 { 0xa0 } else { 0xb0 };
        out(base + 6, unit);
        let mut spin = 0;
        while spin < 4 {
            inp(base + 0x206);
            spin += 1
        }
        let initial = inp(base + 7);
        if initial != 255 {
            out(base + 7, 0xa1);
            if !status(base, true) {
                n += 1;
                continue;
            }
            unsafe { read_words_port(base, BUF, 0, 256) }
            if w(BUF, 0) & 0x9f00 != 0x8500 {
                n += 1;
                continue;
            }

            sw(STATE, 0, base);
            store(STATE, 2, unit);
            let mut sector = 16;
            while sector < 32 && read(sector) {
                if byte(BUF, 1) != 67
                    || byte(BUF, 2) != 68
                    || byte(BUF, 3) != 48
                    || byte(BUF, 4) != 48
                    || byte(BUF, 5) != 49
                {
                    break;
                }
                if byte(BUF, 0) == 1 && w(BUF, 128) == 2048 {
                    let root = dw(BUF, 158);
                    let size = dw(BUF, 166);
                    if root > 0 && size > 0 {
                        sd(STATE, 4, root);
                        sd(STATE, 8, size);
                        sd(STATE, 12, root);
                        sd(STATE, 16, size);
                        return true;
                    }
                }
                if byte(BUF, 0) == 255 {
                    break;
                }
                sector += 1;
            }
        }
        n += 1;
    }
    sw(STATE, 0, 0);
    false
}
pub fn present() -> bool {
    w(STATE, 0) != 0
}
pub fn letter() -> u16 {
    let count = byte(0x40, 0x75);
    2 + if count == 0 { 1 } else { count }
}
fn upper(c: u16) -> u16 {
    if c >= 97 && c <= 122 { c - 32 } else { c }
}
fn match_name(seg: u16, at: u16, n: u16, p: u16, len: u16) -> bool {
    if len == 1 && byte(BUF, p) <= 1 {
        return (byte(BUF, p) == 0 && n == 1 && byte(seg, at) == 46)
            || (byte(BUF, p) == 1 && n == 2 && byte(seg, at) == 46 && byte(seg, at + 1) == 46);
    }
    let mut i = 0;
    while i < len && byte(BUF, p + i) != 59 {
        if i == n {
            return byte(BUF, p + i) == 46 && (i + 1 == len || byte(BUF, p + i + 1) == 59);
        }
        if upper(byte(seg, at + i)) != byte(BUF, p + i) {
            return false;
        }
        i += 1;
    }
    i == n
}
fn directory(lba: u32, size: u32, seg: u16, at: u16, n: u16, list: bool) -> bool {
    let mut offset = 0;
    while offset < size {
        if !read(lba + (offset >> 11)) {
            return false;
        }
        let mut p = 0;
        while p < 2048 && offset + (p as u32) < size {
            let length = byte(BUF, p);
            if length == 0 {
                break;
            }
            if length < 34 || length > 2048 - p {
                return false;
            }
            let len = byte(BUF, p + 32);
            if 33 + len > length {
                return false;
            }
            if list {
                if len != 1 || byte(BUF, p + 33) > 1 {
                    let mut i = 0;
                    while i < len && byte(BUF, p + 33 + i) != 59 {
                        super::put(byte(BUF, p + 33 + i));
                        i += 1;
                    }
                    super::print("\r\n");
                }
            } else if match_name(seg, at, n, p + 33, len) {
                sd(STATE, 20, dw(BUF, p + 2));
                sd(STATE, 24, dw(BUF, p + 10));
                store(STATE, 28, byte(BUF, p + 25));
                return true;
            }
            p += length;
        }
        offset += 2048;
    }
    list
}
pub fn list() -> bool {
    present() && directory(dw(STATE, 12), dw(STATE, 16), 0, 0, 0, true)
}
pub fn find(seg: u16, mut at: u16, end: u16, dir: bool) -> bool {
    if !present() {
        return false;
    }
    let rooted = at < end && byte(seg, at) == 92;
    let mut lba = dw(STATE, if rooted { 4 } else { 12 });
    let mut size = dw(STATE, if rooted { 8 } else { 16 });
    if rooted {
        at += 1;
    }
    if at == end && !dir {
        return false;
    }
    while at < end {
        let start = at;
        while at < end && byte(seg, at) != 92 {
            at += 1;
        }
        if at == start || !directory(lba, size, seg, start, at - start, false) {
            return false;
        }
        lba = dw(STATE, 20);
        size = dw(STATE, 24);
        let folder = byte(STATE, 28) & 2 != 0;
        if (at < end || dir) != folder {
            return false;
        }
        if at < end {
            at += 1;
        }
    }
    if dir {
        sd(STATE, 12, lba);
        sd(STATE, 16, size)
    }
    true
}
pub fn extent() -> u32 {
    dw(STATE, 20)
}
pub fn length() -> u32 {
    dw(STATE, 24)
}
pub fn fetch(lba: u32, size: u32, pos: u32, len: u16, seg: u16, at: u16) -> u32 {
    if pos >= size {
        return 0;
    }
    let remain = size - pos;
    let n = if remain < len as u32 {
        remain as u16
    } else {
        len
    };
    if at as u32 + n as u32 > 65536 {
        return u32::MAX;
    }
    let mut done = 0;
    while done < n {
        let p = pos + done as u32;
        let off = (p & 2047) as u16;
        let mut take = 2048 - off;
        if take > n - done {
            take = n - done
        }
        if !read(lba + (p >> 11)) {
            return u32::MAX;
        }
        copy_bytes(BUF, off, seg, at + done, take);
        done += take;
    }
    n as u32
}
pub fn display() -> bool {
    let base = extent();
    let total = length();
    let mut pos = 0;
    while pos < total {
        if !read(base + (pos >> 11)) {
            return false;
        }
        let mut i = 0;
        while i < 2048 && pos < total {
            super::put(byte(BUF, i));
            i += 1;
            pos += 1;
        }
    }
    true
}

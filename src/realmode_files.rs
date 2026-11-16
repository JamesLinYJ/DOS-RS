//! File handles for a native DOS process. Read positions use full 32-bit
//! offsets; a single near-segment write staging area is shared by the process.
use dosrs_realmode::{self as pc, disk::Volume};
const TABLE: u16 = 0x3600;
const STAGE: u16 = super::mz::FILE;
const FIRST: u16 = 5;
const END: u16 = 13;
const LIMIT: u16 = 0xfe00;
fn b(seg: u16, at: u16) -> u16 {
    pc::byte(seg, at)
}
fn w(seg: u16, at: u16) -> u16 {
    b(seg, at) | b(seg, at.wrapping_add(1)) << 8
}
fn set(seg: u16, at: u16, x: u16) {
    pc::store(seg, at, x);
    pc::store(seg, at.wrapping_add(1), x >> 8);
}
fn d(at: u16) -> u32 {
    w(TABLE, at) as u32 | ((w(TABLE, at + 2) as u32) << 16)
}
fn set_d(at: u16, v: u32) {
    set(TABLE, at, v as u16);
    set(TABLE, at + 2, (v >> 16) as u16);
}
fn slot(fd: u16) -> u16 {
    32 + (fd - FIRST) * 32
}
pub fn begin(hard: bool) {
    set(TABLE, 0, hard as u16);
    set(TABLE, 2, 0);
    let mut f = FIRST;
    while f < END {
        set(TABLE, slot(f), 0);
        f += 1;
    }
}
fn close(vol: &mut Volume, fd: u16) -> bool {
    let s = slot(fd);
    if w(TABLE, 2) == fd {
        let mut i = 0;
        while i < 11 {
            pc::store(pc::COMMAND, 128 + i, b(TABLE, s + 16 + i));
            i += 1;
        }
        let current = vol.directory();
        vol.restore(d(s + 28));
        let saved = vol.save(STAGE, d(s + 8) as u16);
        vol.restore(current);
        if !saved {
            return false;
        }
        set(TABLE, 2, 0);
    }
    set(TABLE, s, 0);
    true
}
pub fn finish(vol: &mut Volume) {
    let f = w(TABLE, 2);
    if f != 0 && !close(vol, f) {
        pc::print("File close failed\r\n");
    }
}
fn name(vol: &mut Volume, seg: u16, addr: u16) -> bool {
    let mut at = addr;
    let mut hard = w(TABLE, 0) != 0;
    if b(seg, at.wrapping_add(1)) == 58 {
        let c = b(seg, at);
        hard = c == 67 || c == 99;
        at = at.wrapping_add(2);
    }
    if !hard {
        return false;
    }
    let mut n = 0;
    while n < 128 && b(seg, at.wrapping_add(n)) != 0 {
        n += 1;
    }
    n < 128 && vol.path(seg, at, n, false)
}
fn open(vol: &mut Volume, creating: bool, access: u16) -> u32 {
    let mut fd = FIRST;
    while fd < END && w(TABLE, slot(fd)) != 0 {
        fd += 1;
    }
    if fd == END {
        return failure(4);
    }
    if access > 2 || (access != 0 && w(TABLE, 2) != 0) {
        return failure(5);
    }
    let mut head = 0;
    let mut size = 0;
    if creating {
        if !vol.save(STAGE, 0) {
            return failure(5);
        }
    } else if !vol.describe(&mut head, &mut size) {
        return failure(2);
    }
    let s = slot(fd);
    set_d(s + 28, vol.directory());
    if access != 0 {
        if size > LIMIT as u32 || vol.read_at(head, size, 0, size as u16, STAGE, 0) != size {
            return failure(8);
        }
        let mut i = 0;
        while i < 11 {
            pc::store(TABLE, s + 16 + i, b(pc::COMMAND, 128 + i));
            i += 1;
        }
        set(TABLE, 2, fd);
    }
    set(TABLE, s + 2, 2);
    set(TABLE, s, access + 1);
    set_d(s + 4, head);
    set_d(s + 8, size);
    set_d(s + 12, 0);
    fd as u32
}
fn removable(seg: u16, at: u16, n: u16, drive: u16, write: bool) -> u32 {
    if write {
        return failure(19);
    }
    let head;
    let size;
    if drive == pc::cdrom::letter() {
        if !pc::cdrom::find(seg, at, at + n, false) {
            return failure(2);
        }
        head = pc::cdrom::extent();
        size = pc::cdrom::length();
    } else if drive < 2 && drive < pc::floppies() {
        let mut start = at;
        let mut length = n;
        if length > 0 && b(seg, start) == 92 {
            length -= 1;
            start += 1;
        }
        if !pc::filename(seg, start, length) || pc::control(23, drive, 0, 0) == 0 {
            return failure(2);
        }
        head = pc::system::dw(0x35e0, 0);
        size = pc::system::dw(0x35e0, 4);
    } else {
        return failure(15);
    }
    let mut fd = FIRST;
    while fd < END && w(TABLE, slot(fd)) != 0 {
        fd += 1;
    }
    if fd == END {
        return failure(4);
    }
    let s = slot(fd);
    set(TABLE, s, 1);
    set(TABLE, s + 2, drive);
    set_d(s + 4, head);
    set_d(s + 8, size);
    set_d(s + 12, 0);
    fd as u32
}
fn opened(vol: &mut Volume, ds: u16, dx: u16, ah: u16, al: u16) -> u32 {
    let mut at = dx;
    let drive = if b(ds, at.wrapping_add(1)) == 58 {
        let c = b(ds, at) & 0xdf;
        if !(65..=90).contains(&c) {
            return failure(15);
        }
        at = at.wrapping_add(2);
        c - 65
    } else {
        pc::byte(pc::COMMAND, 256)
    };
    if drive == 2 {
        if !vol.mounted() || !name(vol, ds, dx) {
            failure(3)
        } else {
            open(vol, ah == 0x3c, if ah == 0x3c { 2 } else { al & 7 })
        }
    } else {
        let mut n = 0;
        while n < 127 && b(ds, at.wrapping_add(n)) != 0 {
            n += 1;
        }
        if n == 127 || n > u16::MAX - at {
            return failure(3);
        }
        removable(ds, at, n, drive, ah == 0x3c || al & 7 != 0)
    }
}
fn failure(error: u16) -> u32 {
    0x10000 | error as u32
}
fn transfer(vol: &mut Volume, fd: u16, count: u16, seg: u16, at: u16, write: bool) -> u32 {
    if at as u32 + count as u32 > 65536 {
        return failure(5);
    }
    if write && (fd == 1 || fd == 2) {
        let mut i = 0;
        while i < count {
            pc::put(b(seg, at + i));
            i += 1;
        }
        return count as u32;
    }
    if fd < FIRST || fd >= END {
        return failure(6);
    }
    let s = slot(fd);
    let access = w(TABLE, s);
    if access == 0 {
        return failure(6);
    }
    let pos = d(s + 12);
    let size = d(s + 8);
    let n = if write {
        if access == 1 {
            return failure(5);
        }
        if pos > LIMIT as u32 || count as u32 > LIMIT as u32 - pos {
            return failure(39);
        }
        let mut gap = size;
        while gap < pos {
            pc::store(STAGE, gap as u16, 0);
            gap += 1;
        }
        let mut i = 0;
        while i < count {
            pc::store(STAGE, pos as u16 + i, b(seg, at + i));
            i += 1;
        }
        let end = pos + count as u32;
        if end > size || count == 0 {
            set_d(s + 8, end);
        }
        count as u32
    } else {
        if access == 2 {
            return failure(5);
        }
        if w(TABLE, 2) == fd {
            let n = if pos >= size {
                0
            } else if size - pos < count as u32 {
                (size - pos) as u16
            } else {
                count
            };
            let mut i = 0;
            while i < n {
                pc::store(seg, at + i, b(STAGE, pos as u16 + i));
                i += 1;
            }
            n as u32
        } else {
            let drive = w(TABLE, s + 2);
            let n = if drive == 2 {
                vol.read_at(d(s + 4), size, pos, count, seg, at)
            } else if drive < 2 {
                let n = if pos >= size {
                    0
                } else if size - pos < count as u32 {
                    (size - pos) as u16
                } else {
                    count
                };
                let r = 0x35e0;
                pc::system::sd(r, 0, d(s + 4));
                pc::system::sd(r, 4, size);
                pc::system::sd(r, 8, pos);
                pc::system::sw(r, 12, n);
                pc::system::sw(r, 14, seg);
                pc::system::sw(r, 16, at);
                if n == 0 || pc::control(24, drive, 0, 0) != 0 {
                    n as u32
                } else {
                    u32::MAX
                }
            } else {
                pc::cdrom::fetch(d(s + 4), size, pos, count, seg, at)
            };
            if n == u32::MAX {
                return failure(23);
            }
            n
        }
    };
    set_d(s + 12, pos + n);
    n
}

/// SS:SP identifies nine saved registers followed by the real INT frame.
#[unsafe(no_mangle)]
pub fn dos16_service(vol: &mut Volume, ss: u16, sp: u16) {
    let ax = w(ss, sp);
    let ah = ax >> 8;
    let al = ax & 255;
    let bx = w(ss, sp + 2);
    let cx = w(ss, sp + 4);
    let dx = w(ss, sp + 6);
    let ds = w(ss, sp + 16);
    if ax == 0xf102 && bx >= 2 && bx <= 15 {
        set(ss, sp, pc::control(bx, cx, dx, 0));
        return;
    }
    if ax == 0xf100 {
        let n = if cx > 126 { 126 } else { cx };
        pc::copy_bytes(ds, dx, pc::COMMAND, 0, n);
        pc::store(pc::COMMAND, 257, n);
        return;
    }
    if ah == 8 {
        set(ss, sp, pc::key());
        return;
    }
    if ah == 0x19 {
        set(ss, sp, pc::byte(pc::COMMAND, 256));
        return;
    }
    if ah == 2 {
        pc::put(dx & 255);
        set(ss, sp, (ax & 0xff00) | (dx & 255));
        return;
    }
    if ah == 9 {
        let mut ptr = dx;
        loop {
            let c = b(ds, ptr);
            if c == 36 {
                break;
            }
            pc::put(c);
            ptr = ptr.wrapping_add(1);
            if ptr == dx {
                break;
            }
        }
        set(ss, sp, (ax & 0xff00) | 36);
        return;
    }
    if ah == 0x30 {
        set(ss, sp, crate::version::WORD);
        set(ss, sp + 2, if al == 1 { 0 } else { 0xff00 });
        set(ss, sp + 4, 0);
        return;
    }
    if ax == 0x3306 {
        set(ss, sp + 2, crate::version::WORD);
        set(ss, sp + 6, 0);
        return;
    }
    let current = vol.directory();
    let result = match ah {
        0x3c | 0x3d => opened(vol, ds, dx, ah, al),
        0x3e => {
            if bx < FIRST || bx >= END || w(TABLE, slot(bx)) == 0 {
                failure(6)
            } else if close(vol, bx) {
                0
            } else {
                failure(39)
            }
        }
        0x3f | 0x40 => transfer(vol, bx, cx, ds, dx, ah == 0x40),
        0x42 => {
            if bx < FIRST || bx >= END || w(TABLE, slot(bx)) == 0 {
                failure(6)
            } else if al > 2 {
                failure(1)
            } else {
                let s = slot(bx);
                let base = if al == 0 {
                    0
                } else if al == 1 {
                    d(s + 12)
                } else {
                    d(s + 8)
                };
                let delta = ((cx as u32) << 16) | dx as u32;
                let pos = base.wrapping_add(delta);
                if (cx & 0x8000 == 0 && pos < base) || (cx & 0x8000 != 0 && pos > base) {
                    failure(1)
                } else {
                    set_d(s + 12, pos);
                    set(ss, sp + 6, (pos >> 16) as u16);
                    pos as u16 as u32
                }
            }
        }
        _ => failure(1),
    };
    vol.restore(current);
    set(ss, sp, result as u16);
    let flags = w(ss, sp + 22);
    set(
        ss,
        sp + 22,
        if result >= 0x10000 {
            flags | 1
        } else {
            flags & !1
        },
    );
}

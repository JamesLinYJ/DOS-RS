//! Process-local JFT handles reference system file descriptions. FAT file data
//! travels directly between guest buffers and sectors, including during EXEC.
#[path = "realmode_char.rs"]
mod char_device;
use dosrs_realmode::{self as pc, disk::Volume};
use pc::dos_tables::{LIST, LIST_SEG, SEG as TABLE, entry as slot, handle, jft_len, set_handle};
const FIRST: u16 = 5;
const SLOTS: u16 = pc::dos_tables::COUNT;
const CLOSED: u16 = 255;
use pc::{
    byte as b,
    system::{dw, sd, sw as set, w},
};
fn d(at: u16) -> u32 {
    dw(TABLE, at)
}
fn set_d(at: u16, value: u32) {
    sd(TABLE, at, value)
}
pub fn begin(hard: bool) {
    super::arena::begin();
    set(TABLE, 0, hard as u16);
    let mut sys = 0;
    while sys < SLOTS {
        set(TABLE, slot(sys), u16::from(sys < FIRST));
        sys += 1;
    }
}
fn free_handle(psp: u16) -> u16 {
    let limit = jft_len(psp);
    let mut fd = 0;
    while fd < limit {
        if handle(psp, fd) == CLOSED {
            return fd;
        }
        fd += 1;
    }
    CLOSED
}
fn free_slot() -> u16 {
    let mut sys = FIRST;
    while sys < SLOTS && w(TABLE, slot(sys)) != 0 {
        sys += 1;
    }
    sys
}
fn close(psp: u16, fd: u16) -> bool {
    let sys = handle(psp, fd);
    if sys >= SLOTS {
        return false;
    }
    let s = slot(sys);
    let refs = w(TABLE, s);
    if refs == 0 {
        return false;
    }
    if sys >= FIRST && w(TABLE, s + 5) & 0x80 != 0 {
        return pc::control(27, psp, fd, 0) == 1;
    }
    set(TABLE, s, refs - 1);
    set_handle(psp, fd, CLOSED);
    true
}
pub fn inherit(psp: u16) {
    let mut fd = 0;
    let limit = jft_len(psp);
    while fd < limit {
        let sys = handle(psp, fd);
        if sys < SLOTS {
            let s = slot(sys);
            let refs = w(TABLE, s);
            if refs != 0 && w(TABLE, s + 2) & 0x80 == 0 {
                set(TABLE, s, refs + 1);
            } else {
                set_handle(psp, fd, CLOSED);
            }
        }
        fd += 1;
    }
}
pub fn reap(psp: u16) {
    let mut fd = 0;
    let limit = jft_len(psp);
    while fd < limit {
        if handle(psp, fd) != CLOSED {
            close(psp, fd);
        }
        fd += 1;
    }
}
pub fn finish() {
    reap(super::arena::current());
    super::arena::release();
}
fn duplicate(psp: u16, fd: u16, dest: u16, force: bool) -> u32 {
    let sys = handle(psp, fd);
    if sys >= SLOTS || w(TABLE, slot(sys)) == 0 {
        return failure(6);
    }
    let target = if force { dest } else { free_handle(psp) };
    if target >= jft_len(psp) {
        return failure(if force { 6 } else { 4 });
    }
    if target != fd {
        if handle(psp, target) != CLOSED && !close(psp, target) {
            return failure(6);
        }
        let s = slot(sys);
        set(TABLE, s, w(TABLE, s) + 1);
        set_handle(psp, target, sys);
    }
    u32::from(if force { 0 } else { target })
}
pub(super) fn name(vol: &mut Volume, seg: u16, addr: u16) -> bool {
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
// FAT identities are the sector and offset of the live directory entry.
// Refresh from that entry so independent opens see writes and truncation.
fn metadata(vol: &Volume, sys: u16) -> u16 {
    let s = slot(sys);
    let mut first = 0;
    let mut size = 0;
    let mut attr = 0;
    let error = vol.file_stat(
        d(s + 0x1b),
        b(TABLE, s + 0x1f) << 5,
        &mut first,
        &mut size,
        &mut attr,
    );
    if error == 0 {
        set_d(s + 0x11, size);
        set_d(s + 0x2b, first);
        pc::store(TABLE, s + 4, attr);
    }
    error
}
fn open(vol: &mut Volume, creating: bool, mode: u16, attributes: u16) -> u32 {
    let psp = super::arena::current();
    let fd = free_handle(psp);
    let sys = free_slot();
    if fd == CLOSED || sys >= SLOTS {
        return failure(4);
    }
    if mode & 7 > 2 {
        return failure(12);
    }
    let mut lba = 0;
    let mut at = 0;
    let error = vol.file_open(creating, attributes, &mut lba, &mut at);
    if error != 0 {
        return failure(error);
    }
    let s = slot(sys);
    set_d(s + 0x1b, lba);
    pc::store(TABLE, s + 0x1f, at >> 5);
    let error = metadata(vol, sys);
    if error != 0 {
        return failure(error);
    }
    if !creating && mode & 7 != 0 && b(TABLE, s + 4) & 1 != 0 {
        return failure(5);
    }
    set(TABLE, s + 2, mode);
    set(TABLE, s + 5, 2);
    pc::copy_bytes(pc::COMMAND, 128, TABLE, s + 0x20, 11);
    set_d(s + 0x15, 0);
    set(TABLE, s, 1);
    set_handle(psp, fd, sys);
    u32::from(fd)
}
fn removable(seg: u16, at: u16, n: u16, drive: u16, mode: u16) -> u32 {
    if mode & 7 != 0 {
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
    let fd = free_handle(super::arena::current());
    let sys = free_slot();
    if fd == CLOSED || sys >= SLOTS {
        return failure(4);
    }
    let s = slot(sys);
    set(TABLE, s, 1);
    set(TABLE, s + 2, mode);
    set(TABLE, s + 5, drive);
    pc::copy_bytes(pc::COMMAND, 128, TABLE, s + 0x20, 11);
    set_handle(super::arena::current(), fd, sys);
    set_d(s + 0x2b, head);
    set_d(s + 0x11, size);
    set_d(s + 0x15, 0);
    fd as u32
}
fn opened(vol: &mut Volume, ds: u16, dx: u16, ah: u16, al: u16, attr: u16) -> u32 {
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
            open(vol, ah == 0x3c, if ah == 0x3c { 2 } else { al }, attr)
        }
    } else {
        let mut n = 0;
        while n < 127 && b(ds, at.wrapping_add(n)) != 0 {
            n += 1;
        }
        if n == 127 || n > u16::MAX - at {
            return failure(3);
        }
        removable(ds, at, n, drive, if ah == 0x3c { 2 } else { al })
    }
}
fn failure(error: u16) -> u32 {
    0x10000 | error as u32
}
fn transfer(vol: &mut Volume, sys: u16, count: u16, seg: u16, at: u16, write: bool) -> u32 {
    if write && u32::from(at) + u32::from(count) > 65536 {
        return failure(5);
    }
    if write && (sys == 1 || sys == 2) {
        let mut i = 0;
        while i < count {
            pc::put(b(seg, at + i));
            i += 1;
        }
        return u32::from(count);
    }
    if sys < FIRST || sys >= SLOTS {
        return failure(6);
    }
    let s = slot(sys);
    let access = w(TABLE, s + 2) & 7;
    if (write && access == 0) || (!write && access == 1) {
        return failure(5);
    }
    let drive = w(TABLE, s + 5);
    let pos = d(s + 0x15);
    let result = if write {
        if drive != 2 {
            return failure(19);
        }
        vol.file_write(d(s + 0x1b), b(TABLE, s + 0x1f) << 5, pos, count, seg, at)
    } else {
        if drive == 2 {
            let error = metadata(vol, sys);
            if error != 0 {
                return failure(error);
            }
        }
        let head = d(s + 0x2b);
        let size = d(s + 0x11);
        if pos >= size || count == 0 {
            return 0;
        }
        let n = if size - pos < u32::from(count) {
            (size - pos) as u16
        } else {
            count
        };
        // A caller can request the rest of a file with CX=FFFFh. Only bytes
        // actually read before EOF must fit in its segment.
        if u32::from(at) + u32::from(n) > 65536 {
            return failure(5);
        }
        let read = if drive == 2 {
            vol.read_at(head, size, pos, n, seg, at)
        } else if drive < 2 {
            let r = 0x35e0;
            sd(r, 0, head);
            sd(r, 4, size);
            sd(r, 8, pos);
            set(r, 12, n);
            set(r, 14, seg);
            set(r, 16, at);
            if pc::control(24, drive, 0, 0) != 0 {
                u32::from(n)
            } else {
                u32::MAX
            }
        } else {
            pc::cdrom::fetch(head, size, pos, n, seg, at)
        };
        if read == u32::MAX {
            return failure(23);
        }
        read
    };
    if result < 0x10000 {
        set_d(s + 0x15, pos + result);
        if write {
            let _ = metadata(vol, sys);
        }
    }
    result
}
fn seek(vol: &Volume, sys: u16, origin: u16, hi: u16, lo: u16, ss: u16, sp: u16) -> u32 {
    let s = slot(sys);
    if sys < FIRST {
        return failure(6);
    }
    let base = match origin {
        0 => 0,
        1 => d(s + 0x15),
        2 => {
            if w(TABLE, s + 5) == 2 {
                let error = metadata(vol, sys);
                if error != 0 {
                    return failure(error);
                }
            }
            d(s + 0x11)
        }
        _ => return failure(1),
    };
    let delta = (u32::from(hi) << 16) | u32::from(lo);
    let pos = base.wrapping_add(delta);
    if origin != 0 && ((hi & 0x8000 == 0 && pos < base) || (hi & 0x8000 != 0 && pos > base)) {
        return failure(1);
    }
    set_d(s + 0x15, pos);
    set(ss, sp + 6, (pos >> 16) as u16);
    u32::from(pos as u16)
}

/// SS:SP identifies nine saved registers followed by the real INT frame.
#[unsafe(no_mangle)]
pub fn dos16_service(vol: &mut Volume, ss: u16, sp: u16) {
    let in_dos = pc::dos_tables::SDA + 1;
    let old = b(LIST_SEG, in_dos);
    pc::store(LIST_SEG, in_dos, old + 1);
    service(vol, ss, sp);
    pc::store(LIST_SEG, in_dos, old);
}
fn service(vol: &mut Volume, ss: u16, sp: u16) {
    let ax = w(ss, sp);
    let ah = ax >> 8;
    let al = ax & 255;
    let bx = w(ss, sp + 2);
    let cx = w(ss, sp + 4);
    let dx = w(ss, sp + 6);
    let ds = w(ss, sp + 16);
    if matches!(ah, 0x3c..=0x40 | 0x42 | 0x44) {
        match pc::control(26, ss, sp, super::arena::current()) {
            1 => return,
            2 => {
                char_device::open(ss, sp, super::arena::current());
                return;
            }
            _ => (),
        }
    }
    let seg = pc::dos_tables::LIST_SEG;
    let sda = pc::dos_tables::SDA;
    set(seg, pc::dos_tables::SAVED_REGS, ds);
    set(seg, pc::dos_tables::SAVED_REGS + 2, bx);
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
    let invalid = (0x3e..=0x40).contains(&ah) || ah == 0x42;
    let mut keep_directory = false;
    let psp = super::arena::current();
    let sys = handle(psp, bx);
    let result = if invalid && (sys >= SLOTS || w(TABLE, slot(sys)) == 0) {
        failure(6)
    } else {
        match ah {
            0x59 if al == 0 => {
                set(ss, sp + 2, w(seg, sda + 6));
                set(ss, sp + 4, w(seg, sda + 8));
                u32::from(w(seg, sda + 4))
            }
            0x3b => {
                let mut at = dx;
                if b(ds, at.wrapping_add(1)) == 58 && b(ds, at) & 0xdf == 67 {
                    at = at.wrapping_add(2);
                }
                let mut n = 0;
                while n < 64 && b(ds, at.wrapping_add(n)) != 0 {
                    n += 1;
                }
                if n == 64 || at > u16::MAX - n || !vol.path(ds, at, n, true) {
                    failure(3)
                } else {
                    super::environment::path_change(ds, at, at + n);
                    keep_directory = true;
                    0
                }
            }
            0x43 if al == 0 => {
                if !name(vol, ds, dx) {
                    failure(3)
                } else if let Some(attr) = vol.attributes() {
                    set(ss, sp + 4, attr);
                    0
                } else {
                    failure(2)
                }
            }
            0x47 if dx & 255 == 0 || dx & 255 == 3 => {
                super::environment::cwd(ds, w(ss, sp + 8));
                0x100
            }
            0x4b if al == 0 => super::mz::child(vol, super::arena::current(), ss, sp),
            0x4d => u32::from(b(0x3900, 10)),
            0x51 | 0x62 => {
                set(ss, sp + 2, super::arena::current());
                u32::from(ax)
            }
            0x1a => {
                set(seg, sda + 12, dx);
                set(seg, sda + 14, ds);
                u32::from(ax)
            }
            0x2f => {
                set(ss, sp + 2, w(seg, sda + 12));
                set(ss, sp + 14, w(seg, sda + 14));
                u32::from(ax)
            }
            0x34 => {
                set(ss, sp + 2, sda + 1);
                set(ss, sp + 14, seg);
                u32::from(ax)
            }
            0x5d if al == 6 => {
                set(ss, sp + 16, seg);
                set(ss, sp + 8, sda);
                set(ss, sp + 4, 32);
                set(ss, sp + 6, 32);
                u32::from(ax)
            }
            0x55 => {
                // Create a child PSP without loading an image. Windows uses this
                // for tasks created after it has installed its own larger JFT.
                if dx == psp || dx < pc::PROGRAM {
                    failure(9)
                } else {
                    pc::control(25, dx, psp, w(ss, sp + 8));
                    pc::copy_bytes(psp, 0x80, dx, 0x80, 128);
                    sd(dx, 0x38, u32::from(psp) << 16);
                    inherit(dx);
                    super::arena::select(dx);
                    0xf0
                }
            }
            0x50 => {
                super::arena::select(bx);
                u32::from(ax)
            }
            0x52 => {
                set(ss, sp + 2, LIST);
                set(ss, sp + 14, LIST_SEG);
                u32::from(ax)
            }
            0x25 => {
                set(0, al * 4, dx);
                set(0, al * 4 + 2, ds);
                0
            }
            0x35 => {
                set(ss, sp + 2, w(0, al * 4));
                set(ss, sp + 14, w(0, al * 4 + 2));
                u32::from(ax)
            }
            0x48..=0x4a | 0x58 => {
                let mut room = 0;
                let result = super::arena::request(
                    if ah == 0x58 { ax } else { ah << 8 },
                    bx,
                    w(ss, sp + 14),
                    &mut room,
                );
                if result == 0x10008 {
                    set(ss, sp + 2, room);
                }
                result
            }
            0x3c | 0x3d => opened(vol, ds, dx, ah, al, cx),
            0x3e => {
                if close(psp, bx) {
                    0
                } else {
                    failure(6)
                }
            }
            0x45 | 0x46 => duplicate(psp, bx, cx, ah == 0x46),
            0x3f | 0x40 => transfer(vol, sys, cx, ds, dx, ah == 0x40),
            0x42 => seek(vol, sys, al, cx, dx, ss, sp),
            _ => failure(1),
        }
    };
    if !keep_directory {
        vol.restore(current);
    }
    if result >= 0x10000 {
        set(seg, sda + 4, result as u16);
        set(seg, sda + 6, 0);
        set(seg, sda + 8, 0);
    }
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

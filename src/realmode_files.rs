//! Process-local JFT handles reference system file descriptions. FAT file data
//! travels directly between guest buffers and sectors, including during EXEC.
#[path = "realmode_char.rs"]
mod char_device;
#[path = "realmode_clock.rs"]
mod clock;
#[path = "realmode_paths.rs"]
mod paths;
use dosrs_realmode::{self as pc, disk::Volume};
pub(super) use paths::executable;
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
pub fn begin() {
    super::arena::begin();
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
    set(TABLE, s + 5, vol.drive());
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
    if drive != pc::cdrom::letter() || !pc::cdrom::find(seg, at, at + n, false) {
        return failure(2);
    }
    let head = pc::cdrom::extent();
    let size = pc::cdrom::length();
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
fn path_service(main: &mut Volume, ah: u16, al: u16, ss: u16, sp: u16) -> u32 {
    let ds = w(ss, sp + 16);
    let dx = w(ss, sp + 6);
    let cx = w(ss, sp + 4);
    let path = match paths::Path::read(ds, dx) {
        Ok(p) => p,
        Err(e) => return failure(e),
    };
    if path.drive == pc::cdrom::letter() {
        return match ah {
            0x3b => paths::chdir(main, &path),
            0x3d => removable(ds, path.start, path.end - path.start, path.drive, al),
            0x43 if al == 0 && pc::cdrom::find(ds, path.start, path.end, false) => {
                set(ss, sp + 4, 1);
                0
            }
            0x43 => failure(2),
            _ => failure(19),
        };
    }
    let mut tmp = Volume::EMPTY;
    let vol = match paths::select(main, &mut tmp, path.drive) {
        Ok(v) => v,
        Err(e) => return failure(e),
    };
    let orig = vol.directory();
    let r = match ah {
        0x3b => paths::chdir(vol, &path),
        0x39 | 0x3a | 0x41 | 0x56 => paths::directory(vol, &path, ah, ss, sp),
        0x4e => paths::search(vol, Some(&path), ss, sp),
        _ if !path.fat_name(vol) => failure(3),
        0x3c | 0x3d | 0x5b => {
            if ah == 0x5b && vol.attributes().is_some() {
                failure(80)
            } else {
                open(vol, ah != 0x3d, if ah == 0x3d { al } else { 2 }, cx)
            }
        }
        0x43 if al == 0 => match vol.attributes() {
            Some(v) => {
                set(ss, sp + 4, v);
                0
            }
            None => failure(2),
        },
        0x43 if al == 1 => dos_result(vol.set_attributes(cx)),
        _ => failure(1),
    };
    if ah != 0x3b || r >= 0x10000 {
        vol.restore(orig)
    }
    r
}
fn dos_result(code: u16) -> u32 {
    if code == 0 { 0 } else { failure(code) }
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
    let mut floppy = Volume::EMPTY;
    let vol = if drive < 2 {
        match paths::select(vol, &mut floppy, drive) {
            Ok(v) => v,
            Err(e) => return failure(e),
        }
    } else {
        vol
    };
    let pos = d(s + 0x15);
    let result = if write {
        if drive > 2 {
            return failure(19);
        }
        vol.file_write(d(s + 0x1b), b(TABLE, s + 0x1f) << 5, pos, count, seg, at)
    } else {
        if drive <= 2 {
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
        let read = if drive <= 2 {
            vol.read_at(head, size, pos, n, seg, at)
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
fn seek(vol: &mut Volume, sys: u16, origin: u16, hi: u16, lo: u16, ss: u16, sp: u16) -> u32 {
    let s = slot(sys);
    if sys < FIRST {
        return failure(6);
    }
    let base = match origin {
        0 => 0,
        1 => d(s + 0x15),
        2 => {
            let drive = w(TABLE, s + 5);
            if drive <= 2 {
                let mut tmp = Volume::EMPTY;
                let vol = match paths::select(vol, &mut tmp, drive) {
                    Ok(v) => v,
                    Err(e) => return failure(e),
                };
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

fn volume_service(main: &mut Volume, ss: u16, sp: u16, ax: u16) -> u32 {
    let free_space = ax >> 8 == 0x36;
    if !free_space && ax != 0x6900 {
        return failure(1);
    }
    let unit = w(ss, sp + if free_space { 6 } else { 2 }) & 255;
    let drive = if unit == 0 {
        b(pc::COMMAND, 256)
    } else {
        unit - 1
    };
    let mut tmp = Volume::EMPTY;
    let v = match paths::select(main, &mut tmp, drive) {
        Ok(v) => v,
        Err(e) => return if free_space { 0xffff } else { failure(e) },
    };
    if free_space {
        let mut sectors = 0;
        let mut free = 0;
        let mut total = 0;
        if !v.free_space(&mut sectors, &mut free, &mut total) {
            return 0xffff;
        }
        set(ss, sp + 2, free);
        set(ss, sp + 4, 512);
        set(ss, sp + 6, total);
        u32::from(sectors)
    } else {
        let seg = w(ss, sp + 16);
        let at = w(ss, sp + 6);
        if at > u16::MAX - 24 || w(seg, at) != 0 {
            return failure(13);
        }
        if v.volume_info(seg, at) {
            0
        } else {
            failure(30)
        }
    }
}
/// Character functions follow the caller's standard handles. Temporarily use
/// the PSP's reserved byte for single-byte transfers to installed .SYS devices.
fn stream_character(vol: &mut Volume, ss: u16, sp: u16, ah: u16) -> u32 {
    let output = ah == 2 || ah == 9;
    let fd = u16::from(output);
    let psp = super::arena::current();
    let sys = handle(psp, fd);
    if sys >= SLOTS || w(TABLE, slot(sys)) == 0 {
        return failure(6);
    }
    if !output && sys == 0 {
        return u32::from(pc::key() & 255);
    }
    let ds = w(ss, sp + 16);
    let dx = w(ss, sp + 6);
    let scratch = 0x7c;
    let mut count = 1;
    let (seg, at) = if ah == 9 {
        count = 0;
        while b(ds, dx.wrapping_add(count)) != 36 {
            if count == u16::MAX || dx.wrapping_add(count) == u16::MAX {
                return failure(13);
            }
            count += 1;
        }
        (ds, dx)
    } else {
        (psp, scratch)
    };
    let old = b(psp, scratch);
    if ah == 2 {
        pc::store(psp, scratch, dx)
    }
    let bx = w(ss, sp + 2);
    let cx = w(ss, sp + 4);
    let flags = w(ss, sp + 22);
    set(ss, sp, if output { 0x4000 } else { 0x3f00 });
    set(ss, sp + 2, fd);
    set(ss, sp + 4, count);
    set(ss, sp + 6, at);
    set(ss, sp + 16, seg);
    let result = if char_device::dispatch(ss, sp, psp) == 1 {
        if w(ss, sp + 22) & 1 != 0 {
            failure(w(ss, sp))
        } else {
            u32::from(w(ss, sp))
        }
    } else {
        transfer(vol, sys, count, seg, at, output)
    };
    set(ss, sp + 2, bx);
    set(ss, sp + 4, cx);
    set(ss, sp + 6, dx);
    set(ss, sp + 16, ds);
    set(ss, sp + 22, flags);
    let data = b(psp, scratch);
    if ah != 9 {
        pc::store(psp, scratch, old)
    }
    if result >= 0x10000 {
        return result;
    }
    u32::from(if ah == 9 {
        36
    } else if ah == 2 {
        dx & 255
    } else if result == 0 {
        26
    } else {
        data
    })
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
        match char_device::dispatch(ss, sp, super::arena::current()) {
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
    if ah == 0x19 {
        set(ss, sp, b(pc::COMMAND, 256));
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
            2 | 7 | 8 | 9 => stream_character(vol, ss, sp, ah),
            0x2a..=0x2d => clock::service(ss, sp, ah),
            0x2e => {
                pc::store(LIST_SEG, pc::dos_tables::VERIFY, al & 1);
                0
            }
            0x54 => u32::from(b(LIST_SEG, pc::dos_tables::VERIFY)),
            0x33 if al <= 2 => {
                let old = b(LIST_SEG, pc::dos_tables::BREAK);
                if al != 0 {
                    pc::store(LIST_SEG, pc::dos_tables::BREAK, dx & 1)
                }
                if al != 1 {
                    set(ss, sp + 6, (dx & 0xff00) | old)
                }
                0
            }
            0x36 | 0x69 => volume_service(vol, ss, sp, ax),
            0x57 if al <= 1 => {
                if sys < FIRST || sys >= SLOTS || w(TABLE, slot(sys)) == 0 {
                    failure(6)
                } else {
                    let s = slot(sys);
                    let drive = w(TABLE, s + 5);
                    let mut tmp = Volume::EMPTY;
                    match paths::select(vol, &mut tmp, drive) {
                        Err(_) => failure(if al == 0 { 1 } else { 5 }),
                        Ok(v) => {
                            let mut time = cx;
                            let mut date = dx;
                            let err = v.file_time(
                                d(s + 0x1b),
                                b(TABLE, s + 0x1f) << 5,
                                al == 1,
                                &mut time,
                                &mut date,
                            );
                            if err == 0 && al == 0 {
                                set(ss, sp + 4, time);
                                set(ss, sp + 6, date);
                            }
                            dos_result(err)
                        }
                    }
                }
            }
            0x60 => paths::truename(vol, ss, sp),
            0x66 if al == 1 => {
                set(ss, sp + 2, 437);
                set(ss, sp + 6, 437);
                0
            }
            0x66 if al == 2 => {
                if bx == 437 {
                    0
                } else {
                    failure(65)
                }
            }
            0x59 if al == 0 => {
                set(ss, sp + 2, w(seg, sda + 6));
                set(ss, sp + 4, w(seg, sda + 8));
                u32::from(w(seg, sda + 4))
            }
            0x0e => {
                let drive = dx & 255;
                if paths::available(vol, drive) {
                    pc::store(pc::COMMAND, 256, drive)
                }
                u32::from(if pc::cdrom::present() {
                    pc::cdrom::letter() + 1
                } else {
                    3
                })
            }
            0x39 | 0x3a | 0x3b | 0x3c | 0x3d | 0x41 | 0x43 | 0x4e | 0x56 | 0x5b => {
                let r = path_service(vol, ah, al, ss, sp);
                keep_directory = ah == 0x3b && r == 0;
                r
            }
            0x4f => {
                let drive = paths::search_drive();
                let mut tmp = Volume::EMPTY;
                match paths::select(vol, &mut tmp, drive) {
                    Ok(v) => paths::search(v, None, ss, sp),
                    Err(_) => failure(18),
                }
            }
            0x47 => {
                let unit = dx & 255;
                let drive = if unit == 0 {
                    b(pc::COMMAND, 256)
                } else {
                    unit - 1
                };
                if paths::available(vol, drive) {
                    super::environment::cwd(drive, ds, w(ss, sp + 8));
                    0x100
                } else {
                    failure(15)
                }
            }
            0x4b if al == 0 => {
                keep_directory = true;
                super::mz::child(vol, psp, ss, sp)
            }
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

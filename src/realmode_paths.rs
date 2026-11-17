//! DOS drive selection, FAT directories and the 43-byte FindFirst DTA.
use super::*;
use dosrs_realmode::{
    BUFFER, COMMAND,
    disk::{match_mask, unpack_name},
};

const PATH: u16 = 0x320;
const FROM: u16 = 0x3a0;
const TO: u16 = FROM + 11;
pub(super) struct Path {
    pub drive: u16,
    pub seg: u16,
    pub start: u16,
    pub end: u16,
}
impl Path {
    pub fn read(seg: u16, pointer: u16) -> Result<Self, u16> {
        let mut end = pointer;
        while b(seg, end) != 0 {
            if end - pointer >= 127 || end == u16::MAX {
                return Err(3);
            }
            end += 1;
        }
        let (drive, start) = if end - pointer >= 2 && b(seg, pointer + 1) == 58 {
            let c = b(seg, pointer) & 0xdf;
            if !(65..=90).contains(&c) {
                return Err(15);
            }
            (c - 65, pointer + 2)
        } else {
            (b(COMMAND, 256), pointer)
        };
        Ok(Self {
            drive,
            seg,
            start,
            end,
        })
    }
    fn canon(&self, limit: u16) -> Result<u16, u16> {
        super::super::environment::canonical(
            self.drive, self.seg, self.start, self.end, COMMAND, PATH, limit,
        )
        .ok_or(3)
    }
    pub fn fat_name(&self, vol: &mut Volume) -> bool {
        if self.end == self.start || b(self.seg, self.end - 1) == 92 {
            return false;
        }
        let Ok(n) = self.canon(127) else { return false };
        vol.path(COMMAND, PATH, n, false)
    }
}
pub(super) fn truename(main: &Volume, ss: u16, sp: u16) -> u32 {
    let path = match Path::read(w(ss, sp + 16), w(ss, sp + 8)) {
        Ok(p) => p,
        Err(e) => return failure(e),
    };
    if !available(main, path.drive) {
        return failure(15);
    }
    let n = match path.canon(124) {
        Ok(n) => n,
        Err(e) => return failure(e),
    };
    let seg = w(ss, sp + 14);
    let at = w(ss, sp + 10);
    if at > u16::MAX - 2 - n {
        return failure(3);
    }
    set(seg, at, 0x3a41 + path.drive);
    pc::copy_bytes(COMMAND, PATH, seg, at + 2, n + 1);
    0
}
pub(super) fn available(vol: &Volume, drive: u16) -> bool {
    drive < 2 && drive < pc::floppies()
        || drive == 2 && vol.mounted()
        || drive == pc::cdrom::letter() && (pc::cdrom::present() || pc::cdrom::probe())
}
pub(super) fn select<'a>(
    main: &'a mut Volume,
    tmp: &'a mut Volume,
    drive: u16,
) -> Result<&'a mut Volume, u16> {
    if drive == 2 && main.mounted() {
        return Ok(main);
    }
    if drive < 2 && drive < pc::floppies() {
        return if tmp.mount_drive(drive) {
            Ok(tmp)
        } else {
            Err(21)
        };
    }
    Err(15)
}
pub(crate) fn executable(
    main: &mut Volume,
    seg: u16,
    at: u16,
    unit: &mut u16,
    first: &mut u32,
    size: &mut u32,
) -> u16 {
    let path = match Path::read(seg, at) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if path.drive == pc::cdrom::letter() {
        let Ok(n) = path.canon(127) else { return 3 };
        if !pc::cdrom::find(COMMAND, PATH, PATH + n, false) {
            return 2;
        }
        *unit = u16::MAX;
        *first = pc::cdrom::extent();
        *size = pc::cdrom::length();
        return 0;
    }
    let mut removable = Volume::EMPTY;
    let vol = match select(main, &mut removable, path.drive) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let orig = vol.directory();
    let ok = path.fat_name(vol) && vol.describe(first, size);
    vol.restore(orig);
    *unit = if path.drive == 2 {
        pc::hard_unit()
    } else {
        path.drive
    };
    if ok { 0 } else { 2 }
}
pub(super) fn chdir(vol: &mut Volume, path: &Path) -> u32 {
    let Ok(n) = path.canon(64) else {
        return failure(3);
    };
    let ok = if path.drive == pc::cdrom::letter() {
        pc::cdrom::find(COMMAND, PATH, PATH + n, true)
    } else {
        vol.path(COMMAND, PATH, n, true)
    };
    if !ok {
        return failure(3);
    }
    super::super::environment::remember(path.drive, COMMAND, PATH, n);
    0
}
fn active(vol: &mut Volume) -> bool {
    let mut lba = 0;
    let mut at = 0;
    if vol.file_open(false, 0, &mut lba, &mut at) != 0 {
        return false;
    }
    let mut sys = FIRST;
    while sys < SLOTS {
        let s = slot(sys);
        if w(TABLE, s) != 0
            && w(TABLE, s + 5) == vol.drive()
            && d(s + 0x1b) == lba
            && b(TABLE, s + 0x1f) == at >> 5
        {
            return true;
        }
        sys += 1;
    }
    false
}
pub(super) fn directory(vol: &mut Volume, path: &Path, ah: u16, ss: u16, sp: u16) -> u32 {
    let mut orig = vol.directory();
    if ah == 0x3a && path.drive < 2 {
        pc::store(COMMAND, PATH, 92);
        super::super::environment::cwd(path.drive, COMMAND, PATH + 1);
        let n = super::super::environment::path_len(path.drive) + 1;
        if vol.path(COMMAND, PATH, n, true) {
            orig = vol.directory();
        }
    }
    if !path.fat_name(vol) {
        return failure(3);
    }
    let err = match ah {
        0x39 => vol.mkdir(),
        0x3a | 0x41 => {
            if active(vol) {
                5
            } else {
                vol.remove(ah == 0x3a, orig)
            }
        }
        0x56 => {
            if active(vol) {
                return failure(5);
            }
            let from = vol.directory();
            pc::copy_bytes(COMMAND, 128, COMMAND, FROM, 11);
            let dst = match Path::read(w(ss, sp + 14), w(ss, sp + 10)) {
                Ok(p) => p,
                Err(e) => return failure(e),
            };
            if dst.drive != path.drive {
                return failure(17);
            }
            if !dst.fat_name(vol) {
                return failure(3);
            }
            if vol.directory() != from {
                return failure(17);
            }
            pc::copy_bytes(COMMAND, 128, COMMAND, TO, 11);
            pc::copy_bytes(COMMAND, FROM, COMMAND, 128, 11);
            vol.rename(TO)
        }
        _ => 1,
    };
    if err == 0 { 0 } else { failure(err) }
}
// Store both cluster and slot in the user's DTA. Independent searches in
// different processes can then resume after a nested EXEC or a disk read.
pub(super) fn search(vol: &mut Volume, path: Option<&Path>, ss: u16, sp: u16) -> u32 {
    let sda = pc::dos_tables::SDA;
    let seg = w(LIST_SEG, sda + 14);
    let at = w(LIST_SEG, sda + 12);
    if at > 65535 - 43 {
        return failure(18);
    }
    if let Some(path) = path {
        let n = match path.canon(127) {
            Ok(n) => n,
            Err(e) => return failure(e),
        };
        if !vol.pattern(COMMAND, PATH, PATH + n, seg, at + 1) {
            return failure(3);
        }
        pc::store(seg, at, w(ss, sp + 4));
        sd(seg, at + 12, vol.directory());
        set(seg, at + 16, 0);
        set(seg, at + 18, 0);
        pc::store(seg, at + 20, 0xf0 | vol.drive());
    }
    let mut cluster = dw(seg, at + 12);
    let mut pos = w(seg, at + 16);
    let mut visits = w(seg, at + 18);
    let mut lba = 0;
    let mut off = 0;
    let wanted = b(seg, at);
    while visits != 65535 {
        match vol.record(&mut cluster, &mut pos, &mut lba, &mut off) {
            1 => (),
            0 => break,
            _ => return failure(30),
        }
        visits += 1;
        let flags = b(BUFFER, off + 11);
        if b(BUFFER, off) == 229
            || flags == 15
            || (flags & 8 != 0) != (wanted == 8)
            || flags & 0x16 & !wanted != 0
            || !match_mask(seg, at + 1, off)
        {
            continue;
        }
        sd(seg, at + 12, cluster);
        set(seg, at + 16, pos);
        set(seg, at + 18, visits);
        pc::store(seg, at + 21, flags);
        set(seg, at + 22, w(BUFFER, off + 22));
        set(seg, at + 24, w(BUFFER, off + 24));
        sd(seg, at + 26, dw(BUFFER, off + 28));
        unpack_name(BUFFER, off, seg, at + 30);
        return 0;
    }
    pc::store(seg, at + 20, 0);
    failure(18)
}
pub(super) fn search_drive() -> u16 {
    let sda = pc::dos_tables::SDA;
    let at = w(LIST_SEG, sda + 12);
    if at > u16::MAX - 43 {
        return u16::MAX;
    }
    let v = b(w(LIST_SEG, sda + 14), at + 20);
    if v & 0xf0 == 0xf0 { v & 15 } else { u16::MAX }
}

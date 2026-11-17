//! Two-floppy setup reads drive B directly, caches disk 2 in the PSP when it fits,
//! and switches the single drive by release ID when conventional memory is tight.
use dosrs_realmode::{
    byte, copy_bytes,
    system::{File, Media},
};
unsafe extern "Rust" {
    fn edit_heap() -> u16;
}
fn arena() -> u16 {
    // The rust16 DOS runtime reserves 16 KiB above SS for its stack/near data.
    unsafe { edit_heap() }
}
pub const CONFIG_BUFFER: u16 = 0x30c0;
const ID_BUFFER: u16 = 0x31e0;
// The contiguous manifest group also contains the license and notices shared
// by both disks. Older release sets only put EDIT and WELCOME on disk 2.
const FIRST: u16 = 9;
const COUNT: usize = 6;
const EMPTY: File = File {
    first: 0,
    size: 0,
    contiguous: false,
};
pub struct Package {
    pub boot: Media,
    second: Media,
    release: [u8; 16],
    entries: [File; COUNT],
    cached: bool,
    inserted_second: core::cell::Cell<bool>,
}
impl Package {
    pub fn new(boot: Media) -> Self {
        Self {
            boot,
            second: boot,
            release: [0; 16],
            entries: [EMPTY; COUNT],
            cached: false,
            inserted_second: core::cell::Cell::new(false),
        }
    }
    fn entry(&self, index: u16) -> Option<File> {
        if index < FIRST || usize::from(index - FIRST) >= COUNT {
            return None;
        }
        let f = self.entries[usize::from(index - FIRST)];
        if f.size == 0 { None } else { Some(f) }
    }
    // Disk 1 is restored by the initial release-ID check in the setup dialog.
    // While streaming, switch only at the boundary between manifest groups.
    fn select(&self, second: bool) -> bool {
        let media = self.second;
        if self.cached || media.unit != self.boot.unit || self.inserted_second.get() == second {
            return true;
        }
        let part = if second { 2 } else { 1 };
        crate::text(if second {
            "\r\nInsert disk 2 and press Enter. Esc cancels.\r\nInsert disk 2 > "
        } else {
            "\r\nRestore disk 1 and press Enter.\r\nRestore disk 1 > "
        });
        loop {
            let key = crate::screen(15, 0, 0);
            if key == 27 && second {
                return false;
            }
            if key != 13 {
                continue;
            }
            if matching(self.boot.unit, part, &self.release).is_some() {
                self.inserted_second.set(second);
                return true;
            }
            // The normal display font belongs to the release. Emit the error via
            // the BIOS teletype while an unrelated medium is still in the drive.
            let text: &[u8] = if second {
                b"Wrong disk 2 > "
            } else {
                b"Wrong disk 1 > "
            };
            let mut i = 0;
            while i < text.len() {
                dosrs_realmode::video(0x0e00 | u16::from(text[i]), 7, 0, 0);
                crate::screen(13, u16::from(text[i]), 0);
                i += 1;
            }
        }
    }
    pub fn home(&self) -> bool {
        self.select(false)
    }
    pub fn lookup(&self, index: u16) -> Option<File> {
        if let Some(f) = self.entry(index) {
            if !self.select(true) {
                return None;
            }
            return Some(f);
        }
        if !self.home() {
            return None;
        }
        let mut f = self.boot.lookup(crate::install::name(index))?;
        self.boot.check_extent(&mut f);
        Some(f)
    }
    pub fn fetch(&self, index: u16, f: &File, offset: u32, n: u16, seg: u16) -> bool {
        if self.entry(index).is_some() {
            if offset > f.size || u32::from(n) > f.size - offset {
                return false;
            }
            if self.cached {
                let at = f.first + offset;
                copy_bytes(arena() + (at >> 4) as u16, (at & 15) as u16, seg, 0, n);
                return true;
            }
            if !self.select(true) {
                return false;
            }
            return self.second.fetch(f, offset, n, seg, 0);
        }
        self.home() && self.boot.fetch(f, offset, n, seg, 0)
    }
    pub fn stage(&mut self, disk: &Media) -> bool {
        let Some(id) = identity(disk, 2) else {
            return false;
        };
        let mut entries = [EMPTY; COUNT];
        let mut total = 0u32;
        let mut i = 0;
        while i < COUNT {
            if let Some(mut f) = disk.lookup(crate::install::name(FIRST + i as u16)) {
                if f.size == 0 || f.size > 16 * 1024 * 1024 {
                    return false;
                }
                disk.check_extent(&mut f);
                entries[i] = f;
                total += f.size;
            }
            i += 1;
        }
        // Both files have been part of disk 2 since the first release set.
        if entries[0].size == 0 || entries[COUNT - 1].size == 0 {
            return false;
        }
        let end = crate::allocation_end();
        let dest = arena();
        let cache =
            disk.unit == self.boot.unit && dest < end && total <= u32::from(end - dest) * 16;
        if cache {
            i = 0;
            let mut next = 0u32;
            while i < COUNT {
                let f = entries[i];
                let mut pos = 0u32;
                while pos < f.size {
                    let left = f.size - pos;
                    let len = if left < 16384 { left as u16 } else { 16384 };
                    let at = next + pos;
                    if !disk.fetch(&f, pos, len, dest + (at >> 4) as u16, (at & 15) as u16) {
                        return false;
                    }
                    pos += u32::from(len);
                }
                entries[i] = File {
                    first: next,
                    size: f.size,
                    contiguous: true,
                };
                next += f.size;
                i += 1;
            }
        }
        self.entries = entries;
        self.release = id;
        self.second = *disk;
        self.cached = cache;
        self.inserted_second.set(false);
        true
    }
}
pub fn identity(disk: &Media, part: u16) -> Option<[u8; 16]> {
    let f = disk.lookup(if part == 1 {
        b"DISK1   ID "
    } else {
        b"DISK2   ID "
    })?;
    if f.size != 16 || !disk.fetch(&f, 0, 16, ID_BUFFER, 0) {
        return None;
    }
    let mut value = [0; 16];
    let mut i = 0;
    while i < 16 {
        value[i] = byte(ID_BUFFER, i as u16) as u8;
        i += 1;
    }
    Some(value)
}
pub fn matching(unit: u16, part: u16, wanted: &[u8; 16]) -> Option<Media> {
    let disk = Media::device(unit)?;
    let id = identity(&disk, part)?;
    let mut i = 0;
    while i < 16 {
        if id[i] != wanted[i] {
            return None;
        }
        i += 1;
    }
    Some(disk)
}

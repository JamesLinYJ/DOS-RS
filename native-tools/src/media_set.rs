//! Disk 2 stays in the installer's own PSP allocation, after its data and stack.
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
const APP_FILES: [u16; 2] = [14, 9]; // EDIT.EXE, WELCOME.TXT in the installation manifest
pub struct Package {
    pub boot: Media,
    sizes: [u16; 2],
}
impl Package {
    pub fn new(boot: Media) -> Self {
        Self {
            boot,
            sizes: [0; 2],
        }
    }
    fn slot(index: u16) -> Option<usize> {
        match index {
            14 => Some(0),
            9 => Some(1),
            _ => None,
        }
    }
    pub fn lookup(&self, index: u16) -> Option<File> {
        if let Some(i) = Self::slot(index) {
            if self.sizes[i] != 0 {
                return Some(File {
                    first: if i == 0 { 0 } else { self.sizes[0] as u32 },
                    size: self.sizes[i] as u32,
                    contiguous: true,
                });
            }
        }
        let mut f = self.boot.lookup(crate::install::name(index))?;
        self.boot.check_extent(&mut f);
        Some(f)
    }
    pub fn fetch(&self, index: u16, f: &File, offset: u32, n: u16, seg: u16) -> bool {
        if let Some(i) = Self::slot(index) {
            if self.sizes[i] != 0 {
                if offset > f.size || n as u32 > f.size - offset {
                    return false;
                }
                copy_bytes(arena(), (f.first + offset) as u16, seg, 0, n);
                return true;
            }
        }
        self.boot.fetch(f, offset, n, seg, 0)
    }
    pub fn stage(&mut self, disk: &Media) -> bool {
        let mut sizes = [0u16; 2];
        let mut next = 0u32;
        let dest = arena();
        let psp =
            dosrs_realmode::system::w(dosrs_realmode::COMMAND, dosrs_realmode::layout::CURRENT_PSP);
        let end = dosrs_realmode::system::w(psp, 2);
        if dest >= end {
            return false;
        }
        let available = u32::from(end - dest) * 16;
        let mut i = 0;
        while i < 2 {
            let Some(mut f) = disk.lookup(crate::install::name(APP_FILES[i])) else {
                return false;
            };
            if f.size == 0 || f.size > 65520 - next || next + f.size > available {
                return false;
            }
            disk.check_extent(&mut f);
            if !disk.fetch(&f, 0, f.size as u16, dest, next as u16) {
                return false;
            }
            sizes[i] = f.size as u16;
            next += f.size;
            i += 1;
        }
        self.sizes = sizes;
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

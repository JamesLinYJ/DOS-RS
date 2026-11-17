//! DOS naming, metadata and per-drive current directories over mounted volumes.
use super::driver::{self, Name};
#[cfg(not(test))]
pub(crate) use super::fat::with_media;
use driver::HARD;

pub fn in_dir<T>(dir: u32, act: impl FnOnce() -> T) -> T {
    let saved = working_directory();
    set_working(dir);
    let result = act();
    set_working(saved);
    result
}
pub fn has_c() -> bool {
    driver::hard_present()
}
pub fn flush(dir: u32) -> bool {
    driver::at(dir).flush(dir)
}
pub fn root(dir: u32) -> u32 {
    dir & HARD
}
pub fn drive_root(drive: u8) -> Option<u32> {
    match drive {
        0 | 1 => Some(0),
        2 if has_c() => Some(HARD),
        _ => None,
    }
}

/// DOS keeps a different current directory for each drive in each virtual machine.
#[derive(Clone, Copy)]
pub struct Directories {
    active: u8,
    paths: [u32; 3],
}
impl Directories {
    pub const ROOTS: Self = Self {
        active: 0,
        paths: [0, 0, HARD],
    };
}
static DIRECTORIES: crate::kernel_cell::KernelCell<Directories> =
    crate::kernel_cell::KernelCell::new(Directories::ROOTS);

pub fn active_drive() -> u8 {
    DIRECTORIES.borrow().active
}
pub fn directories() -> Directories {
    *DIRECTORIES.borrow()
}
pub fn restore(dirs: Directories) {
    set_working(dirs.paths[usize::from(dirs.active)]);
    *DIRECTORIES.borrow() = dirs;
}
pub fn drive_directory(drive: u8) -> Option<u32> {
    drive_root(drive)?;
    Some(DIRECTORIES.borrow().paths[usize::from(drive)])
}
pub fn select_drive(drive: u8) -> bool {
    let Some(cwd) = drive_directory(drive) else {
        return false;
    };
    DIRECTORIES.borrow().active = drive;
    set_working(cwd);
    true
}

pub fn change_dir(path: &[u8]) -> bool {
    let Some(dir) = resolve_dir(path) else {
        return false;
    };
    let mut dirs = DIRECTORIES.borrow();
    let drive = if path.get(1) == Some(&b':') {
        path[0].to_ascii_uppercase() - b'A'
    } else {
        dirs.active
    };
    dirs.paths[usize::from(drive)] = dir;
    if drive == dirs.active {
        set_working(dir);
    }
    true
}
pub fn resolve_dir(path: &[u8]) -> Option<u32> {
    let (base, path) = path_start(path)?;
    walk(base, path)
}
pub(super) fn walk(mut dir: u32, path: &[u8]) -> Option<u32> {
    for part in path.split(|b| matches!(*b, b'/' | b'\\')) {
        if part.is_empty() || part == b"." {
            continue;
        }
        dir = if part == b".." {
            parent_directory(dir)
        } else {
            directory_cluster(dir, &canonical(part)?)?
        };
    }
    Some(dir)
}
pub fn path_start(path: &[u8]) -> Option<(u32, &[u8])> {
    directories().start(working_directory(), path)
}

/// POSIX-visible absolute names for mounted DOS drives.
pub(super) fn mount_path(path: &[u8]) -> Option<(u8, &[u8])> {
    let tail = path.strip_prefix(b"/dos/")?;
    let (&letter, rest) = tail.split_first()?;
    if !(b'a'..=b'c').contains(&letter) || !(rest.is_empty() || rest.starts_with(b"/")) {
        return None;
    }
    Some((letter - b'a', rest))
}

impl Directories {
    fn start(self, current: u32, path: &[u8]) -> Option<(u32, &[u8])> {
        let (dir, tail) = if path.get(1) == Some(&b':') {
            let drive = path[0].to_ascii_uppercase().checked_sub(b'A')?;
            drive_root(drive)?;
            (self.paths[usize::from(drive)], &path[2..])
        } else {
            (current, path)
        };
        let absolute = tail.first().is_some_and(|b| matches!(*b, b'/' | b'\\'));
        Some((if absolute { root(dir) } else { dir }, tail))
    }

    pub fn locate(self, path: &[u8]) -> Option<(u32, &[u8])> {
        if let Some((drive, rest)) = mount_path(path) {
            return Some((drive_root(drive)?, rest));
        }
        self.start(self.paths[usize::from(self.active)], path)
    }
}

pub fn path_name(path: &[u8]) -> Option<(u32, [u8; 11])> {
    let (base, tail) = path_start(path)?;
    file_in(base, tail)
}
pub fn file_in(base: u32, tail: &[u8]) -> Option<(u32, [u8; 11])> {
    if let Some(i) = tail.iter().rposition(|b| matches!(*b, b'/' | b'\\')) {
        let dir = walk(base, &tail[..i])?;
        Some((dir, canonical(&tail[i + 1..])?))
    } else {
        Some((base, canonical(tail)?))
    }
}

static SELECTED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
pub fn working_directory() -> u32 {
    SELECTED.load(core::sync::atomic::Ordering::Relaxed)
}
fn set_working(dir: u32) {
    SELECTED.store(dir, core::sync::atomic::Ordering::Relaxed)
}

use dosrs_native_posix::{Access, Errno, Open};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    Missing,
    Directory,
    Access,
    Limit,
    Io,
    Space,
}

/// A DOS handle adds 32-bit offsets and packed timestamps to the common open file.
pub struct OpenFile(super::File);
impl OpenFile {
    /// DOS checks the per-file read-only attribute at write time. A protected mount
    /// rejects a write-mode open immediately, including opens from the interrupt gate.
    pub fn dos_mode(dir: u32, name: &[u8; 11], writable: bool) -> Result<Self, FileError> {
        if writable && driver::at(dir).read_only() {
            return Err(FileError::Access);
        }
        Self::open(dir, name, false)
    }
    pub fn open(dir: u32, name: &[u8; 11], writable: bool) -> Result<Self, FileError> {
        let flags = Open {
            access: if writable {
                Access::ReadWrite
            } else {
                Access::Read
            },
            ..Open::READ
        };
        driver::at(dir)
            .open(dir, Name::Dos(name), flags)
            .map(|(file, _)| Self(file))
            .map_err(|e| match e {
                Errno::NoEntry => FileError::Missing,
                Errno::IsDirectory => FileError::Directory,
                Errno::TooManyFiles => FileError::Limit,
                Errno::ReadOnly | Errno::Access => FileError::Access,
                _ => FileError::Io,
            })
    }
    pub fn len(&self) -> usize {
        self.0.len().unwrap_or(0).min(u32::MAX as u64) as usize
    }
    pub fn read(&self, at: usize, out: &mut [u8]) -> dosrs_native_posix::Result<usize> {
        self.0.read(at as u64, out)
    }
    pub fn write(&self, at: usize, data: &[u8]) -> dosrs_native_posix::Result<()> {
        self.0.write(at as u64, data)
    }
    pub fn sync(&self) -> bool {
        self.0.sync().is_ok()
    }
    pub fn identity(&self) -> (u32, u64) {
        self.0.identity()
    }
    pub fn stamp(&self) -> (u16, u16) {
        self.0.stamp().unwrap_or_default()
    }
    pub fn set_stamp(&self, time: u16, date: u16) -> bool {
        self.0.set_stamp(time, date).is_ok()
    }
}
pub fn boot() {
    driver::boot();
    restore(Directories::ROOTS);
}
pub fn directory_cluster(dir: u32, name: &[u8; 11]) -> Option<u32> {
    driver::at(dir).child(dir, name)
}
pub fn parent_directory(dir: u32) -> u32 {
    driver::at(dir).parent(dir)
}
pub fn current_path(out: &mut [u8]) -> usize {
    let dir = working_directory();
    driver::at(dir).path(dir, out)
}
pub fn find_match(dir: u32, template: &[u8; 11], attr: u8, start: u16) -> Option<DirMatch> {
    driver::at(dir).find(dir, template, attr, start)
}
pub fn attribute(name: &[u8; 11]) -> Option<u8> {
    let dir = working_directory();
    driver::at(dir).attr(dir, name)
}
pub fn set_attribute(name: &[u8; 11], attr: u8) -> bool {
    let dir = working_directory();
    driver::at(dir).set_attr(dir, name, attr)
}
pub fn exists_at(dir: u32, name: &[u8; 11]) -> bool {
    driver::at(dir).attr(dir, name).is_some()
}
pub fn file_len(name: &[u8; 11]) -> Option<usize> {
    Some(OpenFile::open(working_directory(), name, false).ok()?.len())
}
pub fn read_range(dir: u32, name: &[u8; 11], at: usize, out: &mut [u8]) -> Option<usize> {
    OpenFile::open(dir, name, false).ok()?.read(at, out).ok()
}
pub fn read_head(name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    read_range(working_directory(), name, 0, out)
}
pub fn read_file(name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    read_file_at(working_directory(), name, out)
}
pub fn read_file_at(dir: u32, name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    let file = OpenFile::open(dir, name, false).ok()?;
    let size = file.len();
    if size > out.len() {
        return None;
    }
    let n = file.read(0, &mut out[..size]).ok()?;
    (n == size).then_some(n)
}
#[cfg(test)]
pub fn write_file(name: &[u8; 11], data: &[u8]) -> bool {
    write_named(name, data, 0x20)
}
#[cfg(test)]
pub fn write_named(name: &[u8; 11], data: &[u8], attr: u8) -> bool {
    write_named_at(working_directory(), name, data, attr)
}
pub fn write_named_at(dir: u32, name: &[u8; 11], data: &[u8], attr: u8) -> bool {
    driver::at(dir).replace(dir, name, data, attr)
}

pub fn remove_file(name: &[u8; 11]) -> bool {
    let dir = working_directory();
    driver::at(dir).unlink(dir, Name::Dos(name)).is_ok()
}
pub fn make_dir(name: &[u8; 11]) -> bool {
    let dir = working_directory();
    driver::at(dir).mkdir(dir, name)
}
pub fn remove_dir(name: &[u8; 11]) -> bool {
    let dir = working_directory();
    driver::at(dir).rmdir(dir, name)
}
pub fn rename_file(from: &[u8; 11], to: &[u8; 11]) -> bool {
    let dir = working_directory();
    driver::at(dir).rename(dir, from, to)
}
pub fn media_id() -> u8 {
    let dir = working_directory();
    driver::at(dir).media(dir)
}
pub fn volume_id() -> Option<([u8; 4], [u8; 11], [u8; 8])> {
    let dir = working_directory();
    driver::at(dir).volume(dir)
}
pub fn set_volume_id(serial: &[u8; 4], label: &[u8; 11]) -> bool {
    let dir = working_directory();
    driver::at(dir).set_volume(dir, serial, label)
}
pub fn drive_block() -> Option<DriveBlock> {
    let dir = working_directory();
    driver::at(dir).parameters(dir)
}
fn space_at(dir: u32) -> Option<(u32, u32, u32, u32)> {
    driver::at(dir).space(dir)
}
#[cfg(test)]
pub fn space() -> Option<(u32, u32, u32, u32)> {
    space_at(working_directory())
}
pub fn allocation() -> Option<(u16, u16, u16, u16)> {
    allocation_at(working_directory())
}
pub fn allocation_at(dir: u32) -> Option<(u16, u16, u16, u16)> {
    let (mut spc, bps, mut free, mut total) = space_at(dir)?;
    // Group allocation units for the classic 16-bit free-space interface.
    while total > 0xfffe && spc < 128 {
        total /= 2;
        free /= 2;
        spc *= 2;
    }
    Some((
        spc as u16,
        bps as u16,
        free.min(0xfffe) as u16,
        total.min(0xfffe) as u16,
    ))
}

pub struct DirMatch {
    pub name: [u8; 11],
    pub attr: u8,
    pub time: u16,
    pub date: u16,
    pub size: u32,
    pub index: u16,
}

pub struct DriveBlock {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved: u16,
    pub fats: u8,
    pub root_entries: u16,
    pub sectors_per_fat: u16,
    pub media: u8,
    pub data_sector: u16,
    pub max_cluster: u16,
    pub shift: u8,
}

pub(super) fn attribute_visible(file_attr: u8, search_attr: u8) -> bool {
    const SPECIAL: u8 = 0x02 | 0x04 | 0x08 | 0x10;
    file_attr & SPECIAL & !search_attr == 0
}

pub(super) fn names_match(template: &[u8; 11], name: &[u8; 11]) -> bool {
    let mut index = 0usize;
    while index < 11 {
        if template[index] != b'?' && template[index] != name[index] {
            return false;
        }
        index += 1;
    }
    true
}

pub fn canonical(text: &[u8]) -> Option<[u8; 11]> {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut seen = false;
    for byte in text {
        if *byte == 0 || *byte == b' ' {
            break;
        }
        seen = true;
        if *byte == b'.' && !extension {
            extension = true;
            continue;
        }
        let upper = if byte.is_ascii_lowercase() {
            byte.wrapping_sub(b'a').wrapping_add(b'A')
        } else {
            *byte
        };
        if !extension && base < 8 {
            name[base] = upper;
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = upper;
            ext += 1;
        }
    }
    if seen { Some(name) } else { None }
}

pub fn short_name(name: &[u8; 11]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(12);
    out.extend(name[..8].iter().copied().take_while(|&b| b != b' '));
    if name[8] != b' ' {
        out.push(b'.');
        out.extend(name[8..].iter().copied().take_while(|&b| b != b' '));
    }
    out
}

pub(crate) fn search_path(path: &[u8]) -> Option<(u32, [u8; 11])> {
    let (base, tail) = path_start(path)?;
    let (dir, name) = match tail.iter().rposition(|b| matches!(*b, b'/' | b'\\')) {
        Some(i) => (walk(base, &tail[..i])?, &tail[i + 1..]),
        None => (base, tail),
    };
    if name.is_empty() {
        return None;
    }
    Some((dir, component_template(name)))
}
fn component_template(text: &[u8]) -> [u8; 11] {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut fill = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        let upper = text[index].to_ascii_uppercase();
        if upper == b'.' && !extension {
            if fill {
                while base < 8 {
                    name[base] = b'?';
                    base += 1;
                }
            }
            extension = true;
            fill = false;
            index += 1;
            continue;
        }
        if upper == b'*' {
            fill = true;
            index += 1;
            continue;
        }
        if !extension && base < 8 {
            name[base] = if fill { b'?' } else { upper };
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = if fill { b'?' } else { upper };
            ext += 1;
        }
        index += 1;
    }
    if fill {
        if extension {
            while ext < 3 {
                name[8 + ext] = b'?';
                ext += 1;
            }
        } else {
            while base < 8 {
                name[base] = b'?';
                base += 1;
            }
        }
    }
    name
}

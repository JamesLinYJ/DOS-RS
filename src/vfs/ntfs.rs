//! NTFS mounts expose the read side of the block device to the filesystem.
use super::{
    dos::{self, DirMatch, DriveBlock},
    driver::{HARD, Name, Volume, VolumeId},
    folders::Folders,
};
use crate::kernel_cell::KernelCell;
use alloc::boxed::Box;
use dosrs_block::BlockWindow;
use dosrs_native_posix::{Errno, Open, Result};
use dosrs_ntfs::{Entry, Filesystem, ROOT};

struct Mount {
    fs: Filesystem<BlockWindow>,
    dirs: Folders,
    space: Option<(u32, u32, u32, u32)>,
}
static MOUNT: KernelCell<Option<Mount>> = KernelCell::new(None);
fn error(e: dosrs_ntfs::Error) -> Errno {
    match e {
        dosrs_ntfs::Error::Missing => Errno::NoEntry,
        dosrs_ntfs::Error::Unsupported => Errno::NotSupported,
        dosrs_ntfs::Error::Directory => Errno::IsDirectory,
        _ => Errno::Io,
    }
}
pub(super) fn mount(device: BlockWindow) -> bool {
    let Ok(fs) = Filesystem::mount(device.read_only()) else {
        return false;
    };
    *MOUNT.borrow() = Some(Mount {
        fs,
        dirs: Folders::new(ROOT),
        space: None,
    });
    crate::puts("Mounted NTFS C:\\ (read-only).\n");
    true
}
fn mounted<T>(act: impl FnOnce(&mut Mount) -> Result<T>) -> Result<T> {
    act(MOUNT.borrow().as_mut().ok_or(Errno::NoEntry)?)
}
impl Mount {
    fn lookup(&self, dir: u32, name: &[u8]) -> Result<Entry> {
        self.fs
            .lookup(
                self.dirs.get(dir)?.id,
                core::str::from_utf8(name).map_err(|_| Errno::Invalid)?,
            )
            .map_err(error)
    }
    fn child(&mut self, dir: u32, name: &[u8]) -> Result<u32> {
        if name == b"." || name.is_empty() {
            return Ok(dir);
        }
        if name == b".." {
            return Ok(self.dirs.get(dir)?.parent);
        }
        let entry = self.lookup(dir, name)?;
        if entry.reparse() {
            return Err(Errno::NotSupported);
        }
        if !entry.directory() {
            return Err(Errno::NotDirectory);
        }
        self.dirs.remember(entry.id, dir, entry.name.into_bytes())
    }
    fn open(&mut self, mut dir: u32, path: &[u8]) -> Result<super::File> {
        let mut parts = path.split(|b| matches!(b, b'/' | b'\\')).peekable();
        while let Some(name) = parts.next() {
            if parts.peek().is_some() {
                dir = self.child(dir, name)?;
                continue;
            }
            if name.is_empty() || name == b"." || name == b".." {
                return Err(Errno::IsDirectory);
            }
            let entry = self.lookup(dir, name)?;
            if entry.reparse() {
                return Err(Errno::NotSupported);
            }
            return Ok(Box::new(File(self.fs.open(entry.id).map_err(error)?)));
        }
        Err(Errno::NoEntry)
    }
}
fn attrs(e: &Entry) -> u8 {
    (e.attrs as u8 & 0x27) | if e.directory() { 0x10 } else { 0 }
}
struct File(dosrs_ntfs::File);
impl super::Node for File {
    fn len(&self) -> Result<u64> {
        Ok(self.0.len())
    }
    fn read(&self, pos: u64, out: &mut [u8]) -> Result<usize> {
        mounted(|m| m.fs.read(&self.0, pos, out).map_err(error))
    }
    fn write(&self, _: u64, _: &[u8]) -> Result<()> {
        Err(Errno::ReadOnly)
    }
    fn sync(&self) -> Result<()> {
        Ok(())
    }
    fn identity(&self) -> (u32, u64) {
        (HARD, self.0.id)
    }
    fn stamp(&self) -> Result<(u16, u16)> {
        Ok(super::time::pack(self.0.modified))
    }
    fn set_stamp(&self, _: u16, _: u16) -> Result<()> {
        Err(Errno::ReadOnly)
    }
}
pub(super) struct Driver;
impl Volume for Driver {
    fn read_only(&self) -> bool {
        true
    }

    fn open(&self, dir: u32, name: Name, flags: Open) -> Result<(super::File, bool)> {
        if flags.access.writable() || flags.create || flags.truncate {
            return Err(Errno::ReadOnly);
        }
        name.with_bytes(|path, _| mounted(|m| Ok((m.open(dir, path)?, false))))
    }
    fn child(&self, dir: u32, name: &[u8; 11]) -> Option<u32> {
        mounted(|m| m.child(dir, &dos::short_name(name))).ok()
    }
    fn parent(&self, dir: u32) -> u32 {
        mounted(|m| Ok(m.dirs.get(dir)?.parent)).unwrap_or(HARD)
    }
    fn path(&self, dir: u32, out: &mut [u8]) -> usize {
        mounted(|m| m.dirs.path(dir, out)).unwrap_or(0)
    }
    fn find(&self, dir: u32, pattern: &[u8; 11], attr: u8, start: u16) -> Option<DirMatch> {
        mounted(|m| {
            let mut next = 0;
            let mut result = None;
            let parent = m.dirs.get(dir)?.id;
            m.fs.visit(parent, |e| {
                let Some(name) = dos::canonical(e.name.as_bytes()) else {
                    return false;
                };
                if !dos::short_name(&name).eq_ignore_ascii_case(e.name.as_bytes())
                    || !dos::names_match(pattern, &name)
                    || !dos::attribute_visible(attrs(e), attr)
                {
                    return false;
                }
                if next < start {
                    next += 1;
                    return false;
                }
                let (time, date) = super::time::pack(e.modified);
                result = Some(DirMatch {
                    name,
                    attr: attrs(e),
                    time,
                    date,
                    size: e.size.min(u64::from(u32::MAX)) as u32,
                    index: next,
                });
                true
            })
            .map_err(error)?;
            result.ok_or(Errno::NoEntry)
        })
        .ok()
    }
    fn attr(&self, dir: u32, name: &[u8; 11]) -> Option<u8> {
        mounted(|m| Ok(attrs(&m.lookup(dir, &dos::short_name(name))?))).ok()
    }
    fn media(&self, _: u32) -> u8 {
        0xf8
    }
    fn volume(&self, _: u32) -> Option<VolumeId> {
        mounted(|m| {
            let mut label = [b' '; 11];
            for (dest, src) in label.iter_mut().zip(m.fs.label.bytes()) {
                *dest = if src.is_ascii() { src } else { b'?' }
            }
            Ok(((m.fs.serial as u32).to_le_bytes(), label, *b"NTFS    "))
        })
        .ok()
    }
    fn parameters(&self, _: u32) -> Option<DriveBlock> {
        None
    }
    fn space(&self, _: u32) -> Option<(u32, u32, u32, u32)> {
        mounted(|m| {
            if let Some(s) = m.space {
                return Ok(s);
            }
            let s = m.fs.space().map_err(error)?;
            m.space = Some(s);
            Ok(s)
        })
        .ok()
    }
    fn flush(&self, _: u32) -> bool {
        true
    }
}

//! Single-device Btrfs volume, shared by native descriptor tables and DOS sessions.
#![forbid(unsafe_code)]
use super::dos::short_name;
use super::driver::HARD;
use crate::kernel_cell::KernelCell;
#[cfg(not(test))]
use alloc::sync::Arc;
use alloc::{collections::BTreeMap, vec::Vec};
#[cfg(not(test))]
use dosrs_block::BlockWindow;
#[cfg(not(test))]
use dosrs_block::{BlockDevice, BlockRead};
use dosrs_btrfs::{Error, Filesystem, inode::Inode};
use dosrs_native_posix::{Errno, Open, Result};

#[path = "btrfs/posix.rs"]
pub(super) mod posix;

static VOLUME: KernelCell<Option<Mount>> = KernelCell::new(None);
use super::folders::Folders;
struct Mount {
    fs: Filesystem,
    dirs: Folders,
    opens: BTreeMap<u64, usize>,
    orphans: alloc::collections::BTreeSet<u64>,
}
#[cfg(not(test))]
pub fn mount(device: BlockWindow) -> bool {
    let dev = Arc::new(device);
    let mut magic = [0; 8];
    if dev.read_at(65536 + 64, &mut magic).is_err() || &magic != b"_BHRfS_M" {
        return false;
    }
    dosrs_btrfs::set_clock(crate::bios::rtc::now);
    let result = if dev.is_writable() {
        Filesystem::mount_rw(dev.clone())
            .and_then(|mut fs| {
                fs.recover_open_files()?;
                Ok(fs)
            })
            .or_else(|_| Filesystem::mount(dev))
    } else {
        Filesystem::mount(dev)
    };
    let Ok(fs) = result else { return false };
    let Ok(root) = fs.root_inode() else {
        return false;
    };
    let writable = fs.is_writable();
    *VOLUME.borrow() = Some(Mount {
        fs,
        dirs: Folders::new(root.ino),
        opens: BTreeMap::new(),
        orphans: Default::default(),
    });
    crate::puts(if writable {
        "Mounted Btrfs / (read/write).\n"
    } else {
        "Mounted Btrfs / (read-only).\n"
    });
    true
}
pub(super) fn is_mounted() -> bool {
    VOLUME.borrow().is_some()
}
#[cfg(not(test))]
pub(super) fn is_writable() -> bool {
    mounted(|m| Ok(m.fs.is_writable())).unwrap_or(false)
}
fn err(e: Error) -> Errno {
    match e {
        Error::NotFound => Errno::NoEntry,
        Error::NotADirectory => Errno::NotDirectory,
        Error::NotAFile => Errno::IsDirectory,
        Error::ReadOnly => Errno::ReadOnly,
        Error::UnsupportedFeature(_) | Error::UnsupportedProfile(_) => Errno::NotSupported,
        _ => Errno::Io,
    }
}
fn mounted<T>(action: impl FnOnce(&mut Mount) -> Result<T>) -> Result<T> {
    let mut guard = VOLUME.borrow();
    action(guard.as_mut().ok_or(Errno::NoEntry)?)
}
impl Mount {
    fn by_name(&self, parent: u64, name: &[u8], dos: bool) -> Result<(Inode, Vec<u8>)> {
        match self.fs.lookup(parent, name) {
            Ok(node) => Ok((node, name.to_vec())),
            Err(Error::NotFound) if dos => {
                let entries = self.fs.read_dir(parent).map_err(err)?;
                let mut matches = entries
                    .iter()
                    .filter(|d| d.is_inode() && d.name.eq_ignore_ascii_case(name));
                let entry = matches.next().ok_or(Errno::NoEntry)?;
                if matches.next().is_some() {
                    return Err(Errno::Invalid);
                }
                Ok((
                    self.fs.read_inode(entry.ino).map_err(err)?,
                    entry.name.clone(),
                ))
            }
            Err(e) => Err(err(e)),
        }
    }
    fn child(&mut self, parent: u32, name: &[u8], dos: bool) -> Result<u32> {
        let dir = self.dirs.get(parent)?;
        if name == b"." || name.is_empty() {
            return Ok(parent);
        }
        if name == b".." {
            return Ok(dir.parent);
        }
        let (node, name) = self.by_name(dir.id, name, dos)?;
        if !node.is_dir() {
            return Err(Errno::NotDirectory);
        }
        self.dirs.remember(node.ino, parent, name)
    }
    fn parent_leaf<'p>(
        &mut self,
        mut dir: u32,
        path: &'p [u8],
        dos: bool,
    ) -> Result<(u64, &'p [u8])> {
        let mut parts = path.split(|b| matches!(b, b'/' | b'\\')).peekable();
        while let Some(name) = parts.next() {
            if parts.peek().is_none() {
                if name.is_empty() || name == b"." || name == b".." {
                    return Err(Errno::IsDirectory);
                }
                if name.len() > 255 {
                    return Err(Errno::NameTooLong);
                }
                if name.contains(&0) {
                    return Err(Errno::Invalid);
                }
                return Ok((self.dirs.get(dir)?.id, name));
            }
            dir = self.child(dir, name, dos)?;
        }
        Err(Errno::NoEntry)
    }
}
pub struct File(u64, bool); // DOS attributes apply to legacy writes, POSIX permissions are checked at open.
impl File {
    pub fn open(dir: u32, path: &[u8], flags: Open, dos: bool) -> Result<Self> {
        mounted(|m| {
            let (parent, name) = m.parent_leaf(dir, path, dos)?;
            if (flags.access.writable() || flags.create) && !m.fs.is_writable() {
                return Err(Errno::ReadOnly);
            }
            let ino = match m.by_name(parent, name, dos) {
                Ok(_) if flags.create && flags.exclusive => return Err(Errno::Exists),
                Ok((n, _)) if n.is_dir() => return Err(Errno::IsDirectory),
                Ok((n, _)) if !n.is_regular_file() => return Err(Errno::NotSupported),
                Ok((n, _)) => {
                    if flags.access.writable() && n.mode & 0o222 == 0 {
                        return Err(Errno::Access);
                    }
                    if flags.truncate {
                        m.fs.truncate(n.ino, 0).map_err(err)?
                    }
                    n.ino
                }
                Err(Errno::NoEntry) if flags.create => {
                    m.fs.create(parent, name, 0o644, 0, 0).map_err(err)?
                }
                Err(e) => return Err(e),
            };
            *m.opens.entry(ino).or_default() += 1;
            Ok(Self(ino, true))
        })
    }
    pub fn read(&self, pos: u64, out: &mut [u8]) -> Result<usize> {
        mounted(|m| m.fs.read_at(self.0, pos, out).map_err(err))
    }
    pub fn write(&self, pos: u64, data: &[u8]) -> Result<()> {
        mounted(|m| {
            if self.1 && m.fs.read_inode(self.0).map_err(err)?.mode & 0o222 == 0 {
                return Err(Errno::Access);
            }
            if data.is_empty() {
                return m.fs.truncate(self.0, pos).map_err(err);
            }
            let n = m.fs.write(self.0, pos, data).map_err(err)?;
            if n != data.len() {
                return Err(Errno::Io);
            }
            Ok(())
        })
    }
    pub fn size(&self) -> Result<u64> {
        mounted(|m| Ok(m.fs.read_inode(self.0).map_err(err)?.size))
    }
}
impl Drop for File {
    fn drop(&mut self) {
        let _ = mounted(|m| {
            let count = m.opens.get_mut(&self.0).ok_or(Errno::BadFd)?;
            *count -= 1;
            if *count == 0 {
                m.opens.remove(&self.0);
                if m.orphans.remove(&self.0) {
                    m.fs.reap_file(self.0).map_err(err)?;
                }
            }
            Ok(())
        });
    }
}
pub fn unlink(dir: u32, path: &[u8], dos: bool) -> Result<()> {
    mounted(|m| {
        let (parent, name) = m.parent_leaf(dir, path, dos)?;
        let (n, name) = m.by_name(parent, name, dos)?;
        if m.opens.contains_key(&n.ino) && n.nlink == 1 {
            m.fs.unlink_open(parent, &name).map_err(err)?;
            m.orphans.insert(n.ino);
            return Ok(());
        }
        m.fs.unlink(parent, &name).map_err(err)
    })
}
pub fn child(dir: u32, name: &[u8]) -> Option<u32> {
    mounted(|m| m.child(dir, name, true)).ok()
}
pub fn parent(dir: u32) -> u32 {
    mounted(|m| Ok(m.dirs.get(dir)?.parent)).unwrap_or(HARD)
}
pub fn current_path(dir: u32, out: &mut [u8]) -> usize {
    mounted(|m| m.dirs.path(dir, out)).unwrap_or(0)
}

pub fn info(dir: u32, name: &[u8; 11]) -> Option<Inode> {
    mounted(|m| {
        let id = m.dirs.get(dir)?.id;
        Ok(m.by_name(id, &short_name(name), true)?.0)
    })
    .ok()
}
pub fn dir_match(
    dir: u32,
    template: &[u8; 11],
    search_attr: u8,
    start: u16,
) -> Option<super::dos::DirMatch> {
    mounted(|m| {
        let id = m.dirs.get(dir)?.id;
        for (i, e) in
            m.fs.read_dir(id)
                .map_err(err)?
                .iter()
                .take(u16::MAX as usize)
                .enumerate()
                .skip(start as usize)
        {
            if !e.is_inode() {
                continue;
            }
            let Some(name) = super::dos::canonical(&e.name) else {
                continue;
            };
            // Only expose round-trippable 8.3 names through the DOS directory ABI.
            if !short_name(&name).eq_ignore_ascii_case(&e.name)
                || !super::dos::names_match(template, &name)
            {
                continue;
            }
            let inode = m.fs.read_inode(e.ino).map_err(err)?;
            let attr = attrs(&inode);
            if !super::dos::attribute_visible(attr, search_attr) {
                continue;
            }
            return Ok(super::dos::DirMatch {
                name,
                attr,
                time: super::time::pack(inode.mtime.sec).0,
                date: super::time::pack(inode.mtime.sec).1,
                size: inode.size.min(u32::MAX as u64) as u32,
                index: i as u16,
            });
        }
        Err(Errno::NoEntry)
    })
    .ok()
}
fn attrs(node: &Inode) -> u8 {
    (if node.is_dir() { 0x10 } else { 0x20 }) | u8::from(node.mode & 0o222 == 0)
}
pub fn attributes(dir: u32, name: &[u8; 11]) -> Option<u8> {
    Some(attrs(&info(dir, name)?))
}
pub fn set_attributes(dir: u32, name: &[u8; 11], attr: u8) -> bool {
    mounted(|m| {
        let (node, _) = m.by_name(m.dirs.get(dir)?.id, &short_name(name), true)?;
        if !node.is_regular_file() || attr & !0x21 != 0 {
            return Err(Errno::NotSupported);
        }
        let mode = if attr & 1 != 0 {
            node.mode & !0o222
        } else {
            node.mode | 0o200
        };
        m.fs.set_mode(node.ino, mode).map_err(err)
    })
    .is_ok()
}
pub fn mkdir(dir: u32, name: &[u8; 11]) -> bool {
    mounted(|m| {
        let parent = m.dirs.get(dir)?.id;
        let name = short_name(name);
        match m.by_name(parent, &name, true) {
            Err(Errno::NoEntry) => {}
            Err(e) => return Err(e),
            Ok(_) => return Err(Errno::Exists),
        }
        m.fs.mkdir(parent, &name, 0o755, 0, 0).map_err(err)?;
        Ok(())
    })
    .is_ok()
}
pub fn rmdir(dir: u32, name: &[u8; 11]) -> bool {
    mounted(|m| {
        let parent = m.dirs.get(dir)?.id;
        let (node, name) = m.by_name(parent, &short_name(name), true)?;
        m.fs.rmdir(parent, &name).map_err(err)?;
        m.dirs.remove(node.ino);
        Ok(())
    })
    .is_ok()
}
pub fn rename(dir: u32, from: &[u8; 11], to: &[u8; 11]) -> bool {
    mounted(|m| {
        let parent = m.dirs.get(dir)?.id;
        let (node, old) = m.by_name(parent, &short_name(from), true)?;
        let new = short_name(to);
        match m.by_name(parent, &new, true) {
            Err(Errno::NoEntry) => {}
            Err(e) => return Err(e),
            Ok(_) => return Err(Errno::Exists),
        }
        m.fs.rename(parent, &old, parent, &new).map_err(err)?;
        m.dirs.rename(node.ino, new);
        Ok(())
    })
    .is_ok()
}
pub fn replace(dir: u32, name: &[u8; 11], data: &[u8], attr: u8) -> bool {
    if attr & !0x21 != 0 {
        return false;
    }
    let result = (|| {
        let f = File::open(
            dir,
            &short_name(name),
            Open {
                truncate: false,
                ..Open::OUTPUT
            },
            true,
        )?;
        let old = f.size()?;
        f.write(0, data)?;
        if !data.is_empty() && old > data.len() as u64 {
            f.write(data.len() as u64, &[])?;
        }
        if attr & 1 != 0 && !set_attributes(dir, name, attr) {
            return Err(Errno::Access);
        }
        Ok(())
    })();
    result.is_ok()
}
pub fn volume_id() -> Option<([u8; 4], [u8; 11], [u8; 8])> {
    mounted(|m| {
        let sb = m.fs.superblock();
        let mut label = [b' '; 11];
        let bytes = sb.label.as_bytes();
        let len = bytes.len().min(11);
        label[..len].copy_from_slice(&bytes[..len]);
        Ok((sb.fsid[..4].try_into().unwrap(), label, *b"BTRFS   "))
    })
    .ok()
}
pub fn space() -> Option<(u32, u32, u32, u32)> {
    mounted(|m| {
        let sb = m.fs.superblock();
        let unit = u64::from(sb.sectorsize);
        Ok((
            1,
            sb.sectorsize,
            ((sb.total_bytes.saturating_sub(sb.bytes_used)) / unit).min(u32::MAX as u64) as u32,
            (sb.total_bytes / unit).min(u32::MAX as u64) as u32,
        ))
    })
    .ok()
}
impl File {
    pub fn ino(&self) -> u64 {
        self.0
    }
    pub fn stamp(&self) -> Result<(u16, u16)> {
        mounted(|m| {
            Ok(super::time::pack(
                m.fs.read_inode(self.0).map_err(err)?.mtime.sec,
            ))
        })
    }
    pub fn set_stamp(&self, time: u16, date: u16) -> Result<()> {
        let secs = super::time::unpack(time, date).ok_or(Errno::Invalid)?;
        mounted(|m| m.fs.set_times(self.0, None, Some((secs, 0))).map_err(err))
    }
}

use super::driver::{Volume, VolumeId};
use super::{
    dos::{DirMatch, DriveBlock},
    driver::Name,
};
pub(super) struct Driver;
impl Volume for Driver {
    fn read_only(&self) -> bool {
        mounted(|m| Ok(!m.fs.is_writable())).unwrap_or(true)
    }

    fn open(
        &self,
        dir: u32,
        path: Name,
        flags: Open,
    ) -> dosrs_native_posix::Result<(super::File, bool)> {
        path.with_bytes(|p, dos| File::open(dir, p, flags, dos))
            .map(|f| (alloc::boxed::Box::new(f) as super::File, false))
    }
    fn unlink(&self, dir: u32, path: Name) -> dosrs_native_posix::Result<()> {
        path.with_bytes(|p, dos| unlink(dir, p, dos))
    }
    fn child(&self, dir: u32, name: &[u8; 11]) -> Option<u32> {
        child(dir, &short_name(name))
    }
    fn parent(&self, dir: u32) -> u32 {
        parent(dir)
    }
    fn path(&self, dir: u32, out: &mut [u8]) -> usize {
        current_path(dir, out)
    }
    fn find(&self, dir: u32, pattern: &[u8; 11], attr: u8, start: u16) -> Option<DirMatch> {
        dir_match(dir, pattern, attr, start)
    }
    fn attr(&self, dir: u32, name: &[u8; 11]) -> Option<u8> {
        attributes(dir, name)
    }
    fn set_attr(&self, dir: u32, name: &[u8; 11], attr: u8) -> bool {
        set_attributes(dir, name, attr)
    }
    fn replace(&self, dir: u32, name: &[u8; 11], data: &[u8], attr: u8) -> bool {
        replace(dir, name, data, attr)
    }

    fn mkdir(&self, dir: u32, name: &[u8; 11]) -> bool {
        mkdir(dir, name)
    }
    fn rmdir(&self, dir: u32, name: &[u8; 11]) -> bool {
        rmdir(dir, name)
    }
    fn rename(&self, dir: u32, from: &[u8; 11], to: &[u8; 11]) -> bool {
        rename(dir, from, to)
    }
    fn media(&self, _dir: u32) -> u8 {
        0xf8
    }
    fn volume(&self, _dir: u32) -> Option<VolumeId> {
        volume_id()
    }
    fn set_volume(&self, _dir: u32, _serial: &[u8; 4], _label: &[u8; 11]) -> bool {
        false
    }
    fn parameters(&self, _dir: u32) -> Option<DriveBlock> {
        None
    }
    fn space(&self, _dir: u32) -> Option<(u32, u32, u32, u32)> {
        space()
    }
    fn flush(&self, _dir: u32) -> bool {
        true
    }
}

impl super::Node for File {
    fn len(&self) -> Result<u64> {
        self.size()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> Result<usize> {
        self.read(offset, out)
    }
    fn write(&self, offset: u64, data: &[u8]) -> Result<()> {
        self.write(offset, data)
    }
    fn sync(&self) -> Result<()> {
        Ok(())
    } // Every mutating operation commits and flushes.
    fn metadata(&self) -> Result<super::posix::Metadata> {
        mounted(|m| Ok(posix::metadata(&m.fs.read_inode(self.0).map_err(err)?)))
    }
    fn dirent(&self, offset: u64) -> Result<Option<super::posix::DirEntry>> {
        posix::dirent(self.0, offset)
    }
    fn identity(&self) -> (u32, u64) {
        (HARD, self.ino())
    }
    fn stamp(&self) -> Result<(u16, u16)> {
        self.stamp()
    }
    fn set_stamp(&self, time: u16, date: u16) -> Result<()> {
        self.set_stamp(time, date)
    }
}

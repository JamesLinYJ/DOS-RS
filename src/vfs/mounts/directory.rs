//! Directory nodes published by the /dos mount namespace.
use crate::vfs::{
    self, Node, dos, driver,
    posix::{DirEntry, Metadata, PosixDir, ROOT, ROOT_INODE},
};
use alloc::{borrow::Cow, vec::Vec};
use dosrs_native_posix::{Errno, Result};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MountDir {
    Hub,
    Drive {
        letter: u8,
        cluster: u32,
        at_root: bool,
    },
}

impl MountDir {
    pub(crate) fn path(self) -> Vec<u8> {
        match self {
            Self::Hub => b"/dos".to_vec(),
            Self::Drive {
                letter,
                cluster,
                at_root,
            } => {
                let mut prefix = if at_root {
                    Vec::new()
                } else {
                    let mut name = b"/dos/a".to_vec();
                    name[5] += letter;
                    name
                };
                let mut buf = [0; 256];
                let len = driver::at(cluster).path(cluster, &mut buf);
                if len > 0 {
                    prefix.push(b'/');
                    prefix.extend(
                        buf[..len]
                            .iter()
                            .map(|b| if *b == b'\\' { b'/' } else { *b }),
                    );
                }
                if prefix.is_empty() {
                    prefix.push(b'/');
                }
                prefix
            }
        }
    }
    pub(crate) fn mount_point(self) -> bool {
        match self {
            Self::Hub => true,
            Self::Drive {
                letter, cluster, ..
            } => dos::drive_root(letter) == Some(cluster),
        }
    }
    pub(crate) fn from_path(path: &[u8]) -> Option<Self> {
        let trimmed = path.strip_suffix(b"/").unwrap_or(path);
        if trimmed == b"/dos" {
            return Some(Self::Hub);
        }
        let (letter, rest) = dos::mount_path(path)?;
        let cluster = dos::walk(dos::drive_root(letter)?, rest)?;
        Some(Self::Drive {
            letter,
            cluster,
            at_root: false,
        })
    }
    pub(crate) fn on_dos_root(dirs: dos::Directories, path: &[u8]) -> Option<Self> {
        if path.is_empty() {
            return None;
        }
        let (base, rest) = dirs.locate(path)?;
        let letter = if path.get(1) == Some(&b':') {
            path[0].to_ascii_uppercase().checked_sub(b'A')?
        } else {
            dirs.active()
        };
        Some(Self::Drive {
            letter,
            cluster: dos::walk(base, rest)?,
            at_root: letter == dirs.active(),
        })
    }
    fn dir_item(self, offset: u64) -> Option<DirEntry> {
        let (ino, name, kind) = if offset < 2 {
            let parent = match self {
                Self::Hub => ROOT_INODE,
                Self::Drive {
                    letter,
                    cluster,
                    at_root,
                } if dos::drive_root(letter) == Some(cluster) => {
                    if at_root {
                        self.identity().1
                    } else {
                        Self::Hub.identity().1
                    }
                }
                Self::Drive {
                    letter,
                    cluster,
                    at_root,
                } => {
                    Self::Drive {
                        at_root,
                        letter,
                        cluster: driver::at(cluster).parent(cluster),
                    }
                    .identity()
                    .1
                }
            };
            (
                if offset == 0 {
                    self.identity().1
                } else {
                    parent
                },
                if offset == 0 {
                    b".".to_vec()
                } else {
                    b"..".to_vec()
                },
                4,
            )
        } else {
            match self {
                Self::Hub => {
                    let (letter, cluster) = (0..=2)
                        .filter_map(|n| dos::drive_root(n).map(|d| (n, d)))
                        .nth(usize::try_from(offset - 2).ok()?)?;
                    (
                        Self::Drive {
                            letter,
                            cluster,
                            at_root: false,
                        }
                        .identity()
                        .1,
                        alloc::vec![b'a' + letter],
                        4,
                    )
                }
                Self::Drive { cluster, .. } => {
                    let mut cursor = 0;
                    let mut at = 2;
                    loop {
                        let entry = driver::at(cluster).find(cluster, &[b'?'; 11], 0x37, cursor)?;
                        cursor = entry.index.checked_add(1)?;
                        let name = dos::short_name(&entry.name);
                        if name == b"." || name == b".." || entry.attr & 8 != 0 {
                            continue;
                        }
                        if at == offset {
                            break (
                                u64::from(entry.index) + 3,
                                name,
                                if entry.attr & 16 == 0 { 8 } else { 4 },
                            );
                        }
                        at += 1;
                    }
                }
            }
        };
        Some(DirEntry { ino, name, kind })
    }
}

impl Node for MountDir {
    fn len(&self) -> Result<u64> {
        Ok(0)
    }
    fn read(&self, _: u64, _: &mut [u8]) -> Result<usize> {
        Err(Errno::IsDirectory)
    }
    fn write(&self, _: u64, _: &[u8]) -> Result<()> {
        Err(Errno::IsDirectory)
    }
    fn sync(&self) -> Result<()> {
        Ok(())
    }
    fn identity(&self) -> (u32, u64) {
        match *self {
            Self::Hub => (2, 1),
            Self::Drive { cluster, .. } => (dos::root(cluster), u64::from(cluster) + 2),
        }
    }
    fn directory(&self) -> Result<PosixDir> {
        Ok(PosixDir::Mount(*self))
    }
    fn dirent(&self, offset: u64) -> Result<Option<DirEntry>> {
        Ok(self.dir_item(offset))
    }
    fn metadata(&self) -> Result<Metadata> {
        let (dev, ino) = self.identity();
        Ok(Metadata {
            dev: u64::from(dev),
            ino,
            mode: match self {
                Self::Drive { cluster, .. } if !driver::at(*cluster).read_only() => 0o040777,
                _ => 0o040555,
            },
            nlink: 2,
            ..Default::default()
        })
    }
    fn capacity(&self) -> Result<vfs::capacity::Capacity> {
        match *self {
            Self::Hub => vfs::posix::capacity(),
            Self::Drive { cluster, .. } => vfs::capacity::dos_volume(cluster),
        }
    }
    fn stamp(&self) -> Result<(u16, u16)> {
        Ok((0, 0))
    }
    fn set_stamp(&self, _: u16, _: u16) -> Result<()> {
        Err(Errno::ReadOnly)
    }
}

// Preserve driver paths on Btrfs. For relative operations based at a mount
// directory, project the saved directory identity back into the VFS namespace.
pub(crate) fn qualify<'a>(dir: PosixDir, path: &'a [u8]) -> Cow<'a, [u8]> {
    if path.is_empty() || path.starts_with(b"/") || path.get(1) == Some(&b':') {
        return Cow::Borrowed(path);
    }
    let mut base = match dir {
        PosixDir::Mount(node) => node.path(),
        d if d == ROOT && (path == b"dos" || path.starts_with(b"dos/")) => Vec::new(),
        _ => return Cow::Borrowed(path),
    };
    if base == b"/" {
        base.clear();
    }
    for part in path.split(|b| *b == b'/') {
        match part {
            b"" | b"." => (),
            b".." if base.is_empty() || MountDir::from_path(&base).is_some() => {
                base.truncate(base.iter().rposition(|b| *b == b'/').unwrap_or(0));
            }
            _ => {
                base.push(b'/');
                base.extend_from_slice(part);
            }
        }
    }
    if base.is_empty() {
        base.push(b'/');
    }
    Cow::Owned(base)
}

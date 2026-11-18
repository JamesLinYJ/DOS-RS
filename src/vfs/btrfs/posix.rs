//! POSIX paths use byte names and inode-valued working directories.
use super::{File, Mount, err, mounted};
use crate::vfs::posix::{Attribute, Credentials, DirEntry, Metadata, PosixDir, ROOT_INODE, Times};
use alloc::{collections::VecDeque, vec::Vec};
use dosrs_btrfs::{Error, inode::Inode};
use dosrs_native_posix::{Errno, Open, Result};

const PATH_MAX: usize = 4096;
const SYMLOOP_MAX: usize = 40;

fn access(n: &Inode, who: Credentials, mask: u32) -> Result<()> {
    if who.uid == 0 {
        return if mask & 1 == 0 || n.is_dir() || n.mode & 0o111 != 0 {
            Ok(())
        } else {
            Err(Errno::Access)
        };
    }
    let shift = if who.uid == n.uid {
        6
    } else if who.in_group(n.gid) {
        3
    } else {
        0
    };
    if (n.mode >> shift) & mask == mask {
        Ok(())
    } else {
        Err(Errno::Access)
    }
}
fn valid(path: &[u8]) -> Result<()> {
    if path.is_empty() {
        return Err(Errno::NoEntry);
    }
    if path.len() >= PATH_MAX {
        return Err(Errno::NameTooLong);
    }
    if path.contains(&0) {
        return Err(Errno::Invalid);
    }
    Ok(())
}
fn components(path: &[u8]) -> VecDeque<Vec<u8>> {
    let mut items: VecDeque<_> = path
        .split(|&b| b == b'/')
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect();
    if path.ends_with(b"/") {
        items.push_back(b".".to_vec());
    }
    items
}
enum Lookup {
    Existing(Inode),
    Missing(u64, Vec<u8>),
}
fn lookup(
    m: &Mount,
    cwd: PosixDir,
    path: &[u8],
    who: Credentials,
    follow: bool,
    create: bool,
) -> Result<Lookup> {
    valid(path)?;
    let mut here = if path.starts_with(b"/") {
        ROOT_INODE
    } else {
        cwd.inode()?
    };
    let mut todo = components(path);
    let mut links = 0;
    while let Some(name) = todo.pop_front() {
        let dir = m.fs.read_inode(here).map_err(err)?;
        if !dir.is_dir() {
            return Err(Errno::NotDirectory);
        }
        access(&dir, who, 1)?;
        match name.as_slice() {
            b"." => continue,
            b".." => {
                here = m.fs.directory_parent(here).map_err(err)?.0;
                continue;
            }
            n if n.len() > 255 => return Err(Errno::NameTooLong),
            _ => {}
        }
        let node = match m.fs.lookup(here, &name) {
            Ok(node) => node,
            Err(Error::NotFound) if create && todo.is_empty() => {
                return Ok(Lookup::Missing(here, name));
            }
            Err(e) => return Err(err(e)),
        };
        if node.is_symlink() && (follow || !todo.is_empty()) {
            links += 1;
            if links > SYMLOOP_MAX {
                return Err(Errno::Loop);
            }
            let target = m.fs.read_link(node.ino).map_err(err)?;
            valid(&target)?;
            if target.len() + todo.iter().map(|p| p.len() + 1).sum::<usize>() >= PATH_MAX {
                return Err(Errno::NameTooLong);
            }
            if target.starts_with(b"/") {
                here = ROOT_INODE;
            }
            let mut expanded = components(&target);
            expanded.append(&mut todo);
            todo = expanded;
        } else {
            here = node.ino;
        }
    }
    Ok(Lookup::Existing(m.fs.read_inode(here).map_err(err)?))
}
fn resolve(m: &Mount, cwd: PosixDir, path: &[u8], who: Credentials, follow: bool) -> Result<Inode> {
    match lookup(m, cwd, path, who, follow, false)? {
        Lookup::Existing(n) => Ok(n),
        Lookup::Missing(..) => Err(Errno::NoEntry),
    }
}
fn parent<'a>(
    m: &Mount,
    cwd: PosixDir,
    path: &'a [u8],
    who: Credentials,
) -> Result<(Inode, &'a [u8])> {
    valid(path)?;
    let end = path.iter().rposition(|&b| b != b'/').ok_or(Errno::Busy)? + 1;
    let start = path[..end]
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i + 1);
    let name = &path[start..end];
    if name == b"." || name == b".." {
        return Err(Errno::Invalid);
    }
    if name.len() > 255 {
        return Err(Errno::NameTooLong);
    }
    let dir = resolve(
        m,
        cwd,
        if start == 0 { b"." } else { &path[..start] },
        who,
        true,
    )?;
    if !dir.is_dir() {
        return Err(Errno::NotDirectory);
    }
    if dir.nlink == 0 {
        return Err(Errno::NoEntry);
    }
    // The mount namespace shadows any on-disk entry with the same name.
    if dir.ino == ROOT_INODE && name == b"dos" {
        return Err(Errno::Busy);
    }
    access(&dir, who, 3)?;
    Ok((dir, name))
}
fn absent(m: &Mount, dir: u64, name: &[u8]) -> Result<()> {
    match m.fs.lookup(dir, name) {
        Err(Error::NotFound) => Ok(()),
        Err(e) => Err(err(e)),
        Ok(_) => Err(Errno::Exists),
    }
}
fn check_remove(dir: &Inode, node: &Inode, who: Credentials) -> Result<()> {
    if dir.mode & 0o1000 != 0 && who.uid != 0 && who.uid != dir.uid && who.uid != node.uid {
        return Err(Errno::Permission);
    }
    Ok(())
}
pub(crate) fn metadata(n: &Inode) -> Metadata {
    let dates = [n.atime, n.mtime, n.ctime];
    Metadata {
        dev: 2,
        ino: n.ino,
        size: n.size,
        blocks: n.nbytes.div_ceil(512),
        mode: n.mode,
        nlink: n.nlink,
        uid: n.uid,
        gid: n.gid,
        times: dates.map(|t| (t.sec, t.nsec)),
    }
}
#[cfg(not(test))]
pub(crate) fn from_dos(dir: u32) -> Result<PosixDir> {
    mounted(|m| Ok(PosixDir::Btrfs(m.dirs.get(dir)?.id)))
}
pub(crate) fn chdir(cwd: PosixDir, path: &[u8], who: Credentials) -> Result<PosixDir> {
    mounted(|m| {
        let n = resolve(m, cwd, path, who, true)?;
        if !n.is_dir() {
            return Err(Errno::NotDirectory);
        }
        access(&n, who, 1)?;
        Ok(PosixDir::Btrfs(n.ino))
    })
}
pub(crate) fn pin_directory(dir: PosixDir) -> Result<super::super::File> {
    mounted(|m| {
        let inode = dir.inode()?;
        if !m.fs.read_inode(inode).map_err(err)?.is_dir() {
            return Err(Errno::NotDirectory);
        }
        *m.opens.entry(inode).or_default() += 1;
        Ok(alloc::boxed::Box::new(File(inode, false)) as super::super::File)
    })
}
pub(crate) fn getcwd(cwd: PosixDir) -> Result<Vec<u8>> {
    mounted(|m| {
        let mut path = VecDeque::new();
        let mut len = 1;
        let mut ino = cwd.inode()?;
        while ino != ROOT_INODE {
            let (parent, name) = m.fs.directory_parent(ino).map_err(err)?;
            len += name.len() + 1;
            if len > PATH_MAX {
                return Err(Errno::NameTooLong);
            }
            path.push_front(name);
            ino = parent;
        }
        let mut out = alloc::vec![b'/'];
        for name in path {
            if out.len() > 1 {
                out.push(b'/');
            }
            out.extend(name);
        }
        Ok(out)
    })
}
pub(crate) fn capacity() -> Result<crate::vfs::capacity::Capacity> {
    mounted(|m| {
        let sb = m.fs.superblock();
        let unit = u64::from(sb.sectorsize);
        let readonly = !m.fs.is_writable();
        Ok(crate::vfs::capacity::Capacity {
            block_size: unit,
            unit,
            blocks: sb.total_bytes / unit,
            free: sb.total_bytes.saturating_sub(sb.bytes_used) / unit,
            available: if readonly {
                0
            } else {
                m.fs.available_data_bytes().map_err(err)? / unit
            },
            fsid: u64::from_le_bytes(sb.fsid[..8].try_into().unwrap()),
            flags: 2 | u64::from(readonly),
            name_max: 255,
            fs_type: 0x9123683e,
        })
    })
}
pub(crate) fn stat(cwd: PosixDir, path: &[u8], who: Credentials, follow: bool) -> Result<Metadata> {
    mounted(|m| Ok(metadata(&resolve(m, cwd, path, who, follow)?)))
}
pub(crate) fn readlink(cwd: PosixDir, path: &[u8], who: Credentials) -> Result<Vec<u8>> {
    mounted(|m| {
        let n = resolve(m, cwd, path, who, false)?;
        if !n.is_symlink() {
            return Err(Errno::Invalid);
        }
        m.fs.read_link(n.ino).map_err(err)
    })
}
pub(crate) fn open(
    cwd: PosixDir,
    path: &[u8],
    flags: Open,
    mode: u32,
    who: Credentials,
    executable: bool,
) -> Result<(File, bool)> {
    mounted(|m| {
        if flags.truncate && !flags.access.writable() {
            return Err(Errno::Invalid);
        }
        let dirty = flags.truncate || flags.create;
        let ino = match lookup(
            m,
            cwd,
            path,
            who,
            !(flags.create && flags.exclusive),
            flags.create,
        )? {
            Lookup::Existing(n) => {
                if flags.create && flags.exclusive {
                    return Err(Errno::Exists);
                }
                if executable && !n.is_regular_file() {
                    return Err(Errno::Access);
                }
                if n.is_dir() && flags.access.writable() {
                    return Err(Errno::IsDirectory);
                }
                if !n.is_regular_file() && !n.is_dir() {
                    return Err(Errno::NotSupported);
                }
                if flags.access.writable() && !m.fs.is_writable() {
                    return Err(Errno::ReadOnly);
                }
                access(
                    &n,
                    who,
                    if executable {
                        1
                    } else {
                        (u32::from(flags.access.readable()) << 2)
                            | (u32::from(flags.access.writable()) << 1)
                    },
                )?;
                if flags.truncate {
                    m.fs.truncate(n.ino, 0).map_err(err)?;
                }
                n.ino
            }
            Lookup::Missing(dir, name) => {
                let parent = m.fs.read_inode(dir).map_err(err)?;
                if parent.nlink == 0 {
                    return Err(Errno::NoEntry);
                }
                access(&parent, who, 3)?;
                let gid = if parent.mode & 0o2000 != 0 {
                    parent.gid
                } else {
                    who.gid
                };
                m.fs.create(dir, &name, mode & 0o7777, who.uid, gid)
                    .map_err(err)?
            }
        };
        *m.opens.entry(ino).or_default() += 1;
        Ok((File(ino, false), dirty))
    })
}
pub(crate) fn mkdir(cwd: PosixDir, path: &[u8], mode: u32, who: Credentials) -> Result<()> {
    mounted(|m| {
        let (dir, name) = parent(m, cwd, path, who)?;
        absent(m, dir.ino, name)?;
        let inherited = dir.mode & 0o2000;
        let gid = if inherited != 0 { dir.gid } else { who.gid };
        m.fs.mkdir(dir.ino, name, (mode & 0o7777) | inherited, who.uid, gid)
            .map(|_| ())
            .map_err(err)
    })
}
pub(crate) fn symlink(cwd: PosixDir, target: &[u8], path: &[u8], who: Credentials) -> Result<()> {
    mounted(|m| {
        valid(target)?;
        let (dir, name) = parent(m, cwd, path, who)?;
        absent(m, dir.ino, name)?;
        if path.ends_with(b"/") {
            return Err(Errno::NoEntry);
        }
        m.fs.symlink(dir.ino, name, target, who.uid, who.gid)
            .map(|_| ())
            .map_err(err)
    })
}
pub(crate) fn link(cwd: PosixDir, old: &[u8], new: &[u8], who: Credentials) -> Result<()> {
    link_at(cwd, old, cwd, new, false, who)
}
pub(crate) fn link_at(
    cwd: PosixDir,
    old: &[u8],
    dest: PosixDir,
    new: &[u8],
    follow: bool,
    who: Credentials,
) -> Result<()> {
    mounted(|m| {
        let n = resolve(m, cwd, old, who, follow)?;
        if n.is_dir() {
            return Err(Errno::Permission);
        }
        let (dir, name) = parent(m, dest, new, who)?;
        absent(m, dir.ino, name)?;
        if new.ends_with(b"/") {
            return Err(Errno::NoEntry);
        }
        m.fs.link(n.ino, dir.ino, name).map_err(err)
    })
}
pub(crate) fn unlink(cwd: PosixDir, path: &[u8], who: Credentials) -> Result<()> {
    mounted(|m| {
        let (dir, name) = parent(m, cwd, path, who)?;
        let n = m.fs.lookup(dir.ino, name).map_err(err)?;
        check_remove(&dir, &n, who)?;
        if n.is_dir() {
            return Err(Errno::IsDirectory);
        }
        if path.ends_with(b"/") {
            return Err(Errno::NotDirectory);
        }
        if n.nlink == 1 && m.opens.contains_key(&n.ino) {
            m.fs.unlink_open(dir.ino, name).map_err(err)?;
            m.orphans.insert(n.ino);
        } else {
            m.fs.unlink(dir.ino, name).map_err(err)?;
        }
        Ok(())
    })
}
pub(crate) fn rmdir(cwd: PosixDir, path: &[u8], who: Credentials) -> Result<()> {
    mounted(|m| {
        let (dir, name) = parent(m, cwd, path, who)?;
        let n = m.fs.lookup(dir.ino, name).map_err(err)?;
        check_remove(&dir, &n, who)?;
        if !n.is_dir() {
            return Err(Errno::NotDirectory);
        }
        if !m.fs.read_dir(n.ino).map_err(err)?.is_empty() {
            return Err(Errno::NotEmpty);
        }
        m.remove_directory(dir.ino, name, n.ino)?;
        Ok(())
    })
}
pub(crate) fn check_access(cwd: PosixDir, path: &[u8], mask: u32, who: Credentials) -> Result<()> {
    change_at(cwd, path, who, true, Attribute::Access(mask))
}

pub(crate) fn change_at(
    cwd: PosixDir,
    path: &[u8],
    who: Credentials,
    follow: bool,
    attr: Attribute,
) -> Result<()> {
    mounted(|m| {
        let n = resolve(m, cwd, path, who, follow)?;
        match attr {
            Attribute::Access(mask) => {
                if mask & !7 != 0 {
                    return Err(Errno::Invalid);
                }
                if mask & 2 != 0 && !m.fs.is_writable() {
                    return Err(Errno::ReadOnly);
                }
                access(&n, who, mask)
            }
            Attribute::Mode(mode) => permissions(m, &n, mode, who),
            Attribute::Owner(uid, gid) => owner(m, &n, uid, gid, who),
            Attribute::Times(times) => change_times(m, &n, times, who),
        }
    })
}
fn change_times(m: &mut Mount, n: &Inode, times: Times, who: Credentials) -> Result<()> {
    let [atime, mtime] = times.value;
    if atime.is_none() && mtime.is_none() {
        return Ok(());
    }
    if who.uid != 0 && who.uid != n.uid {
        if !times.both_now {
            return Err(Errno::Permission);
        }
        access(n, who, 2)?;
    }
    m.fs.set_times(n.ino, atime, mtime).map_err(err)
}
pub(super) fn futimens(ino: u64, times: Times, who: Credentials) -> Result<()> {
    mounted(|m| {
        let n = m.fs.read_inode(ino).map_err(err)?;
        change_times(m, &n, times, who)
    })
}

fn owner(m: &mut Mount, n: &Inode, uid: u32, gid: u32, who: Credentials) -> Result<()> {
    if who.uid != 0
        && (who.uid != n.uid
            || uid != u32::MAX && uid != n.uid
            || gid != u32::MAX && !who.in_group(gid) && gid != n.gid)
    {
        return Err(Errno::Permission);
    }
    m.fs.set_owner(
        n.ino,
        (uid != u32::MAX).then_some(uid),
        (gid != u32::MAX).then_some(gid),
    )
    .map_err(err)
}
fn permissions(m: &mut Mount, n: &Inode, mode: u32, who: Credentials) -> Result<()> {
    if who.uid != 0 && who.uid != n.uid {
        return Err(Errno::Permission);
    }
    let mask = if who.uid != 0 && !who.in_group(n.gid) {
        0o5777
    } else {
        0o7777
    };
    m.fs.set_mode(n.ino, mode & mask).map_err(err)
}
pub(crate) fn chown(
    cwd: PosixDir,
    path: &[u8],
    uid: u32,
    gid: u32,
    who: Credentials,
) -> Result<()> {
    change_at(cwd, path, who, true, Attribute::Owner(uid, gid))
}
pub(super) fn fchown(ino: u64, uid: u32, gid: u32, who: Credentials) -> Result<()> {
    mounted(|m| {
        let n = m.fs.read_inode(ino).map_err(err)?;
        owner(m, &n, uid, gid, who)
    })
}
pub(crate) fn chmod(cwd: PosixDir, path: &[u8], mode: u32, who: Credentials) -> Result<()> {
    change_at(cwd, path, who, true, Attribute::Mode(mode))
}
pub(super) fn fchmod(ino: u64, mode: u32, who: Credentials) -> Result<()> {
    mounted(|m| {
        let n = m.fs.read_inode(ino).map_err(err)?;
        permissions(m, &n, mode, who)
    })
}
pub(crate) fn rename(cwd: PosixDir, old: &[u8], new: &[u8], who: Credentials) -> Result<()> {
    rename_at(cwd, old, cwd, new, who)
}
pub(crate) fn rename_at(
    cwd: PosixDir,
    old: &[u8],
    dest: PosixDir,
    new: &[u8],
    who: Credentials,
) -> Result<()> {
    mounted(|m| {
        let (src, a) = parent(m, cwd, old, who)?;
        let (dst, b) = parent(m, dest, new, who)?;
        let source = m.fs.lookup(src.ino, a).map_err(err)?;
        check_remove(&src, &source, who)?;
        if (old.ends_with(b"/") || new.ends_with(b"/")) && !source.is_dir() {
            return Err(Errno::NotDirectory);
        }
        let pinned = match m.fs.lookup(dst.ino, b) {
            Ok(node) => {
                check_remove(&dst, &node, who)?;
                if node.ino == source.ino {
                    return Ok(());
                }
                if node.is_dir() && !m.fs.read_dir(node.ino).map_err(err)?.is_empty() {
                    return Err(Errno::NotEmpty);
                }
                (node.nlink == 1
                    && m.opens.contains_key(&node.ino)
                    && (node.is_regular_file() || node.is_dir()))
                .then_some(node.ino)
            }
            Err(Error::NotFound) => None,
            Err(e) => return Err(err(e)),
        };
        m.fs.rename_pinned(src.ino, a, dst.ino, b, pinned)
            .map_err(err)?;
        if let Some(ino) = pinned {
            m.orphans.insert(ino);
        }
        Ok(())
    })
}
pub(crate) fn dirent(ino: u64, index: u64) -> Result<Option<DirEntry>> {
    mounted(|m| {
        let n = m.fs.read_inode(ino).map_err(err)?;
        if !n.is_dir() {
            return Err(Errno::NotDirectory);
        }
        if n.nlink == 0 {
            return Ok(None);
        }
        if index <= 1 {
            let (child, name) = if index == 0 {
                (ino, b".".to_vec())
            } else {
                (m.fs.directory_parent(ino).map_err(err)?.0, b"..".to_vec())
            };
            return Ok(Some(DirEntry {
                ino: child,
                name,
                kind: 4,
            }));
        }
        let mut offset = index - 2;
        if ino == ROOT_INODE {
            if offset == 0 {
                return Ok(Some(DirEntry {
                    ino: 1,
                    name: b"dos".to_vec(),
                    kind: 4,
                }));
            }
            offset -= 1;
        }
        let entries = m.fs.read_dir(ino).map_err(err)?;
        let Some(e) = entries
            .into_iter()
            .filter(|entry| ino != ROOT_INODE || entry.name != b"dos")
            .nth(usize::try_from(offset).map_err(|_| Errno::Invalid)?)
        else {
            return Ok(None);
        };
        let kind = if e.is_inode() {
            ((m.fs.read_inode(e.ino).map_err(err)?.mode >> 12) & 15) as u8
        } else {
            4
        };
        Ok(Some(DirEntry {
            ino: e.ino,
            name: e.name,
            kind,
        }))
    })
}

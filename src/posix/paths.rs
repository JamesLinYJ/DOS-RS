//! Native VFS syscall boundary. Paths are NUL-terminated byte strings.
#[path = "paths/at.rs"]
mod at;
#[path = "paths/attributes.rs"]
mod attributes;
use super::{executable::errno, io::ProcessContext};
use crate::{arch::user::AddressSpace, vfs::posix as fs};
use alloc::vec::Vec;
use dosrs_native_exec::Call;
use dosrs_native_posix::{Access, Open};
type Result<T> = core::result::Result<T, isize>;
pub(super) fn string(image: &AddressSpace, address: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut offset = 0;
    while offset < 4096 {
        let at = address.checked_add(offset).ok_or(-14isize)?;
        let n = (4096 - at % 4096).min(4096 - offset).min(64);
        let mut part = [0; 64];
        image.read(at, &mut part[..n]).map_err(|_| -14isize)?;
        if let Some(end) = part[..n].iter().position(|&b| b == 0) {
            bytes.extend_from_slice(&part[..end]);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&part[..n]);
        offset += n;
    }
    Err(-36)
}
fn root_path(image: &AddressSpace, at: usize) -> Result<Vec<u8>> {
    let path = string(image, at)?;
    if !fs::native(&path) {
        return Err(-95);
    }
    Ok(path)
}
fn same_root(image: &AddressSpace, a: usize, b: usize) -> Result<(Vec<u8>, Vec<u8>)> {
    let old = string(image, a)?;
    let new = string(image, b)?;
    if fs::native(&old) != fs::native(&new) {
        return Err(-18);
    }
    if !fs::native(&old) {
        return Err(-95);
    }
    Ok((old, new))
}
fn open_flags(b: usize) -> Result<Open> {
    let access = match b & 3 {
        0 => Access::Read,
        1 => Access::Write,
        2 => Access::ReadWrite,
        _ => return Err(-22),
    };
    if b & !(0x6c3 | 0x800 | 0x10000 | 0x20000 | 0x80000 | 0x04000000) != 0 {
        return Err(-22);
    }
    Ok(Open {
        access,
        create: b & 64 != 0,
        exclusive: b & 128 != 0,
        truncate: b & 512 != 0,
        append: b & 1024 != 0,
    })
}
pub(super) fn dispatch(
    image: &mut AddressSpace,
    p: &mut ProcessContext,
    call: Call,
    [a, b, c]: [usize; 3],
) -> Option<Result<usize>> {
    if let Some(result) = attributes::dispatch(image, p, call, [a, b, c]) {
        return Some(result);
    }
    if let Some(result) = at::dispatch(image, p, call, [a, b, c]) {
        return Some(result);
    }
    if !matches!(
        call,
        Call::Open
            | Call::Openmode
            | Call::Unlink
            | Call::Chdir
            | Call::Getcwd
            | Call::Stat
            | Call::Fstat
            | Call::Mkdir
            | Call::Rmdir
            | Call::Link
            | Call::Symlink
            | Call::Readlink
            | Call::Chmod
            | Call::Getdents
            | Call::Rename
            | Call::Umask
            | Call::Setuid
            | Call::Setgid
            | Call::Chown
    ) {
        return None;
    }
    Some((|| {
        let dir = p.directory;
        let who = p.credentials();
        match call {
            Call::Open | Call::Openmode => at::open(
                image,
                p,
                (-100isize) as usize,
                a,
                b,
                if call == Call::Open { 0o666 } else { c as u32 },
            ),
            Call::Unlink => p.unlink_path(&string(image, a)?).map(|_| 0).map_err(errno),
            Call::Chdir => {
                p.chdir(dir, &root_path(image, a)?).map_err(errno)?;
                Ok(0)
            }
            Call::Getcwd => {
                let mut path = fs::getcwd(dir).map_err(errno)?;
                path.push(0);
                if path.len() > b {
                    return Err(-34);
                }
                image.write(a, &path).map_err(|_| -14isize)?;
                Ok(path.len())
            }
            Call::Stat | Call::Fstat => {
                if call == Call::Stat && c > 1 {
                    return Err(-22);
                }
                let meta = if call == Call::Fstat {
                    p.metadata(a)
                } else {
                    p.path_metadata(&string(image, a)?, c == 0)
                }
                .map_err(errno)?;
                image.write(b, &meta.bytes()).map_err(|_| -14isize)?;
                Ok(0)
            }
            Call::Mkdir => fs::mkdir(dir, &root_path(image, a)?, b as u32 & !p.mask, who)
                .map(|_| 0)
                .map_err(errno),
            Call::Rmdir => fs::rmdir(dir, &root_path(image, a)?, who)
                .map(|_| 0)
                .map_err(errno),
            Call::Link => {
                let (old, new) = same_root(image, a, b)?;
                fs::link(dir, &old, &new, who).map(|_| 0).map_err(errno)
            }
            Call::Symlink => fs::symlink(dir, &string(image, a)?, &root_path(image, b)?, who)
                .map(|_| 0)
                .map_err(errno),
            Call::Readlink => {
                if c == 0 {
                    return Err(-22);
                }
                let data = fs::readlink(dir, &root_path(image, a)?, who).map_err(errno)?;
                let n = c.min(data.len());
                image.write(b, &data[..n]).map_err(|_| -14isize)?;
                Ok(n)
            }
            Call::Chmod => fs::chmod(dir, &root_path(image, a)?, b as u32, who)
                .map(|_| 0)
                .map_err(errno),
            Call::Getdents => {
                image.check(b, c.min(265), true).map_err(|_| -14isize)?;
                let bytes = p.direntry(a, c).map_err(errno)?;
                image.write(b, &bytes).map_err(|_| -14isize)?;
                Ok(bytes.len())
            }
            Call::Rename => {
                let (old, new) = same_root(image, a, b)?;
                fs::rename(dir, &old, &new, who).map(|_| 0).map_err(errno)
            }
            Call::Setuid | Call::Setgid => {
                if a >= u32::MAX as usize {
                    return Err(-22);
                }
                let id = a as u32;
                let current = if call == Call::Setuid { p.uid } else { p.gid };
                if p.uid != 0 && id != current {
                    return Err(-1);
                }
                if call == Call::Setuid {
                    p.uid = id;
                } else {
                    p.gid = id;
                }
                Ok(0)
            }
            Call::Chown => fs::chown(dir, &root_path(image, a)?, b as u32, c as u32, who)
                .map(|_| 0)
                .map_err(errno),
            Call::Umask => Ok(core::mem::replace(&mut p.mask, a as u32 & 0o777) as usize),
            _ => unreachable!(),
        }
    })())
}

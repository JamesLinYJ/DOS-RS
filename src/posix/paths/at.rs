//! Directory-relative system calls and checked wire structures.
use super::*;
use crate::vfs::posix::PosixDir;
const FDCWD: i32 = -100;
const NOFOLLOW: usize = 0x100;
const REMOVEDIR: usize = 0x200;
const FOLLOW: usize = 0x400;
const EMPTY: usize = 0x1000;

pub(super) fn words<const N: usize>(image: &AddressSpace, address: usize) -> Result<[usize; N]> {
    let mut result = [0; N];
    for (i, v) in result.iter_mut().enumerate() {
        let mut bytes = [0; size_of::<usize>()];
        let at = address.checked_add(i * bytes.len()).ok_or(-14isize)?;
        image.read(at, &mut bytes).map_err(|_| -14isize)?;
        *v = usize::from_le_bytes(bytes);
    }
    Ok(result)
}
fn length(image: &AddressSpace, address: usize) -> Result<u64> {
    let mut bytes = [0; 8];
    image.read(address, &mut bytes).map_err(|_| -14isize)?;
    u64::try_from(i64::from_le_bytes(bytes)).map_err(|_| -22)
}
pub(super) fn open(
    image: &AddressSpace,
    p: &mut ProcessContext,
    fd: usize,
    addr: usize,
    bits: usize,
    mode: u32,
) -> Result<usize> {
    let path = string(image, addr)?;
    let flags = open_flags(bits)?;
    let dir = p.at(fd, &path).map_err(errno)?;
    if bits & 0x10000 != 0 && flags.create {
        return Err(-22);
    }
    if bits & (0x10000 | 0x20000) != 0 {
        match p.metadata_at(dir, &path, bits & 0x20000 == 0) {
            Ok(meta) if meta.mode & 0o170000 == 0o120000 => {
                return Err(if flags.create && flags.exclusive {
                    -17
                } else {
                    -40
                });
            }
            Ok(meta) if bits & 0x10000 != 0 && meta.mode & 0o170000 != 0o040000 => return Err(-20),
            Ok(_) => (),
            Err(dosrs_native_posix::Errno::NoEntry) if flags.create => (),
            Err(e) => return Err(errno(e)),
        }
    }
    let new = p.open_at(dir, &path, flags, mode).map_err(errno)?;
    p.set_open_flags(new, bits);
    if bits & 0x800 != 0 {
        p.fcntl(new, 4, bits).map_err(errno)?;
    }
    Ok(new)
}
fn native_dir(
    image: &AddressSpace,
    p: &ProcessContext,
    fd: usize,
    addr: usize,
) -> Result<(PosixDir, Vec<u8>)> {
    let path = root_path(image, addr)?;
    let dir = p.at(fd, &path).map_err(errno)?;
    Ok((dir, path))
}
pub(super) fn dispatch(
    image: &mut AddressSpace,
    p: &mut ProcessContext,
    call: Call,
    [a, b, c]: [usize; 3],
) -> Option<Result<usize>> {
    if !matches!(
        call,
        Call::Openat
            | Call::Fstatat
            | Call::Unlinkat
            | Call::Mkdirat
            | Call::Renameat
            | Call::Linkat
            | Call::Fcntl
            | Call::Ftruncate
            | Call::Truncate
            | Call::Fsync
            | Call::Fchmod
            | Call::Fchown
            | Call::Access
            | Call::Readlinkat
            | Call::Symlinkat
    ) {
        return None;
    }
    Some((|| {
        let who = p.credentials();
        match call {
            Call::Openat => {
                let mut bytes = [0; 8];
                image.read(c, &mut bytes).map_err(|_| -14isize)?;
                open(
                    image,
                    p,
                    a,
                    b,
                    u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize,
                    u32::from_le_bytes(bytes[4..].try_into().unwrap()),
                )
            }
            Call::Fstatat => {
                let [output, flags] = words(image, c)?;
                if flags & !(NOFOLLOW | EMPTY) != 0 {
                    return Err(-22);
                }
                let path = string(image, b)?;
                let meta = if path.is_empty() && flags & EMPTY != 0 {
                    if a as i32 == FDCWD {
                        p.path_metadata(b".", true)
                    } else {
                        p.metadata(a)
                    }
                } else {
                    p.metadata_at(p.at(a, &path).map_err(errno)?, &path, flags & NOFOLLOW == 0)
                }
                .map_err(errno)?;
                image.write(output, &meta.bytes()).map_err(|_| -14isize)?;
                Ok(0)
            }
            Call::Unlinkat => {
                if c & !REMOVEDIR != 0 {
                    return Err(-22);
                }
                if c & REMOVEDIR != 0 {
                    return directory_change(image, p, a, b, None);
                }
                let path = string(image, b)?;
                let dir = p.at(a, &path).map_err(errno)?;
                let name = crate::vfs::mounts::qualify(dir, &path);
                if fs::is_mount_point(dir, &name) {
                    return Err(-16);
                }
                if fs::native(&name) {
                    fs::unlink(dir, &name, who).map_err(errno)?;
                } else {
                    p.unlink_path(&name).map_err(errno)?;
                }
                Ok(0)
            }
            Call::Mkdirat => directory_change(image, p, a, b, Some(c as u32)),
            Call::Renameat | Call::Linkat => {
                let [newfd, newpath] = words(image, c)?;
                let (old, new) = same_root(image, b, newpath)?;
                let from = p.at(a, &old).map_err(errno)?;
                let to = p.at(newfd, &new).map_err(errno)?;
                if call == Call::Renameat {
                    fs::rename_at(from, &old, to, &new, who)
                } else {
                    let [_, _, flags] = words(image, c)?;
                    if flags & !FOLLOW != 0 {
                        return Err(-22);
                    }
                    fs::link_at(from, &old, to, &new, flags & FOLLOW != 0, who)
                }
                .map_err(errno)?;
                Ok(0)
            }
            Call::Fcntl => p.fcntl(a, b, c).map_err(errno),
            Call::Ftruncate => {
                p.fd_resize(a, length(image, b)?).map_err(errno)?;
                Ok(0)
            }
            Call::Truncate => {
                p.resize_path(&string(image, a)?, length(image, b)?)
                    .map_err(errno)?;
                Ok(0)
            }
            Call::Fsync => {
                p.fd_flush(a).map_err(errno)?;
                Ok(0)
            }
            Call::Fchmod => {
                p.fd_chmod(a, b as u32).map_err(errno)?;
                Ok(0)
            }
            Call::Fchown => {
                p.fd_chown(a, b as u32, c as u32).map_err(errno)?;
                Ok(0)
            }
            Call::Access => {
                let path = string(image, a)?;
                if b & !7 != 0 {
                    return Err(-22);
                }
                if fs::native(&path) {
                    fs::check_access(p.directory, &path, b as u32, who).map_err(errno)?;
                } else {
                    p.path_metadata(&path, true).map_err(errno)?;
                }
                Ok(0)
            }
            Call::Readlinkat => {
                let [out, len] = words(image, c)?;
                if len == 0 {
                    return Err(-22);
                }
                let path = string(image, b)?;
                let dir = p.at(a, &path).map_err(errno)?;
                let bytes = fs::readlink(dir, &path, who).map_err(errno)?;
                let n = len.min(bytes.len());
                image.write(out, &bytes[..n]).map_err(|_| -14isize)?;
                Ok(n)
            }
            Call::Symlinkat => {
                let (dir, path) = native_dir(image, p, b, c)?;
                fs::symlink(dir, &string(image, a)?, &path, who).map_err(errno)?;
                Ok(0)
            }
            _ => unreachable!(),
        }
    })())
}

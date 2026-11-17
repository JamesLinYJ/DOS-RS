//! Attribute changes resolve exactly one inode while holding the mount lock.
use super::*;
use fs::{Attribute, Times};

const UTIME_NOW: i64 = (1 << 30) - 1;
const UTIME_OMIT: i64 = UTIME_NOW - 1;
const AT_EACCESS: usize = 0x200;
const AT_SYMLINK_NOFOLLOW: usize = 0x100;

fn times(image: &AddressSpace, address: usize) -> Result<Times> {
    let mut raw = [(0, UTIME_NOW); 2];
    if address != 0 {
        let mut wire = [0; 32];
        image.read(address, &mut wire).map_err(|_| -14isize)?;
        for (slot, bytes) in raw.iter_mut().zip(wire.chunks_exact(16)) {
            *slot = (
                i64::from_le_bytes(bytes[..8].try_into().unwrap()),
                i64::from_le_bytes(bytes[8..].try_into().unwrap()),
            );
        }
    }
    let mut value = [None; 2];
    let mut now = None;
    for (set, (sec, ns)) in value.iter_mut().zip(raw) {
        *set = match ns {
            UTIME_NOW => Some(*now.get_or_insert_with(|| {
                let t = crate::bios::rtc::now();
                (t.as_secs() as i64, t.subsec_nanos())
            })),
            UTIME_OMIT => None,
            0..1_000_000_000 => Some((sec, ns as u32)),
            _ => return Err(-22),
        };
    }
    Ok(Times {
        value,
        both_now: raw.iter().all(|&(_, ns)| ns == UTIME_NOW),
    })
}

fn positioned(
    image: &mut AddressSpace,
    p: &ProcessContext,
    call: Call,
    fd: usize,
    address: usize,
    options: usize,
) -> Result<usize> {
    let mut wire = [0; 16];
    image.read(options, &mut wire).map_err(|_| -14isize)?;
    let offset =
        u64::try_from(i64::from_le_bytes(wire[..8].try_into().unwrap())).map_err(|_| -22isize)?;
    let count = u64::from_le_bytes(wire[8..].try_into().unwrap());
    if count > isize::MAX as u64 {
        return Err(-22);
    }
    if count > 0
        && offset
            .checked_add(count)
            .is_none_or(|end| end > i64::MAX as u64)
    {
        return Err(-27);
    }
    let length = count.min(p.transfer_limit(fd) as u64) as usize;
    let mut buf = Vec::new();
    buf.try_reserve_exact(length).map_err(|_| -12isize)?;
    buf.resize(length, 0);
    if call == Call::Pwrite {
        image.read(address, &mut buf).map_err(|_| -14isize)?;
        return p.pwrite(fd, offset, &buf).map_err(errno);
    }
    image.check(address, length, true).map_err(|_| -14isize)?;
    let n = p.pread(fd, offset, &mut buf).map_err(errno)?;
    image.write(address, &buf[..n]).map_err(|_| -14isize)?;
    Ok(n)
}

pub(super) fn dispatch(
    image: &mut AddressSpace,
    p: &mut ProcessContext,
    call: Call,
    [a, b, c]: [usize; 3],
) -> Option<Result<usize>> {
    if !matches!(
        call,
        Call::Statvfs
            | Call::Fstatvfs
            | Call::Fchdir
            | Call::Faccessat
            | Call::Fchmodat
            | Call::Fchownat
            | Call::Utimensat
            | Call::Futimens
            | Call::Pread
            | Call::Pwrite
    ) {
        return None;
    }
    Some((|| {
        match call {
            Call::Statvfs | Call::Fstatvfs => {
                let s = if call == Call::Fstatvfs {
                    p.file_capacity(a)
                } else {
                    p.path_capacity(&string(image, a)?)
                }
                .map_err(errno)?;
                image.write(b, &s.bytes()).map_err(|_| -14isize)?;
            }
            Call::Fchdir => {
                if a as i32 == -100 {
                    return Err(-9);
                }
                let dir = p.at(a, b".").map_err(errno)?;
                p.chdir(dir, b".").map_err(errno)?;
            }
            Call::Futimens => p.fd_times(a, times(image, b)?).map_err(errno)?,
            Call::Pread | Call::Pwrite => return positioned(image, p, call, a, b, c),
            _ => {
                let (attr, flags) = match call {
                    Call::Faccessat | Call::Fchmodat => {
                        let [mode, flags] = at::words(image, c)?;
                        if call == Call::Fchmodat {
                            (Attribute::Mode(mode as u32), flags)
                        } else {
                            if mode & !7 != 0 {
                                return Err(-22);
                            }
                            (Attribute::Access(mode as u32), flags)
                        }
                    }
                    Call::Fchownat => {
                        let [uid, gid, flags] = at::words(image, c)?;
                        (Attribute::Owner(uid as u32, gid as u32), flags)
                    }
                    Call::Utimensat => {
                        let [ptr, flags] = at::words(image, c)?;
                        (Attribute::Times(times(image, ptr)?), flags)
                    }
                    _ => unreachable!(),
                };
                let valid = AT_SYMLINK_NOFOLLOW
                    | if call == Call::Faccessat {
                        AT_EACCESS
                    } else {
                        0
                    };
                if flags & !valid != 0 {
                    return Err(-22);
                }
                let follow = flags & AT_SYMLINK_NOFOLLOW == 0;
                let path = string(image, b)?;
                let dir = p.at(a, &path).map_err(errno)?;
                if fs::native(&path) {
                    fs::change_at(dir, &path, p.credentials(), follow, attr).map_err(errno)?;
                } else if let Attribute::Access(mask) = attr {
                    let meta = p.path_metadata(&path, follow).map_err(errno)?;
                    if mask & 2 != 0 && meta.mode & 0o222 == 0 {
                        return Err(-13);
                    }
                } else {
                    return Err(-95);
                }
            }
        }
        Ok(0)
    })())
}

//! Descriptor flags belong to a process slot. Status flags belong to the shared open description.
use super::*;
use crate::vfs::posix::{PosixDir, ROOT};
use core::sync::atomic::{AtomicU32, Ordering};

pub(in crate::posix) const CLOFORK: usize = 0x04000000;
pub(in crate::posix) const CLOEXEC: usize = 0x80000;
const APPEND: u32 = 0x400;
pub(in crate::posix) const NONBLOCK: u32 = 0x800;
pub(in crate::posix) struct Shared {
    description: KernelCell<Description>,
    flags: AtomicU32,
}
impl core::ops::Deref for Shared {
    type Target = KernelCell<Description>;
    fn deref(&self) -> &Self::Target {
        &self.description
    }
}
impl Drop for Shared {
    fn drop(&mut self) {
        record::release_description(self as *const _ as usize);
    }
}
impl Shared {
    pub(super) fn new(description: Description) -> Self {
        let flags = match &description {
            Description::Input(_) => 0,
            Description::Output(_) => 1,
            Description::Pipe(end) => u32::from(!end.read_end),
            Description::File { access, append, .. } => {
                (match access {
                    Access::Read => 0,
                    Access::Write => 1,
                    Access::ReadWrite => 2,
                }) | if *append { APPEND } else { 0 }
            }
        };
        Self {
            description: KernelCell::new(description),
            flags: AtomicU32::new(flags),
        }
    }
    pub(super) fn nonblock(&self) -> bool {
        self.flags.load(Ordering::Relaxed) & NONBLOCK != 0
    }
    pub(super) fn blocking(&self, result: Poll<Result<usize>>) -> Poll<Result<usize>> {
        if result.is_pending() && self.flags.load(Ordering::Relaxed) & NONBLOCK != 0 {
            return Poll::Ready(Err(Errno::Again));
        }
        result
    }
}

impl ProcessContext {
    pub(in crate::posix) fn at(&self, fd: usize, path: &[u8]) -> Result<PosixDir> {
        if path.starts_with(b"/") {
            return Ok(ROOT);
        }
        if fd as i32 == -100 {
            return Ok(self.directory);
        }
        match &*self.get(fd)?.borrow() {
            Description::File { object, .. } => object.directory(),
            _ => Err(Errno::NotDirectory),
        }
    }
    pub(in crate::posix) fn set_open_flags(&mut self, fd: usize, flags: usize) {
        let bit = 1u32 << fd;
        self.close_on_exec =
            (self.close_on_exec & !bit) | if flags & CLOEXEC != 0 { bit } else { 0 };
        self.close_on_fork =
            (self.close_on_fork & !bit) | if flags & CLOFORK != 0 { bit } else { 0 };
    }
    pub(in crate::posix) fn dup3(&mut self, old: usize, new: usize, flags: usize) -> Result<usize> {
        if flags & !(CLOEXEC | CLOFORK) != 0 || old == new {
            return Err(Errno::Invalid);
        }
        let fd = self.dup2(old, new)?;
        self.set_open_flags(fd, flags);
        Ok(fd)
    }
    pub(in crate::posix) fn pipe2(&mut self, flags: usize) -> Result<(usize, usize)> {
        if flags & !(CLOEXEC | CLOFORK | NONBLOCK as usize) != 0 {
            return Err(Errno::Invalid);
        }
        let (r, w) = self.pipe()?;
        for fd in [r, w] {
            self.set_open_flags(fd, flags);
            if flags & NONBLOCK as usize != 0 {
                self.fcntl(fd, 4, flags)?;
            }
        }
        Ok((r, w))
    }
    pub(in crate::posix) fn fcntl(&mut self, fd: usize, cmd: usize, arg: usize) -> Result<usize> {
        let h = self.get(fd)?.clone();
        match cmd {
            0 | 1030 | 1100 => {
                if arg >= OPEN_MAX {
                    return Err(Errno::Invalid);
                }
                let target = (arg..OPEN_MAX)
                    .find(|&n| self.fds.get(n).is_none_or(Option::is_none))
                    .ok_or(Errno::TooManyFiles)?;
                self.fds
                    .try_reserve(target.saturating_sub(self.fds.len()) + 1)
                    .map_err(|_| Errno::TooManyFiles)?;
                if self.fds.len() < target {
                    self.fds.resize_with(target, || None);
                }
                self.set(target, h);
                let flags = match cmd {
                    1030 => CLOEXEC,
                    1100 => CLOFORK,
                    _ => 0,
                };
                self.set_open_flags(target, flags);
                Ok(target)
            }
            1 => Ok(
                (((self.close_on_exec >> fd) & 1) | (((self.close_on_fork >> fd) & 1) << 1))
                    as usize,
            ),
            2 => {
                self.set_open_flags(
                    fd,
                    (if arg & 1 != 0 { CLOEXEC } else { 0 })
                        | (if arg & 2 != 0 { CLOFORK } else { 0 }),
                );
                Ok(0)
            }
            3 => Ok(h.flags.load(Ordering::Relaxed) as usize),
            4 => {
                let status = (arg as u32) & (APPEND | NONBLOCK);
                let mut d = h.borrow();
                if let Description::File { append, .. } = &mut *d {
                    *append = status & APPEND != 0;
                }
                let old = h.flags.load(Ordering::Relaxed);
                h.flags
                    .store((old & !(APPEND | NONBLOCK)) | status, Ordering::Relaxed);
                Ok(0)
            }
            _ => Err(Errno::Invalid),
        }
    }
    pub(in crate::posix) fn fd_resize(&self, fd: usize, length: u64) -> Result<()> {
        match &mut *self.get(fd)?.borrow() {
            Description::File {
                object,
                access,
                dirty,
                ..
            } if access.writable() => {
                object.resize(length)?;
                *dirty = true;
                Ok(())
            }
            Description::File { .. } => Err(Errno::BadFd),
            _ => Err(Errno::Invalid),
        }
    }
    pub(in crate::posix) fn fd_flush(&self, fd: usize) -> Result<()> {
        match &mut *self.get(fd)?.borrow() {
            Description::File { object, dirty, .. } => {
                object.sync()?;
                *dirty = false;
                Ok(())
            }
            _ => Err(Errno::Invalid),
        }
    }
    pub(in crate::posix) fn fd_chmod(&self, fd: usize, mode: u32) -> Result<()> {
        match &*self.get(fd)?.borrow() {
            Description::File { object, .. } => object.chmod(mode, self.credentials()),
            _ => Err(Errno::NotSupported),
        }
    }
    pub(in crate::posix) fn fd_chown(&self, fd: usize, uid: u32, gid: u32) -> Result<()> {
        match &*self.get(fd)?.borrow() {
            Description::File { object, .. } => object.chown(uid, gid, self.credentials()),
            _ => Err(Errno::NotSupported),
        }
    }
    pub(in crate::posix) fn fd_times(&self, fd: usize, times: vfs::posix::Times) -> Result<()> {
        match &*self.get(fd)?.borrow() {
            Description::File { object, .. } => object.set_times(times, self.credentials()),
            _ => Err(Errno::NotSupported),
        }
    }
    pub(in crate::posix) fn pread(&self, fd: usize, offset: u64, data: &mut [u8]) -> Result<usize> {
        match &*self.get(fd)?.borrow() {
            Description::File { object, access, .. } if access.readable() => {
                object.read(offset, data)
            }
            Description::File { .. } => Err(Errno::BadFd),
            _ => Err(Errno::IllegalSeek),
        }
    }
    pub(in crate::posix) fn pwrite(&self, fd: usize, offset: u64, data: &[u8]) -> Result<usize> {
        match &mut *self.get(fd)?.borrow() {
            Description::File {
                object,
                access,
                dirty,
                ..
            } if access.writable() => {
                if !data.is_empty() {
                    object.write(offset, data)?;
                    *dirty = true;
                }
                Ok(data.len())
            }
            Description::File { .. } => Err(Errno::BadFd),
            _ => Err(Errno::IllegalSeek),
        }
    }
    pub(in crate::posix) fn resize_path(&self, path: &[u8], length: u64) -> Result<()> {
        let options = Open {
            access: Access::Write,
            ..Open::READ
        };
        let (file, _) = vfs::posix::open(
            self.cwd,
            self.directory,
            path,
            options,
            0,
            self.credentials(),
            false,
        )?;
        file.resize(length)?;
        file.sync()
    }
}

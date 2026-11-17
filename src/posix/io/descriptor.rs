//! Descriptor flags belong to a process slot. Status flags belong to the shared open description.
use super::*;
use crate::vfs::posix::{PosixDir, ROOT};
use core::sync::atomic::{AtomicU32, Ordering};

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
impl Shared {
    pub(super) fn new(description: Description) -> Self {
        let flags = match &description {
            Description::Input(_) => 0,
            Description::Output => 1,
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
    pub(in crate::posix) fn fcntl(&mut self, fd: usize, cmd: usize, arg: usize) -> Result<usize> {
        let h = self.get(fd)?.clone();
        match cmd {
            0 | 1030 => {
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
                if cmd == 1030 {
                    self.mark_cloexec(target);
                }
                Ok(target)
            }
            1 => Ok(((self.close_on_exec >> fd) & 1) as usize),
            2 => {
                self.close_on_exec = (self.close_on_exec & !(1 << fd)) | (((arg as u32) & 1) << fd);
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

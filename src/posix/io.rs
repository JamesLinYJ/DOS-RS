use super::{pipe, poll};
use crate::{
    kernel_cell::KernelCell,
    vfs::{self, Directories, File},
};
use alloc::{sync::Arc, vec, vec::Vec};
use core::{
    future::poll_fn,
    task::{Context, Poll},
};
use dosrs_native_posix::{Access, Errno, Fd, Open, Process, Result, Whence};

#[path = "io/terminal.rs"]
mod terminal;
pub(super) use terminal::{ReadClock, Terminal};

#[path = "io/descriptor.rs"]
mod descriptor;
#[path = "io/record.rs"]
pub(super) mod record;
pub(super) type Handle = Arc<descriptor::Shared>;
pub(super) enum Description {
    Input(Arc<KernelCell<Terminal>>),
    Output(Arc<KernelCell<Terminal>>),
    Pipe(pipe::End),
    File {
        object: File,
        access: Access,
        offset: u64,
        append: bool,
        dirty: bool,
    },
}

impl Description {
    fn sync(&mut self) -> Result<()> {
        if let Self::File { object, dirty, .. } = self
            && *dirty
        {
            object.sync()?;
            *dirty = false;
        }
        Ok(())
    }
    fn read(
        &mut self,
        cx: &Context,
        out: &mut [u8],
        timer: &mut ReadClock,
        nonblock: bool,
    ) -> Poll<Result<usize>> {
        match self {
            Self::Input(terminal) => terminal.borrow().read(cx, out, timer, nonblock),
            Self::Pipe(end) => end.read(cx, out),
            Self::File {
                object,
                access,
                offset,
                ..
            } if access.readable() => {
                let count = object.read(*offset, out)?;
                *offset += count as u64;
                Poll::Ready(Ok(count))
            }
            _ => Poll::Ready(Err(Errno::BadFd)),
        }
    }
    fn write(&mut self, cx: &Context, data: &[u8]) -> Poll<Result<usize>> {
        match self {
            Self::Output(tty) => tty.borrow().write(cx, data),
            Self::Pipe(end) => end.write(cx, data),
            Self::File {
                object,
                access,
                offset,
                dirty,
                append,
            } if access.writable() => {
                if !data.is_empty() {
                    // Filesystem operations run to completion on the boot CPU.
                    let pos = if *append { object.len()? } else { *offset };
                    object.write(pos, data)?;
                    *offset = pos + data.len() as u64;
                    *dirty = true;
                }
                Poll::Ready(Ok(data.len()))
            }
            _ => Poll::Ready(Err(Errno::BadFd)),
        }
    }
}
impl Drop for Description {
    fn drop(&mut self) {
        let _ = self.sync();
    }
}

pub(super) fn pipe_handles() -> (Handle, Handle) {
    let (r, w) = pipe::End::pair();
    (handle(Description::Pipe(r)), handle(Description::Pipe(w)))
}
fn handle(desc: Description) -> Handle {
    Arc::new(descriptor::Shared::new(desc))
}

pub(super) const OPEN_MAX: usize = 32;
static NEXT_PID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
pub(super) fn allocate_id() -> Result<u32> {
    NEXT_PID
        .try_update(
            core::sync::atomic::Ordering::Relaxed,
            core::sync::atomic::Ordering::Relaxed,
            |n| (n < i32::MAX as u32).then_some(n + 1),
        )
        .map_err(|_| Errno::TooManyFiles)
}
pub(super) struct ProcessContext {
    pid: u32,
    tty: Arc<KernelCell<Terminal>>,
    pub(super) children: alloc::sync::Weak<super::process::NewProcesses>,
    pub(super) wait_signal: u8,
    pub(super) caller_mask: u64,
    pub(super) uid: u32,
    pub(super) gid: u32,
    cwd: Directories,
    pub(super) directory: vfs::posix::PosixDir,
    cwd_pin: Option<Arc<KernelCell<File>>>,
    pub(super) mask: u32,
    close_on_exec: u32,
    close_on_fork: u32,
    io_budget: u8,
    fds: Vec<Option<Handle>>,
}
impl ProcessContext {
    pub fn terminal(
        input: Arc<KernelCell<Terminal>>,
        cwd: Directories,
        children: alloc::sync::Weak<super::process::NewProcesses>,
    ) -> Self {
        let output = handle(Description::Output(input.clone()));
        let directory = vfs::posix::initial(cwd);
        let cwd_pin = if directory == vfs::posix::ROOT {
            None
        } else {
            vfs::posix::pin_directory(directory)
                .ok()
                .map(|f| Arc::new(KernelCell::new(f)))
        };
        Self {
            pid: 0,
            tty: input.clone(),
            children,
            wait_signal: 0,
            caller_mask: 0,
            uid: 0,
            gid: 0,
            directory: if cwd_pin.is_some() {
                directory
            } else {
                vfs::posix::ROOT
            },
            cwd_pin,
            mask: 0o022,
            close_on_exec: 0,
            close_on_fork: 0,
            cwd,
            io_budget: IO_QUANTUM,
            fds: vec![
                Some(handle(Description::Input(input))),
                Some(output.clone()),
                Some(output),
            ],
        }
    }
    pub(super) fn tty_group(
        &self,
        fd: Fd,
        call: dosrs_native_exec::Call,
        arg: i32,
    ) -> core::result::Result<usize, isize> {
        let tty = self.get(fd).map_err(super::executable::errno)?;
        if !matches!(
            &*tty.borrow(),
            Description::Input(_) | Description::Output(_)
        ) {
            return Err(-25);
        }
        let mut terminal = self.tty.borrow();
        let sid = super::process::group(self.pid, 0, true)? as u32;
        if terminal.session != sid {
            return Err(-25);
        }
        match call {
            dosrs_native_exec::Call::Tcgetsid => Ok(sid as usize),
            dosrs_native_exec::Call::Tcgetpgrp => Ok(terminal.foreground as usize),
            _ if arg <= 0 => Err(-22),
            _ if !super::process::group_in_session(arg as u32, sid) => Err(-1),
            _ => {
                terminal.foreground = arg as u32;
                terminal.waiting.wake();
                Ok(0)
            }
        }
    }
    pub fn open_executable(&self, path: &[u8]) -> Result<File> {
        vfs::posix::open(
            self.cwd,
            self.directory,
            path,
            Open::READ,
            0,
            self.credentials(),
            true,
        )
        .map(|(file, _)| file)
    }
    pub fn inherit(&self) -> Result<Self> {
        let pid = allocate_id()?;
        let mut fds = Vec::new();
        fds.try_reserve_exact(self.fds.len())
            .map_err(|_| Errno::TooManyFiles)?;
        fds.extend(self.fds.iter().enumerate().map(|(fd, entry)| {
            if self.close_on_fork & (1 << fd) == 0 {
                entry.clone()
            } else {
                None
            }
        }));
        super::process::register(pid, self.pid, self.uid).map_err(|_| Errno::TooManyFiles)?;
        if self.pid == 0 {
            self.tty.borrow().attach(pid);
        }
        Ok(Self {
            pid,
            tty: self.tty.clone(),
            children: self.children.clone(),
            wait_signal: 0,
            caller_mask: 0,
            uid: self.uid,
            gid: self.gid,
            cwd: self.cwd,
            directory: self.directory,
            cwd_pin: self.cwd_pin.clone(),
            mask: self.mask,
            close_on_exec: self.close_on_exec & !self.close_on_fork,
            close_on_fork: 0,
            io_budget: IO_QUANTUM,
            fds,
        })
    }
    pub fn complete(&mut self, status: u8) -> u8 {
        let status = if self.finish().is_ok() { status } else { 1 };
        let wait = if self.wait_signal == 0 {
            i32::from(status) << 8
        } else {
            i32::from(self.wait_signal)
        };
        super::process::exited(self.pid, wait);
        self.pid = 0;
        status
    }
    pub(super) fn credentials(&self) -> vfs::posix::Credentials {
        vfs::posix::Credentials {
            uid: self.uid,
            gid: self.gid,
        }
    }
    pub(super) fn chdir(&mut self, from: vfs::posix::PosixDir, path: &[u8]) -> Result<()> {
        let next = vfs::posix::chdir(self.cwd, from, path, self.credentials())?;
        let pin = if next == vfs::posix::ROOT {
            None
        } else {
            Some(Arc::new(KernelCell::new(vfs::posix::pin_directory(next)?)))
        };
        self.cwd_pin = pin;
        self.directory = next;
        Ok(())
    }
    pub(super) fn open_mode(&mut self, path: &[u8], flags: Open, mode: u32) -> Result<Fd> {
        self.open_at(self.directory, path, flags, mode)
    }
    pub(super) fn open_at(
        &mut self,
        dir: vfs::posix::PosixDir,
        path: &[u8],
        flags: Open,
        mode: u32,
    ) -> Result<Fd> {
        let fd = self.available()?;
        let (object, dirty) = vfs::posix::open(
            self.cwd,
            dir,
            path,
            flags,
            mode & !self.mask,
            self.credentials(),
            false,
        )?;
        self.set(
            fd,
            handle(Description::File {
                object,
                offset: 0,
                append: flags.append,
                access: flags.access,
                dirty,
            }),
        );
        Ok(fd)
    }
    pub(super) fn close_exec_fds(&mut self) {
        let mut pending = core::mem::take(&mut self.close_on_exec);
        while pending != 0 {
            let fd = pending.trailing_zeros() as usize;
            pending &= pending - 1;
            let _ = self.close(fd);
        }
    }
    pub(super) fn unlink_path(&self, path: &[u8]) -> Result<()> {
        let name = vfs::mounts::qualify(self.directory, path);
        let path = name.as_ref();
        if vfs::posix::is_mount_point(self.directory, path) {
            return Err(Errno::Busy);
        }
        if vfs::posix::mount_namespace(path) && vfs::dos::mount_path(path).is_none() {
            return Err(Errno::NoEntry);
        }
        if vfs::posix::native(path) {
            return vfs::posix::unlink(self.directory, path, self.credentials());
        }
        vfs::unlink(
            self.cwd,
            core::str::from_utf8(path).map_err(|_| Errno::Invalid)?,
        )
    }
    pub(super) fn path_metadata(&self, path: &[u8], follow: bool) -> Result<vfs::posix::Metadata> {
        self.metadata_at(self.directory, path, follow)
    }
    pub(super) fn metadata_at(
        &self,
        directory: vfs::posix::PosixDir,
        path: &[u8],
        follow: bool,
    ) -> Result<vfs::posix::Metadata> {
        let qualified = vfs::mounts::qualify(directory, path);
        if vfs::posix::native(&qualified) || vfs::posix::mount_namespace(&qualified) {
            return vfs::posix::stat(directory, &qualified, self.credentials(), follow);
        }
        vfs::posix::open(
            self.cwd,
            directory,
            &qualified,
            Open::READ,
            0,
            self.credentials(),
            false,
        )?
        .0
        .metadata()
    }
    pub(super) fn metadata(&self, fd: Fd) -> Result<vfs::posix::Metadata> {
        let desc = self.get(fd)?.borrow();
        let mode = match &*desc {
            Description::File { object, .. } => return object.metadata(),
            Description::Pipe(_) => 0o010600,
            _ => 0o020666,
        };
        Ok(vfs::posix::Metadata {
            mode,
            nlink: 1,
            ..Default::default()
        })
    }
    pub(super) fn direntry(&self, fd: Fd, len: usize) -> Result<Vec<u8>> {
        let mut desc = self.get(fd)?.borrow();
        let Description::File { object, offset, .. } = &mut *desc else {
            return Err(Errno::NotDirectory);
        };
        let Some(e) = object.dirent(*offset)? else {
            return Ok(Vec::new());
        };
        if e.name.len() + 10 > len {
            return Err(Errno::Invalid);
        }
        let mut bytes = e.ino.to_le_bytes().to_vec();
        bytes.push(e.kind);
        bytes.extend(e.name);
        bytes.push(0);
        *offset += 1;
        Ok(bytes)
    }
    pub fn finish(&mut self) -> Result<()> {
        record::release(self.pid, None);
        let mut result = Ok(());
        for h in self.fds.drain(..).flatten() {
            if let Err(e) = h.borrow().sync() {
                result = Err(e);
            }
        }
        result
    }
    fn available(&self) -> Result<Fd> {
        let fd = self
            .fds
            .iter()
            .position(Option::is_none)
            .unwrap_or(self.fds.len());
        if fd >= OPEN_MAX {
            Err(Errno::TooManyFiles)
        } else {
            Ok(fd)
        }
    }
    pub fn set(&mut self, fd: Fd, value: Handle) {
        self.set_open_flags(fd, 0);
        if fd == self.fds.len() {
            self.fds.push(Some(value));
        } else {
            self.fds[fd] = Some(value);
        }
    }
    fn get(&self, fd: Fd) -> Result<&Handle> {
        self.fds
            .get(fd)
            .and_then(Option::as_ref)
            .ok_or(Errno::BadFd)
    }
    pub(super) fn readiness(
        &self,
        fd: Fd,
        events: i16,
        waker: Option<&Arc<core::task::Waker>>,
    ) -> core::result::Result<i16, isize> {
        let Ok(handle) = self.get(fd) else {
            return Ok(poll::INVALID);
        };
        match &*handle.borrow() {
            Description::Input(tty) => tty.borrow().readiness(events, false, waker),
            Description::Output(tty) => tty.borrow().readiness(events, true, waker),
            Description::Pipe(end) => end.readiness(events, waker),
            Description::File { .. } => Ok(events & (poll::READ | poll::WRITE)),
        }
    }
    pub(super) fn poll_read(
        &self,
        fd: Fd,
        cx: &Context,
        out: &mut [u8],
        timer: &mut ReadClock,
    ) -> Poll<Result<usize>> {
        let handle = self.get(fd)?;
        if matches!(&*handle.borrow(), Description::Input(_)) && !out.is_empty() {
            let mut tty = self.tty.borrow();
            let group = super::process::group(self.pid, 0, false).map_err(|_| Errno::Io)? as u32;
            let session = super::process::group(self.pid, 0, true).map_err(|_| Errno::Io)? as u32;
            if session == tty.session && group != tty.foreground {
                let sig = 21;
                if self.caller_mask & super::process::signals::bit(sig) != 0
                    || super::process::signals::disposition(self.pid, sig, false).handler == 1
                {
                    return Poll::Ready(Err(Errno::Io));
                }
                super::process::signals::terminal_input(group, sig);
                return tty.waiting.wait(cx);
            }
        }
        handle.blocking(handle.borrow().read(cx, out, timer, handle.nonblock()))
    }
    pub(super) fn poll_write(&self, fd: Fd, cx: &Context, bytes: &[u8]) -> Poll<Result<usize>> {
        let handle = self.get(fd)?;
        if let Description::Output(terminal) = &*handle.borrow() {
            let mut tty = terminal.borrow();
            if !bytes.is_empty()
                && tty.stop_background_output()
                && self.background_output(&tty).map_err(|_| Errno::Io)?
            {
                return tty.waiting.wait(cx);
            }
        }
        handle.blocking(handle.borrow().write(cx, bytes))
    }
    pub fn redirect(&mut self, target: Fd, path: &str, flags: Open) -> Result<()> {
        let fd = self.open(path, flags)?;
        if fd != target {
            self.dup2(fd, target)?;
            self.close(fd)?;
        }
        Ok(())
    }
}

impl Drop for ProcessContext {
    fn drop(&mut self) {
        if self.pid != 0 {
            record::release(self.pid, None);
            super::process::exited(self.pid, 1 << 8);
        }
    }
}

// Yield between successful transfers as well as on blocking I/O. A stream
// that stays ready cannot monopolize the boot processor or another terminal.
const IO_QUANTUM: u8 = 8;
async fn timeslice(budget: &mut u8) {
    if *budget != 0 {
        *budget -= 1;
        return;
    }
    *budget = IO_QUANTUM - 1;
    let mut first = true;
    poll_fn(|cx| {
        if core::mem::take(&mut first) {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await
}

impl Process for ProcessContext {
    fn lseek(&mut self, fd: Fd, delta: i64, whence: Whence) -> Result<u64> {
        let mut h = self.get(fd)?.borrow();
        let Description::File { object, offset, .. } = &mut *h else {
            return Err(Errno::IllegalSeek);
        };
        let base = match whence {
            Whence::Start => 0,
            Whence::Current => *offset,
            Whence::End => object.len()?,
        };
        let pos = base.checked_add_signed(delta).ok_or(Errno::Invalid)?;
        if pos > i64::MAX as u64 {
            return Err(Errno::FileTooLarge);
        }
        *offset = pos;
        Ok(pos)
    }
    fn getpid(&self) -> u32 {
        self.pid
    }
    fn open(&mut self, path: &str, flags: Open) -> Result<Fd> {
        self.open_mode(path.as_bytes(), flags, 0o666)
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        self.unlink_path(path.as_bytes())
    }
    fn close(&mut self, fd: Fd) -> Result<()> {
        let h = self
            .fds
            .get_mut(fd)
            .and_then(Option::take)
            .ok_or(Errno::BadFd)?;
        self.set_open_flags(fd, 0);
        if let Description::File { object, .. } = &*h.borrow() {
            record::release(self.pid, Some(object.identity()));
        }
        h.borrow().sync()
    }
    fn dup(&mut self, fd: Fd) -> Result<Fd> {
        let h = self.get(fd)?.clone();
        let new = self.available()?;
        self.set(new, h);
        Ok(new)
    }
    fn dup2(&mut self, fd: Fd, target: Fd) -> Result<Fd> {
        let h = self.get(fd)?.clone();
        if target == fd {
            return Ok(fd);
        }
        if target >= OPEN_MAX {
            return Err(Errno::BadFd);
        }
        if self.fds.len() < target {
            self.fds
                .try_reserve(target + 1 - self.fds.len())
                .map_err(|_| Errno::TooManyFiles)?;
            self.fds.resize_with(target, || None);
        }
        if self.get(target).is_ok() {
            self.close(target)?;
        }
        self.set(target, h);
        Ok(target)
    }
    fn pipe(&mut self) -> Result<(Fd, Fd)> {
        let first = self.available()?;
        let (r, w) = pipe_handles();
        self.set(first, r);
        match self.available() {
            Ok(second) => {
                self.set(second, w);
                Ok((first, second))
            }
            Err(e) => {
                self.fds[first] = None;
                Err(e)
            }
        }
    }
    async fn read(&mut self, fd: Fd, out: &mut [u8]) -> Result<usize> {
        timeslice(&mut self.io_budget).await;
        let mut timer = ReadClock::new(self.pid);
        poll_fn(|cx| self.poll_read(fd, cx, out, &mut timer)).await
    }
    async fn write(&mut self, fd: Fd, bytes: &[u8]) -> Result<usize> {
        timeslice(&mut self.io_budget).await;
        poll_fn(|cx| self.poll_write(fd, cx, bytes)).await
    }
}

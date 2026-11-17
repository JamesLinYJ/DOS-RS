use super::{Waiters, pipe};
use crate::{
    bios::pc,
    kernel_cell::KernelCell,
    vfs::{self, Directories, File},
};
use alloc::{collections::VecDeque, sync::Arc, vec, vec::Vec};
use core::{
    future::poll_fn,
    task::{Context, Poll},
};
use dosrs_native_posix::{Access, Errno, Fd, Open, Process, Result, Whence};

pub(super) struct Terminal {
    line: VecDeque<u8>,
    complete: bool,
    waiting: Waiters,
}
impl Terminal {
    pub fn new() -> Self {
        Self {
            line: VecDeque::new(),
            complete: false,
            waiting: Waiters::default(),
        }
    }
    pub fn notify_input(&mut self) {
        self.waiting.wake();
    }
    fn read(&mut self, cx: &Context, out: &mut [u8]) -> Poll<Result<usize>> {
        if out.is_empty() {
            return Poll::Ready(Ok(0));
        }
        while !self.complete {
            let Some(key) = pc::key(true) else {
                return self.waiting.wait(cx);
            };
            match key as u8 {
                4 => self.complete = true,
                b'\r' | b'\n' => {
                    self.line.push_back(b'\n');
                    console(b"\n");
                    self.complete = true;
                }
                8 if self.line.pop_back().is_some() => console(b"\x08 \x08"),
                ch @ 32..=126 if self.line.len() < 1023 => {
                    self.line.push_back(ch);
                    console(&[ch]);
                }
                _ => {}
            }
        }
        let count = self.line.len().min(out.len());
        for (dst, byte) in out.iter_mut().zip(self.line.drain(..count)) {
            *dst = byte;
        }
        self.complete = !self.line.is_empty();
        Poll::Ready(Ok(count))
    }
}
fn console(bytes: &[u8]) {
    for &b in bytes {
        if b == b'\n' {
            crate::putc(b'\r');
        }
        crate::putc(b);
    }
}

pub(super) type Handle = Arc<KernelCell<Description>>;
pub(super) enum Description {
    Input(Arc<KernelCell<Terminal>>),
    Output,
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
    fn read(&mut self, cx: &Context, out: &mut [u8]) -> Poll<Result<usize>> {
        match self {
            Self::Input(terminal) => terminal.borrow().read(cx, out),
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
            Self::Output => {
                console(data);
                Poll::Ready(Ok(data.len()))
            }
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
    Arc::new(KernelCell::new(desc))
}

const OPEN_MAX: usize = 32;
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
    pub(super) uid: u32,
    pub(super) gid: u32,
    cwd: Directories,
    io_budget: u8,
    fds: Vec<Option<Handle>>,
}
impl ProcessContext {
    pub fn terminal(input: Arc<KernelCell<Terminal>>, cwd: Directories) -> Self {
        let output = handle(Description::Output);
        Self {
            pid: 0,
            uid: 0,
            gid: 0,
            cwd,
            io_budget: IO_QUANTUM,
            fds: vec![
                Some(handle(Description::Input(input))),
                Some(output.clone()),
                Some(output),
            ],
        }
    }
    pub fn inherit(&self) -> Result<Self> {
        let pid = allocate_id()?;
        Ok(Self {
            pid,
            uid: self.uid,
            gid: self.gid,
            cwd: self.cwd,
            io_budget: IO_QUANTUM,
            fds: self.fds.clone(),
        })
    }
    pub fn finish(&mut self) -> Result<()> {
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
    pub(super) fn poll_read(&self, fd: Fd, cx: &Context, out: &mut [u8]) -> Poll<Result<usize>> {
        self.get(fd)?.borrow().read(cx, out)
    }
    pub(super) fn poll_write(&self, fd: Fd, cx: &Context, bytes: &[u8]) -> Poll<Result<usize>> {
        self.get(fd)?.borrow().write(cx, bytes)
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
        let fd = self.available()?;
        let (object, dirty) = vfs::open(self.cwd, path, flags)?;
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
    fn unlink(&mut self, path: &str) -> Result<()> {
        vfs::unlink(self.cwd, path)
    }
    fn close(&mut self, fd: Fd) -> Result<()> {
        let h = self
            .fds
            .get_mut(fd)
            .and_then(Option::take)
            .ok_or(Errno::BadFd)?;
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
        if target >= OPEN_MAX {
            return Err(Errno::BadFd);
        }
        if fd != target {
            if self.fds.len() <= target {
                self.fds.resize_with(target + 1, || None);
            }
            self.fds[target] = Some(h);
        }
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
        poll_fn(|cx| self.poll_read(fd, cx, out)).await
    }
    async fn write(&mut self, fd: Fd, bytes: &[u8]) -> Result<usize> {
        timeslice(&mut self.io_budget).await;
        poll_fn(|cx| self.poll_write(fd, cx, bytes)).await
    }
}

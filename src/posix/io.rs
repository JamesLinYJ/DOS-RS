use super::{Waiters, pipe};
use crate::{
    bios::pc,
    fs::{self as disk, FileError, OpenFile},
    kernel_cell::KernelCell,
};
use alloc::{collections::VecDeque, sync::Arc, vec, vec::Vec};
use core::{
    future::poll_fn,
    task::{Context, Poll},
};
use dosrs_native_posix::{Access, Errno, Fd, Open, Process, Result};

fn file_error(error: FileError) -> Errno {
    match error {
        FileError::Missing => Errno::NoEntry,
        FileError::Directory => Errno::IsDirectory,
        FileError::Access => Errno::Access,
        FileError::Limit => Errno::TooManyFiles,
        FileError::Space => Errno::NoSpace,
        FileError::Io => Errno::Io,
    }
}

// Reject names the DOS short-name parser would otherwise silently truncate.
fn path_name(path: &str) -> Result<(u32, [u8; 11])> {
    let tail = if path.as_bytes().get(1) == Some(&b':') {
        &path[2..]
    } else {
        path
    };
    let mut parts = tail.split(['/', '\\']).peekable();
    while let Some(part) = parts.next() {
        if part.is_empty() || matches!(part, "." | "..") {
            if parts.peek().is_none() {
                return Err(Errno::Invalid);
            }
            continue;
        }
        let (base, ext) = part.split_once('.').unwrap_or((part, ""));
        if base.len() > 8 || ext.len() > 3 {
            return Err(Errno::NameTooLong);
        }
        if base.is_empty()
            || part.ends_with('.')
            || ext.contains('.')
            || !part
                .bytes()
                .all(|b| b.is_ascii_graphic() && !b"\"*+,:;<=>?[\\]|".contains(&b))
        {
            return Err(Errno::Invalid);
        }
    }
    disk::path_name(path.as_bytes()).ok_or(Errno::NoEntry)
}

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
    pub fn keyboard(&mut self) {
        if !self.waiting.is_empty() && pc::key(false).is_some() {
            self.waiting.wake();
        }
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
        object: OpenFile,
        access: Access,
        offset: usize,
        dirty: bool,
    },
}

impl Description {
    fn sync(&mut self) -> Result<()> {
        if let Self::File { object, dirty, .. } = self
            && *dirty
        {
            if !object.sync() {
                return Err(Errno::Io);
            }
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
                let count = object.read(*offset, out).map_err(file_error)?;
                *offset += count;
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
            } if access.writable() => {
                if !data.is_empty() {
                    object.write(*offset, data).map_err(file_error)?;
                    *offset += data.len();
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
pub(super) struct ProcessContext {
    pid: u32,
    fds: Vec<Option<Handle>>,
}
impl ProcessContext {
    pub fn terminal(input: Arc<KernelCell<Terminal>>) -> Self {
        let output = handle(Description::Output);
        Self {
            pid: 0,
            fds: vec![
                Some(handle(Description::Input(input))),
                Some(output.clone()),
                Some(output),
            ],
        }
    }
    pub fn inherit(&self) -> Result<Self> {
        let pid = NEXT_PID
            .try_update(
                core::sync::atomic::Ordering::Relaxed,
                core::sync::atomic::Ordering::Relaxed,
                |n| n.checked_add(1),
            )
            .map_err(|_| Errno::TooManyFiles)?;
        Ok(Self {
            pid,
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
async fn timeslice() {
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
    fn getpid(&self) -> u32 {
        self.pid
    }
    fn open(&mut self, path: &str, flags: Open) -> Result<Fd> {
        let fd = self.available()?;
        let (dir, name) = path_name(path)?;
        if flags.truncate && !flags.access.writable() {
            return Err(Errno::Invalid);
        }
        if flags.create && flags.exclusive && disk::exists_at(dir, &name) {
            return Err(Errno::Exists);
        }
        let mut created = false;
        let object = match OpenFile::open(dir, &name, flags.access.writable()) {
            Ok(object) => object,
            Err(FileError::Missing) if flags.create => {
                if !disk::write_named_at(dir, &name, &[], 0x20) {
                    return Err(Errno::NoSpace);
                }
                match OpenFile::open(dir, &name, flags.access.writable()) {
                    Ok(object) => {
                        created = true;
                        object
                    }
                    Err(err) => {
                        disk::in_dir(dir, || disk::remove_file(&name));
                        return Err(file_error(err));
                    }
                }
            }
            Err(e) => return Err(file_error(e)),
        };
        if flags.truncate && !created {
            object.write(0, &[]).map_err(file_error)?;
        }
        self.set(
            fd,
            handle(Description::File {
                object,
                offset: 0,
                access: flags.access,
                dirty: created || flags.truncate,
            }),
        );
        Ok(fd)
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        let (dir, name) = path_name(path)?;
        let attr = disk::in_dir(dir, || disk::attribute(&name)).ok_or(Errno::NoEntry)?;
        if attr & 0x10 != 0 {
            return Err(Errno::IsDirectory);
        }
        if attr & 1 != 0 {
            return Err(Errno::Access);
        }
        if disk::in_dir(dir, || disk::remove_file(&name)) && disk::flush(dir) {
            Ok(())
        } else {
            Err(Errno::Io)
        }
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
        let h = self.get(fd)?;
        timeslice().await;
        poll_fn(|cx| h.borrow().read(cx, out)).await
    }
    async fn write(&mut self, fd: Fd, bytes: &[u8]) -> Result<usize> {
        let h = self.get(fd)?;
        timeslice().await;
        poll_fn(|cx| h.borrow().write(cx, bytes)).await
    }
}

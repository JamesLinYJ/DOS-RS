//! A bounded byte stream. Each end has one shared open description, so dup
//! and inheritance delay EOF/EPIPE until the final reference to that end goes.
use super::Waiters;
use crate::kernel_cell::KernelCell;
use alloc::{collections::VecDeque, sync::Arc};
use core::task::{Context, Poll};
use dosrs_native_posix::{Errno, Result};

pub(super) const CAPACITY: usize = 512;
pub(super) struct Pipe {
    data: VecDeque<u8>,
    reader: bool,
    writer: bool,
    reads: Waiters,
    writes: Waiters,
}

pub(super) struct End {
    pub read_end: bool,
    pipe: Arc<KernelCell<Pipe>>,
}
impl End {
    pub fn pair() -> (Self, Self) {
        let pipe = Arc::new(KernelCell::new(Pipe {
            data: VecDeque::with_capacity(CAPACITY),
            reader: true,
            writer: true,
            reads: Waiters::default(),
            writes: Waiters::default(),
        }));
        (
            Self {
                read_end: true,
                pipe: pipe.clone(),
            },
            Self {
                read_end: false,
                pipe,
            },
        )
    }

    pub fn read(&self, cx: &Context, out: &mut [u8]) -> Poll<Result<usize>> {
        let mut pipe = self.pipe.borrow();
        if !self.read_end {
            return Poll::Ready(Err(Errno::BadFd));
        }
        if out.is_empty() || pipe.data.is_empty() && !pipe.writer {
            return Poll::Ready(Ok(0));
        }
        if pipe.data.is_empty() {
            return pipe.reads.wait(cx);
        }
        let count = out.len().min(pipe.data.len());
        for (dst, byte) in out.iter_mut().zip(pipe.data.drain(..count)) {
            *dst = byte;
        }
        pipe.writes.wake();
        Poll::Ready(Ok(count))
    }

    pub fn write(&self, cx: &Context, data: &[u8]) -> Poll<Result<usize>> {
        let mut pipe = self.pipe.borrow();
        if self.read_end {
            return Poll::Ready(Err(Errno::BadFd));
        }
        if data.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if !pipe.reader {
            return Poll::Ready(Err(Errno::BrokenPipe));
        }
        let space = CAPACITY - pipe.data.len();
        if space == 0 || data.len() <= CAPACITY && space < data.len() {
            return pipe.writes.wait(cx);
        }
        let count = data.len().min(space);
        pipe.data.extend(&data[..count]);
        pipe.reads.wake();
        Poll::Ready(Ok(count))
    }
}
impl Drop for End {
    fn drop(&mut self) {
        let mut pipe = self.pipe.borrow();
        if self.read_end {
            pipe.reader = false;
            pipe.writes.wake();
        } else {
            pipe.writer = false;
            pipe.reads.wake();
        }
    }
}

//! A bounded byte stream. Each end has one shared open description, so dup
//! and inheritance delay EOF/EPIPE until the final reference to that end goes.
use super::{Waiters, poll};
use crate::kernel_cell::KernelCell;
use alloc::{collections::VecDeque, sync::Arc};
use core::task::{Context, Poll};
use dosrs_native_posix::{Errno, Result};

// musl advertises PIPE_BUF = 4096. A single native write of this size is atomic.
pub(super) const CAPACITY: usize = 4096;
pub(super) struct Pipe {
    data: VecDeque<u8>,
    reader: bool,
    writer: bool,
    reads: Waiters,
    writes: Waiters,
    pollers: poll::Subscribers,
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
            pollers: poll::Subscribers::default(),
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

    pub fn readiness(
        &self,
        events: i16,
        waker: Option<&Arc<core::task::Waker>>,
    ) -> core::result::Result<i16, isize> {
        let mut pipe = self.pipe.borrow();
        let mut result = 0;
        if self.read_end {
            if !pipe.data.is_empty() {
                result |= events & poll::READ;
            }
            if !pipe.writer {
                result |= poll::HANGUP;
            }
        } else {
            if pipe.data.len() < CAPACITY {
                result |= events & poll::WRITE;
            }
            if !pipe.reader {
                result |= poll::ERROR;
            }
        }
        if result == 0 {
            pipe.pollers.subscribe(waker)?;
        }
        Ok(result)
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
        let (head, tail) = pipe.data.as_slices();
        let split = count.min(head.len());
        out[..split].copy_from_slice(&head[..split]);
        out[split..count].copy_from_slice(&tail[..count - split]);
        pipe.data.drain(..count);
        pipe.pollers.wake();
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
        pipe.pollers.wake();
        pipe.reads.wake();
        Poll::Ready(Ok(count))
    }
}
impl Drop for End {
    fn drop(&mut self) {
        let mut pipe = self.pipe.borrow();
        pipe.pollers.wake();
        if self.read_end {
            pipe.reader = false;
            pipe.writes.wake();
        } else {
            pipe.writer = false;
            pipe.reads.wake();
        }
    }
}

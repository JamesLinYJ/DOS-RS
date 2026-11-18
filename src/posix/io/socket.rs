//! Connected, anonymous AF_UNIX sockets. Clones of an open description share the
//! same endpoint. Each direction owns its receive buffer and backpressure state.
use super::super::executable::errno;
use super::super::{Waiters, poll};
use super::{Description, ProcessContext, descriptor, handle};
use crate::arch::user::AddressSpace;
use crate::kernel_cell::KernelCell;
use alloc::{collections::VecDeque, sync::Arc};
use core::task::{Context, Poll, Waker};
use dosrs_native_exec::Call;
use dosrs_native_posix::{Errno, Result};

pub(in crate::posix) const CAPACITY: usize = 4096;
pub(super) const PEEK: usize = 2;
pub(super) const TRUNC: usize = 0x20;
pub(super) const DONTWAIT: usize = 0x40;
pub(in crate::posix) const NOSIGNAL: usize = 0x4000;

#[derive(Default)]
struct Queue {
    bytes: VecDeque<u8>,
    // Zero-length records also occupy a slot. Streams do not use this queue.
    records: VecDeque<usize>,
    closed: bool,
    read: Waiters,
    write: Waiters,
}

struct Pair {
    incoming: [Queue; 2],
    pollers: poll::Subscribers,
}

pub(in crate::posix) struct End {
    side: usize,
    pub(super) records: bool,
    pair: Arc<KernelCell<Pair>>,
}

impl End {
    pub(super) fn pair(records: bool) -> (Self, Self) {
        let pair = Arc::new(KernelCell::new(Pair {
            incoming: Default::default(),
            pollers: Default::default(),
        }));
        (
            Self {
                side: 0,
                records,
                pair: pair.clone(),
            },
            Self {
                side: 1,
                records,
                pair,
            },
        )
    }

    pub(super) fn readiness(
        &self,
        events: i16,
        waker: Option<&Arc<Waker>>,
    ) -> core::result::Result<i16, isize> {
        let mut p = self.pair.borrow();
        let own = &p.incoming[self.side];
        let peer = &p.incoming[1 - self.side];
        let mut result = 0;
        if !own.bytes.is_empty() || !own.records.is_empty() || peer.closed {
            result |= events & poll::READ;
        }
        if peer.closed || peer.bytes.len() < CAPACITY && peer.records.len() < CAPACITY {
            result |= events & poll::WRITE;
        }
        if peer.closed {
            result |= poll::HANGUP;
        }
        if result == 0 {
            p.pollers.subscribe(waker)?;
        }
        Ok(result)
    }

    pub(super) fn send(&self, cx: &Context, data: &[u8], nonblock: bool) -> Poll<Result<usize>> {
        let mut p = self.pair.borrow();
        let incoming = &mut p.incoming[1 - self.side];
        if incoming.closed {
            return Poll::Ready(Err(Errno::BrokenPipe));
        }
        if data.is_empty() && !self.records {
            return Poll::Ready(Ok(0));
        }
        if self.records && data.len() > CAPACITY {
            return Poll::Ready(Err(Errno::MessageSize));
        }
        let room = CAPACITY - incoming.bytes.len();
        if room == 0 && !data.is_empty()
            || self.records && (room < data.len() || incoming.records.len() == CAPACITY)
        {
            return if nonblock {
                Poll::Ready(Err(Errno::Again))
            } else {
                incoming.write.wait(cx)
            };
        }
        let count = data.len().min(room);
        incoming
            .bytes
            .try_reserve(count)
            .map_err(|_| Errno::NoMemory)?;
        if self.records {
            incoming
                .records
                .try_reserve(1)
                .map_err(|_| Errno::NoMemory)?;
            incoming.records.push_back(count);
        }
        incoming.bytes.extend(&data[..count]);
        incoming.read.wake();
        p.pollers.wake();
        Poll::Ready(Ok(count))
    }

    pub(super) fn recv(
        &self,
        cx: &Context,
        out: &mut [u8],
        flags: usize,
        nonblock: bool,
    ) -> Poll<Result<usize>> {
        let mut p = self.pair.borrow();
        let closed = p.incoming[1 - self.side].closed;
        let incoming = &mut p.incoming[self.side];
        if !self.records && out.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let length = if self.records {
            incoming.records.front().copied()
        } else {
            (!incoming.bytes.is_empty()).then_some(incoming.bytes.len())
        };
        let Some(length) = length else {
            return if closed {
                Poll::Ready(Ok(0))
            } else if nonblock {
                Poll::Ready(Err(Errno::Again))
            } else {
                incoming.read.wait(cx)
            };
        };
        let copied = length.min(out.len());
        let (first, second) = incoming.bytes.as_slices();
        let split = first.len().min(copied);
        out[..split].copy_from_slice(&first[..split]);
        out[split..copied].copy_from_slice(&second[..copied - split]);
        if flags & PEEK == 0 {
            let consume = if self.records {
                incoming.records.pop_front();
                length
            } else {
                copied
            };
            incoming.bytes.drain(..consume);
            incoming.write.wake();
            p.pollers.wake();
        }
        Poll::Ready(Ok(if self.records && flags & TRUNC != 0 {
            length
        } else {
            copied
        }))
    }
}

impl Drop for End {
    fn drop(&mut self) {
        let mut p = self.pair.borrow();
        let own = &mut p.incoming[self.side];
        own.closed = true;
        own.bytes.clear();
        own.records.clear();
        own.write.wake();
        p.incoming[1 - self.side].read.wake();
        p.pollers.wake();
    }
}

impl ProcessContext {
    pub(in crate::posix) fn check_record_size(
        &self,
        fd: usize,
        length: usize,
    ) -> core::result::Result<(), isize> {
        if length > CAPACITY
            && matches!(&*self.get(fd).map_err(errno)?.borrow(), Description::Socket(s) if s.records)
        {
            Err(-90)
        } else {
            Ok(())
        }
    }
}

pub(in crate::posix) fn socketpair(
    process: &mut ProcessContext,
    image: &mut AddressSpace,
    [domain, kind, options]: [usize; 3],
) -> core::result::Result<usize, isize> {
    let [protocol, output] = super::super::paths::words(image, options)?;
    if domain != 1 {
        return Err(-97);
    }
    if protocol != 0 {
        return Err(-93);
    }
    let flags = kind & (descriptor::CLOEXEC | descriptor::CLOFORK | descriptor::NONBLOCK as usize);
    let records = match kind & !flags {
        1 => false,
        5 => true,
        _ => return Err(-94),
    };
    image.check(output, 8, true).map_err(|_| -14isize)?;
    let (left, right) = End::pair(records);
    let (left, right) = process
        .set_pair((
            handle(Description::Socket(left)),
            handle(Description::Socket(right)),
        ))
        .map_err(errno)?;
    for fd in [left, right] {
        process.set_open_flags(fd, flags);
        if flags & descriptor::NONBLOCK as usize != 0 {
            process.fcntl(fd, 4, flags).map_err(errno)?;
        }
    }
    let mut pair = [0; 8];
    pair[..4].copy_from_slice(&(left as i32).to_le_bytes());
    pair[4..].copy_from_slice(&(right as i32).to_le_bytes());
    image.write(output, &pair).map_err(|_| -14isize)?;
    Ok(0)
}

pub(in crate::posix) fn transfer(
    process: &mut ProcessContext,
    image: &mut AddressSpace,
    call: Call,
    [fd, address, options]: [usize; 3],
    cx: &Context,
    sigpipe: &mut bool,
) -> Poll<core::result::Result<usize, isize>> {
    let [size, flags] = super::super::paths::words(image, options)?;
    let open = process.get(fd).map_err(errno)?;
    let description = open.borrow();
    let Description::Socket(end) = &*description else {
        return Poll::Ready(Err(-88));
    };
    let send = call == Call::Send;
    let allowed = DONTWAIT
        | if send {
            NOSIGNAL | 0x80 /* MSG_EOR */
        } else {
            PEEK | TRUNC
        };
    if flags & !allowed != 0 {
        return Poll::Ready(Err(-95));
    }
    if send && end.records && size > CAPACITY {
        return Poll::Ready(Err(-90));
    }
    let length = size.min(CAPACITY);
    let mut bytes = alloc::vec::Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| -12isize)?;
    bytes.resize(length, 0);
    let nonblock = open.nonblock() || flags & DONTWAIT != 0;
    if send {
        *sigpipe = flags & NOSIGNAL == 0;
        image.read(address, &mut bytes).map_err(|_| -14isize)?;
        end.send(cx, &bytes, nonblock).map(|r| r.map_err(errno))
    } else {
        image.check(address, length, true).map_err(|_| -14isize)?;
        let count = core::task::ready!(end.recv(cx, &mut bytes, flags, nonblock)).map_err(errno)?;
        image
            .write(address, &bytes[..count.min(length)])
            .map_err(|_| -14isize)?;
        Poll::Ready(Ok(count))
    }
}

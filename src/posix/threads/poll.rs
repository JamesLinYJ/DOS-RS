//! pollfd is eight bytes on both ABIs. ppoll's final argument points to a
//! pair of native pointers (relative 64-bit timespec and optional signal mask).
use super::{Member, clock};
use crate::arch::user::AddressSpace;
use crate::posix::{
    io::{OPEN_MAX, ProcessContext},
    process::signals,
};
use alloc::{sync::Arc, vec::Vec};
use core::task::{Context, Poll, Waker};
use dosrs_native_exec::Call;

struct Entry {
    fd: i32,
    events: i16,
    revents: i16,
}

pub(super) struct Request {
    address: usize,
    fds: Vec<Entry>,
    waker: Arc<Waker>,
    previous_mask: Option<u64>,
    immediate: bool,
}

impl Request {
    fn start(
        thread: &mut Member,
        image: &AddressSpace,
        call: Call,
        [address, count, argument]: [usize; 3],
        cx: &Context,
    ) -> Result<Self, isize> {
        if count > OPEN_MAX {
            return Err(-22);
        }
        if count != 0 {
            image
                .check(address, count * 8, true)
                .map_err(|_| -14isize)?;
        }
        let mut fds = Vec::new();
        fds.try_reserve_exact(count).map_err(|_| -12isize)?;
        for i in 0..count {
            let mut bytes = [0; 8];
            image
                .read(address + i * 8, &mut bytes)
                .map_err(|_| -14isize)?;
            fds.push(Entry {
                fd: i32::from_le_bytes(bytes[..4].try_into().unwrap()),
                events: i16::from_le_bytes(bytes[4..6].try_into().unwrap()),
                revents: 0,
            });
        }
        let (duration, mask) = if call == Call::Poll {
            let millis = argument as i32;
            ((millis >= 0).then(|| millis as u64 * 1_000_000), None)
        } else {
            let mut words = [0u8; 2 * size_of::<usize>()];
            image.read(argument, &mut words).map_err(|_| -14isize)?;
            let (time, mask) = words.split_at(size_of::<usize>());
            let time = usize::from_le_bytes(time.try_into().unwrap());
            let mask = usize::from_le_bytes(mask.try_into().unwrap());
            let duration = (time != 0)
                .then(|| clock::read_ns(image, time))
                .transpose()?;
            let mask = (mask != 0)
                .then(|| {
                    let mut raw = [0; 8];
                    image.read(mask, &mut raw).map_err(|_| -14isize)?;
                    Ok::<_, isize>(u64::from_le_bytes(raw) & !(signals::bit(9) | signals::bit(19)))
                })
                .transpose()?;
            (duration, mask)
        };
        if let Some(ns) = duration.filter(|&ns| ns != 0) {
            thread.timer = Some(clock::Timer::after(thread.tid, ns, cx)?);
        }
        let previous_mask = mask.map(|mask| {
            core::mem::replace(&mut thread.registers.as_mut().unwrap().signal_mask, mask)
        });
        Ok(Self {
            address,
            fds,
            waker: Arc::new(cx.waker().clone()),
            previous_mask,
            immediate: duration == Some(0),
        })
    }

    pub(super) fn restore(self, thread: &mut Member) {
        if let Some(mask) = self.previous_mask {
            thread.registers.as_mut().unwrap().signal_mask = mask;
        }
    }
}

pub(super) fn call(
    thread: &mut Member,
    image: &mut AddressSpace,
    process: &ProcessContext,
    call: Call,
    args: [usize; 3],
    cx: &Context,
) -> Poll<Result<usize, isize>> {
    if thread.poll_request.is_none() {
        thread.poll_request = Some(Request::start(thread, image, call, args, cx)?);
    }
    let request = thread.poll_request.as_mut().unwrap();
    let expired = request.immediate || thread.timer.as_ref().is_some_and(clock::Timer::expired);
    let subscription = (!expired).then_some(&request.waker);
    let mut ready = 0;
    for entry in &mut request.fds {
        entry.revents = if entry.fd < 0 {
            0
        } else {
            process.readiness(entry.fd as usize, entry.events, subscription)?
        };
        ready += usize::from(entry.revents != 0);
    }
    // Deliver a newly unmasked pending signal while the temporary mask is
    // installed even when the caller supplies a zero-length timeout.
    if ready == 0 && request.previous_mask.is_some() {
        use dosrs_native_posix::Process;
        let mask = thread.registers.as_ref().unwrap().signal_mask;
        if (signals::pending(process.getpid()) | thread.pending_signals.mask()) & !mask != 0 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
    }
    if ready == 0 && !expired {
        return Poll::Pending;
    }
    for (i, entry) in request.fds.iter().enumerate() {
        image
            .write(request.address + i * 8 + 6, &entry.revents.to_le_bytes())
            .map_err(|_| -14isize)?;
    }
    Poll::Ready(Ok(ready))
}

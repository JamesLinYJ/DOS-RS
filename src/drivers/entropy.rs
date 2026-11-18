//! Sources of genuine random bytes. The BSP owns the PCI queue and entropy buffer.
mod transport;
use crate::{
    arch::{io_memory::Dma, platform},
    kernel_cell::KernelCell,
    platform::pci::Device,
};
use alloc::vec::Vec;
use core::{
    sync::atomic::{AtomicBool, Ordering, fence},
    task::{Context, Poll, Waker},
};

const BUFFER: usize = 256;
static QUEUE: KernelCell<Option<Queue>> = KernelCell::new(None);
static WAITING: AtomicBool = AtomicBool::new(false);

struct Queue {
    transport: transport::Transport,
    _desc: Dma,
    avail: Dma,
    used: Dma,
    bytes: Dma,
    sequence: u16,
    in_flight: bool,
    waiting: Vec<Waker>,
    cache: [u8; BUFFER * 2],
    start: usize,
    end: usize,
    failed: bool,
}
impl Queue {
    fn submit(&mut self) {
        self.in_flight = true;
        self.sequence = self.sequence.wrapping_add(1);
        fence(Ordering::Release);
        // Queue size 1, descriptor and avail.ring[0] always refer to slot zero.
        // VRING_AVAIL_F_NO_INTERRUPT: the kernel polls completions on the BSP.
        self.avail.write(0, (u32::from(self.sequence) << 16) | 1);
        fence(Ordering::SeqCst);
        self.transport.notify();
    }
    fn complete(&mut self) -> bool {
        if !self.in_flight || self.used.read(0) >> 16 != u32::from(self.sequence) {
            return false;
        }
        fence(Ordering::Acquire);
        self.in_flight = false;
        let length = self.used.read(2) as usize;
        if self.used.read(1) != 0 || length == 0 || length > BUFFER {
            self.failed = true;
        } else {
            self.bytes
                .receive(&mut self.cache[self.end..self.end + length]);
            self.end += length;
        }
        true
    }
    fn refill(&mut self) {
        let remaining = self.end - self.start;
        if !self.in_flight && !self.failed && remaining < BUFFER {
            self.cache.copy_within(self.start..self.end, 0);
            self.cache[remaining..].fill(0);
            self.start = 0;
            self.end = remaining;
            self.submit();
        }
    }
    fn read(
        &mut self,
        bytes: &mut [u8],
        nonblock: bool,
        cx: &Context,
    ) -> Poll<Result<usize, isize>> {
        self.complete();
        if self.failed {
            return Poll::Ready(Err(-5));
        }
        if self.end - self.start < bytes.len() {
            self.refill();
            if nonblock {
                return Poll::Ready(Err(-11));
            }
            if !self.waiting.iter().any(|w| w.will_wake(cx.waker())) {
                self.waiting.try_reserve(1).map_err(|_| -12isize)?;
                self.waiting.push(cx.waker().clone());
            }
            WAITING.store(true, Ordering::Release);
            return Poll::Pending;
        }
        let count = bytes.len();
        let source = &mut self.cache[self.start..self.start + count];
        bytes[..count].copy_from_slice(source);
        source.fill(0);
        self.start += count;
        self.refill();
        Poll::Ready(Ok(count))
    }
}

pub(super) fn matches(d: Device) -> bool {
    d.vendor == 0x1af4 && matches!(d.id, 0x1005 | 0x1044)
}

pub(super) fn attach(d: Device) -> Result<(), &'static str> {
    let mut active = QUEUE.borrow();
    if active.is_some() {
        return Ok(());
    }
    let mut transport = transport::Transport::new(d)
        .ok_or("VirtIO RNG needs a modern PCI configuration access capability")?;
    let page = || Dma::new().ok_or("VirtIO RNG DMA pages unavailable");
    let (mut desc, avail, used, bytes) = (page()?, page()?, page()?, page()?);
    desc.write(0, bytes.addr());
    desc.write(1, 0);
    desc.write(2, BUFFER as u32);
    desc.write(3, 2); // VIRTQ_DESC_F_WRITE
    transport
        .initialize(desc.addr(), avail.addr(), used.addr())
        .ok_or("VirtIO RNG queue negotiation failed")?;
    let mut queue = Queue {
        transport,
        _desc: desc,
        avail,
        used,
        bytes,
        sequence: 0,
        in_flight: false,
        waiting: Vec::new(),
        cache: [0; BUFFER * 2],
        start: 0,
        end: 0,
        failed: false,
    };
    queue.submit();
    *active = Some(queue);
    crate::puts("Entropy source: VirtIO RNG\n");
    Ok(())
}

pub(crate) fn tick() {
    if !WAITING.load(Ordering::Acquire) {
        return;
    }
    if let Some(q) = QUEUE.borrow().as_mut()
        && q.complete()
    {
        WAITING.store(false, Ordering::Release);
        for w in q.waiting.drain(..) {
            w.wake();
        }
    }
}

pub(crate) fn fill(bytes: &mut [u8], nonblock: bool, cx: &Context) -> Poll<Result<usize, isize>> {
    if bytes.is_empty() {
        return Poll::Ready(Ok(0));
    }
    if let Some(queue) = QUEUE.borrow().as_mut() {
        return queue.read(bytes, nonblock, cx);
    }
    match platform::hardware_random(bytes) {
        Some(n) if n == bytes.len() => Poll::Ready(Ok(n)),
        Some(_) if !nonblock => {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
        Some(_) => Poll::Ready(Err(-11)),
        None => Poll::Ready(Err(-19)),
    }
}

//! Intel 82540EM polled descriptor rings (legacy descriptor format).
use super::Link;
use crate::{
    arch::io_memory::{Dma, Registers},
    platform::pci::Device,
};
use core::sync::atomic::{Ordering, fence};
const N: usize = 8;
pub(super) struct Nic {
    r: Registers,
    rx: Dma,
    tx: Dma,
    buffers: [Dma; N],
    send: Dma,
    head: usize,
    tail: usize,
    mac: [u8; 6],
    failed: bool,
}
fn page() -> Result<Dma, &'static str> {
    Dma::new().ok_or("Ethernet DMA exhausted")
}
impl Nic {
    pub fn new(dev: Device) -> Result<Self, &'static str> {
        let r = dev
            .mmio()
            .filter(|r| r.fits(0x5400, 8))
            .ok_or("invalid 82540EM BAR")?;
        dev.enable(6);
        r.write(0xd8, u32::MAX);
        r.write(0x100, 0);
        r.write(0x400, 0);
        let mut mac = [0; 6];
        mac[..4].copy_from_slice(&r.read(0x5400).to_le_bytes());
        mac[4..].copy_from_slice(&r.read(0x5404).to_le_bytes()[..2]);
        if mac == [0; 6] || mac[0] & 1 != 0 {
            return Err("invalid station address");
        }
        let mut rx = page()?;
        let tx = page()?;
        let send = page()?;
        let mut buffers: [Option<Dma>; N] = core::array::from_fn(|_| None);
        for (i, p) in buffers.iter_mut().enumerate() {
            let buffer = page()?;
            rx.write(i * 4, buffer.addr());
            *p = Some(buffer);
        }
        for (base, list) in [(0x2800, &rx), (0x3800, &tx)] {
            r.write(base, list.addr());
            r.write(base + 4, 0);
            r.write(base + 8, (N * 16) as u32);
            r.write(base + 16, 0);
            r.write(base + 24, 0);
        }
        r.write(0, r.read(0) | (1 << 6));
        r.write(0x2818, N as u32 - 1);
        r.write(0x410, 0x0060200a);
        r.write(0x400, 2 | 8 | (15 << 4) | (64 << 12));
        r.write(0x100, 2 | (1 << 15) | (1 << 26));
        Ok(Self {
            r,
            rx,
            tx,
            send,
            buffers: buffers.map(Option::unwrap),
            head: 0,
            tail: 0,
            mac,
            failed: false,
        })
    }
}
impl Link for Nic {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }
    fn send(&mut self, frame: &[u8]) -> bool {
        if self.failed || frame.len() > 1518 {
            return false;
        }
        self.send.send(frame);
        let n = self.tail * 4;
        self.tx.write(n, self.send.addr());
        self.tx.write(n + 2, frame.len() as u32 | (0x0b << 24));
        self.tx.write(n + 3, 0);
        fence(Ordering::SeqCst);
        self.tail = (self.tail + 1) % N;
        self.r.write(0x3818, self.tail as u32);
        if !(0..500_000).any(|_| self.tx.read(n + 3) & 1 != 0) {
            self.failed = true;
        }
        !self.failed
    }
    fn receive(&mut self, out: &mut [u8]) -> Option<usize> {
        let n = self.head * 4;
        let status = self.rx.read(n + 3);
        if status & 1 == 0 {
            return None;
        }
        fence(Ordering::Acquire);
        let len = (self.rx.read(n + 2) & 65535) as usize;
        let valid = status & 0xff03 == 3 && len <= out.len();
        if valid {
            self.buffers[self.head].receive(&mut out[..len]);
        }
        self.rx.write(n + 3, 0);
        fence(Ordering::Release);
        self.r.write(0x2818, self.head as u32);
        self.head = (self.head + 1) % N;
        valid.then_some(len)
    }
}

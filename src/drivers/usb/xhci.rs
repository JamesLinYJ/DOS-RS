//! Polled xHCI transport. A slot owns its contexts, transfer rings and DMA buffer.
use super::{Endpoint, Error, Setup, UsbIo};
use crate::arch::io_memory::{Dma, Registers};
use core::sync::atomic::{Ordering, fence};
type Result<T> = core::result::Result<T, Error>;
const N: usize = 64;
const SLOTS: usize = 8;
const PIPES: usize = 8;
const TIMEOUT: usize = 500_000;
fn page() -> Result<Dma> {
    Dma::new().ok_or(Error("DMA pages exhausted"))
}
fn wait(mut ready: impl FnMut() -> bool) -> Result<()> {
    for _ in 0..TIMEOUT {
        if ready() {
            return Ok(());
        }
        core::hint::spin_loop()
    }
    Err(Error("controller timeout"))
}
struct Ring {
    mem: Dma,
    next: usize,
    cycle: u32,
}
impl Ring {
    fn new() -> Result<Self> {
        Ok(Self {
            mem: page()?,
            next: 0,
            cycle: 1,
        })
    }
    fn put(&mut self, arg: u64, status: u32, flags: u32) -> u32 {
        let word = self.next * 4;
        let addr = self.mem.addr() + self.next as u32 * 16;
        self.mem.write(word, arg as u32);
        self.mem.write(word + 1, (arg >> 32) as u32);
        self.mem.write(word + 2, status);
        fence(Ordering::Release);
        self.mem.write(word + 3, flags | self.cycle);
        self.next += 1;
        if self.next == N - 1 {
            self.mem.write(4 * self.next, self.mem.addr());
            fence(Ordering::Release);
            self.mem
                .write(4 * self.next + 3, (6 << 10) | 2 | self.cycle);
            self.next = 0;
            self.cycle ^= 1;
        }
        addr
    }
}
struct Pipe {
    id: u8,
    ring: Ring,
}
struct Slot {
    offline: bool,
    output: Dma,
    input: Dma,
    buffer: Dma,
    pipes: [Option<Pipe>; PIPES],
    speed: u32,
    port: u8,
    highest: u8,
}
impl Slot {
    fn new(port: u8, speed: u32) -> Result<Self> {
        Ok(Self {
            offline: false,
            output: page()?,
            input: page()?,
            buffer: page()?,
            pipes: core::array::from_fn(|_| None),
            speed,
            port,
            highest: 1,
        })
    }
    fn pipe(&mut self, id: u8) -> Result<&mut Pipe> {
        self.pipes
            .iter_mut()
            .flatten()
            .find(|p| p.id == id)
            .ok_or(Error("endpoint unavailable"))
    }
    fn context(&mut self, words: usize, ep: Endpoint) -> Result<()> {
        let id = ep.dci();
        let ring = Ring::new()?;
        let interval = if ep.kind == 3 {
            if self.speed >= 3 {
                ep.interval.clamp(1, 16) - 1
            } else {
                3 + ep.interval.max(1).ilog2() as u8
            }
        } else {
            0
        };
        self.highest = self.highest.max(id);
        self.input.write(1, 1 | (1 << id));
        self.input
            .write(words, (self.speed << 20) | (u32::from(self.highest) << 27));
        self.input.write(words + 1, u32::from(self.port) << 16);
        let at = words * (usize::from(id) + 1);
        let kind = if id == 1 {
            4
        } else {
            ep.kind + if ep.address & 0x80 != 0 { 4 } else { 0 }
        };
        self.input.write(at, u32::from(interval) << 16);
        self.input.write(
            at + 1,
            (u32::from(ep.packet) << 16) | (u32::from(kind) << 3) | (3 << 1),
        );
        self.input.write(at + 2, ring.mem.addr() | 1);
        self.input.write(
            at + 4,
            u32::from(ep.packet)
                | if ep.kind == 3 {
                    u32::from(ep.packet) << 16
                } else {
                    0
                },
        );
        *self
            .pipes
            .iter_mut()
            .find(|p| p.is_none())
            .ok_or(Error("endpoint limit"))? = Some(Pipe { id, ring });
        Ok(())
    }
}
pub(super) struct Controller {
    regs: Registers,
    op: usize,
    db: usize,
    ir: usize,
    words: usize,
    ports: u8,
    command: Ring,
    events: Dma,
    _erst: Dma,
    dcbaa: Dma,
    event: usize,
    cycle: u32,
    slots: [Option<Slot>; SLOTS + 1],
    stopped: bool,
}
impl Controller {
    pub fn new(dev: crate::platform::pci::Device) -> Result<Self> {
        let r = dev.mmio().ok_or(Error("unmapped xHCI BAR"))?;
        dev.enable(6);
        let op = (r.read(0) & 255) as usize;
        let hcs = r.read(4);
        let ports = (hcs >> 24) as u8;
        let db = (r.read(0x14) & !3) as usize;
        let ir = (r.read(0x18) & !31) as usize + 32;
        if !r.fits(op, 0x400 + usize::from(ports) * 16)
            || !r.fits(ir, 32)
            || !r.fits(db, (SLOTS + 1) * 4)
        {
            return Err(Error("invalid xHCI registers"));
        }
        // Request OS ownership through the USB legacy support extended capability.
        let mut ext = (r.read(0x10) >> 16) as usize * 4;
        for _ in 0..64 {
            if ext == 0 {
                break;
            }
            if !r.fits(ext, 8) {
                return Err(Error("invalid xHCI capability"));
            }
            let cap = r.read(ext);
            if cap & 255 == 1 {
                r.write(ext, cap | (1 << 24));
                wait(|| r.read(ext) & (1 << 16) == 0)?;
                r.write(ext + 4, 0);
                break;
            }
            let next = ((cap >> 8) & 255) as usize * 4;
            if next == 0 {
                break;
            }
            ext += next;
        }
        r.write(op, r.read(op) & !1);
        wait(|| r.read(op + 4) & 1 != 0)?;
        r.write(op, 2);
        wait(|| r.read(op) & 2 == 0 && r.read(op + 4) & (1 << 11) == 0)?;
        if r.read(op + 8) & 1 == 0 {
            return Err(Error("controller requires larger pages"));
        }
        let hcs2 = r.read(8);
        let scratch = ((hcs2 >> 27) & 31) | (((hcs2 >> 21) & 31) << 5);
        if scratch > 64 {
            return Err(Error("scratchpad limit"));
        }
        let mut dcbaa = page()?;
        if scratch != 0 {
            let mut list = page()?;
            for n in 0..scratch as usize {
                list.write(n * 2, page()?.addr());
            }
            dcbaa.write(0, list.addr()); // all DMA pages have permanent storage
        }
        let command = Ring::new()?;
        let events = page()?;
        let mut erst = page()?;
        erst.write(0, events.addr());
        erst.write(2, N as u32);
        let write64 = |reg, addr| {
            r.write(reg, addr);
            r.write(reg + 4, 0);
        };
        r.write(op + 0x38, (hcs & 255).min(SLOTS as u32));
        write64(op + 0x30, dcbaa.addr());
        write64(op + 0x18, command.mem.addr() | 1);
        r.write(ir + 8, 1);
        write64(ir + 16, erst.addr());
        write64(ir + 24, events.addr());
        r.write(op, 1);
        wait(|| r.read(op + 4) & 1 == 0)?;
        Ok(Self {
            regs: r,
            op,
            db,
            ir,
            words: if r.read(0x10) & 4 != 0 { 16 } else { 8 },
            ports,
            command,
            events,
            _erst: erst,
            dcbaa,
            event: 0,
            cycle: 1,
            slots: core::array::from_fn(|_| None),
            stopped: false,
        })
    }
    fn completion(&mut self, kind: u32, pointer: u32, short: Option<u32>) -> Result<(u8, usize)> {
        let mut residue = 0;
        let start = self.regs.read(self.ir - 32) & 0x3fff;
        if self.stopped {
            return Err(Error("controller offline"));
        }
        for attempt in 0..4_000_000 {
            if attempt % 128 == 0
                && (self.regs.read(self.ir - 32).wrapping_sub(start) & 0x3fff) >= 8000
            {
                break;
            }
            let n = self.event * 4;
            let flags = self.events.read(n + 3);
            if flags & 1 != self.cycle {
                core::hint::spin_loop();
                continue;
            }
            fence(Ordering::Acquire);
            let code = self.events.read(n + 2);
            let ptr = self.events.read(n);
            self.event += 1;
            if self.event == N {
                self.event = 0;
                self.cycle ^= 1;
            }
            self.regs.write(
                self.ir + 24,
                (self.events.addr() + self.event as u32 * 16) | 8,
            );
            if (flags >> 10) & 63 != kind {
                continue;
            }
            if short == Some(ptr) && code >> 24 == 13 {
                residue = (code & 0xffffff) as usize;
                continue;
            }
            if ptr != pointer && short != Some(ptr) {
                continue;
            }
            return match code >> 24 {
                1 | 13 => Ok(((flags >> 24) as u8, residue.max((code & 0xffffff) as usize))),
                _ => Err(Error("USB transaction failed")),
            };
        }
        Err(Error("USB transfer timeout"))
    }
    fn cmd(&mut self, kind: u32, slot: u8, arg: u32) -> Result<u8> {
        self.endpoint_cmd(kind, slot, 0, arg)
    }
    fn endpoint_cmd(&mut self, kind: u32, slot: u8, ep: u8, arg: u32) -> Result<u8> {
        if self.stopped {
            return Err(Error("controller offline"));
        }
        let addr = self.command.put(
            arg.into(),
            0,
            (kind << 10) | (u32::from(ep) << 16) | (u32::from(slot) << 24),
        );
        fence(Ordering::SeqCst);
        self.regs.write(self.db, 0);
        match self.completion(33, addr, None) {
            Ok((id, _)) => Ok(id),
            Err(e) => {
                if e.0 == "USB transfer timeout" {
                    self.stopped = true;
                }
                Err(e)
            }
        }
    }
    fn transfer_result(&mut self, id: u8, ep: u8, done: u32, short: Option<u32>) -> Result<usize> {
        match self.completion(32, done, short) {
            Ok((_, left)) => Ok(left),
            Err(error) => {
                // A NAK on an idle interrupt endpoint is normal. Stop the outstanding
                // transfer before the shared DMA buffer is used by the next request.
                let words = self.words;
                let state = self.slot(id)?.output.read(usize::from(ep) * words) & 7;
                let recover = (|| {
                    match state {
                        1 => {
                            self.endpoint_cmd(15, id, ep, 0)?;
                        }
                        2 => {
                            self.endpoint_cmd(14, id, ep, 0)?;
                        }
                        3 => {}
                        _ => return Err(Error("endpoint unavailable")),
                    }
                    let ring = &self.slot(id)?.pipe(ep)?.ring;
                    let dequeue = (ring.mem.addr() + ring.next as u32 * 16) | ring.cycle;
                    self.endpoint_cmd(16, id, ep, dequeue)?;
                    Ok(())
                })();
                if recover.is_err() {
                    self.slot(id)?.offline = true;
                }
                Err(error)
            }
        }
    }
    pub fn ports(&self) -> u8 {
        self.ports
    }
    pub fn address(&mut self, port: u8) -> Result<Option<(u8, u8)>> {
        let reg = self.op + 0x400 + (usize::from(port) - 1) * 16;
        let mut status = self.regs.read(reg);
        if status & 1 == 0 {
            return Ok(None);
        }
        if status & 2 == 0 {
            self.regs.write(reg, (status & (1 << 9)) | 16);
            wait(|| self.regs.read(reg) & 16 == 0)?;
            status = self.regs.read(reg);
        }
        if status & 3 != 3 {
            return Err(Error("port reset failed"));
        }
        let speed = (status >> 10) & 15;
        let id = self.cmd(9, 0, 0)?;
        if id == 0 || usize::from(id) > SLOTS {
            return Err(Error("invalid slot"));
        }
        let mut slot = Slot::new(port, speed)?;
        slot.context(
            self.words,
            Endpoint {
                address: 0,
                kind: 0,
                packet: match speed {
                    3 => 64,
                    4.. => 512,
                    _ => 8,
                },
                interval: 0,
            },
        )?;
        let input = slot.input.addr();
        self.dcbaa.write(usize::from(id) * 2, slot.output.addr());
        self.slots[usize::from(id)] = Some(slot);
        self.cmd(11, id, input)?;
        Ok(Some((id, speed as u8)))
    }
    fn slot(&mut self, id: u8) -> Result<&mut Slot> {
        self.slots
            .get_mut(usize::from(id))
            .and_then(Option::as_mut)
            .ok_or(Error("no slot"))
    }
}
impl super::HostController for Controller {
    fn packet_size(&mut self, id: u8, max: u16) -> Result<()> {
        let words = self.words;
        let s = self.slot(id)?;
        let offset = words * 2;
        s.input.write(1, 2);
        let bits = s.input.read(offset + 1);
        s.input
            .write(offset + 1, (bits & 0xffff) | (u32::from(max) << 16));
        let ptr = s.input.addr();
        self.cmd(13, id, ptr)?;
        Ok(())
    }
    fn configure(&mut self, id: u8, ep: Endpoint) -> Result<()> {
        let words = self.words;
        let slot = self.slot(id)?;
        slot.context(words, ep)?;
        let addr = slot.input.addr();
        self.cmd(12, id, addr)?;
        Ok(())
    }
}
impl UsbIo for Controller {
    fn control(&mut self, id: u8, setup: Setup, bytes: &mut [u8]) -> Result<usize> {
        if self.stopped || bytes.len() > 4096 || usize::from(setup.len) != bytes.len() {
            return Err(Error("invalid control request"));
        }
        let slot = self.slot(id)?;
        if slot.offline {
            return Err(Error("USB device offline"));
        }
        let read = setup.kind & 0x80 != 0;
        if !read {
            slot.buffer.send(bytes);
        }
        let buffer = slot.buffer.addr();
        let ring = &mut slot.pipe(1)?.ring;
        ring.put(
            setup.bits(),
            8,
            (2 << 10)
                | (1 << 6)
                | (if bytes.is_empty() {
                    0
                } else if read {
                    3
                } else {
                    2
                } << 16),
        );
        let data = if !bytes.is_empty() {
            Some(ring.put(
                buffer.into(),
                bytes.len() as u32,
                (3 << 10) | (u32::from(read) << 16) | (u32::from(read) << 2),
            ))
        } else {
            None
        };
        let done = ring.put(
            0,
            0,
            (4 << 10) | (1 << 5) | (u32::from(!read || bytes.is_empty()) << 16),
        );
        fence(Ordering::SeqCst);
        self.regs.write(self.db + usize::from(id) * 4, 1);
        let left = self.transfer_result(id, 1, done, data)?;
        let n = bytes
            .len()
            .checked_sub(left)
            .ok_or(Error("invalid control length"))?;
        if read {
            self.slot(id)?.buffer.receive(&mut bytes[..n]);
        }
        Ok(n)
    }
    fn transfer(&mut self, id: u8, ep: Endpoint, bytes: &mut [u8]) -> Result<usize> {
        if self.stopped || bytes.len() > 4096 {
            return Err(Error("invalid transfer"));
        }
        let slot = self.slot(id)?;
        if slot.offline {
            return Err(Error("USB device offline"));
        }
        let read = ep.address & 0x80 != 0;
        if !read {
            slot.buffer.send(bytes);
        }
        let buffer = slot.buffer.addr();
        let addr = slot.pipe(ep.dci())?.ring.put(
            buffer.into(),
            bytes.len() as u32,
            (1 << 10) | (1 << 5) | (1 << 2),
        );
        fence(Ordering::SeqCst);
        self.regs
            .write(self.db + usize::from(id) * 4, u32::from(ep.dci()));
        let left = self.transfer_result(id, ep.dci(), addr, None)?;
        let n = bytes
            .len()
            .checked_sub(left)
            .ok_or(Error("invalid transfer length"))?;
        if read {
            self.slot(id)?.buffer.receive(&mut bytes[..n]);
        }
        Ok(n)
    }
}

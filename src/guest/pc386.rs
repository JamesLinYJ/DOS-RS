//! Transfer real-mode callers into a full 386 interpreter when they need its instruction set.
//! DOS services run at the real-mode boundary. Protected-mode interrupts use the guest IDT.
use super::memory::{EMS_FRAME, EMS_PAGE, GuestRam, RAM_BYTES};
use super::state::{CpuState, Segment, Slot};
use super::step::Outcome;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};
use rsemu::core::space::{AddressSpace, MappingId, MemAttrs, MemOps, MemResult, RamStore, Region};
use rsemu::cpu::x86::{Config, Regs, X86};

#[derive(Debug)]
struct Ports {
    a20: AtomicBool,
}
impl MemOps for Ports {
    fn read(&self, offset: u64, out: &mut [u8], _: MemAttrs) -> MemResult {
        for (i, b) in out.iter_mut().enumerate() {
            let port = offset as u16 + i as u16;
            *b = if port == 0x92 {
                u8::from(self.a20.load(Relaxed)) << 1
            } else {
                crate::bios::pc::port_read(port)
            };
        }
        Ok(())
    }
    fn write(&self, offset: u64, data: &[u8], _: MemAttrs) -> MemResult {
        for (i, &b) in data.iter().enumerate() {
            let port = offset as u16 + i as u16;
            if port == 0x92 {
                self.a20.store(b & 2 != 0, Relaxed)
            } else {
                crate::bios::pc::port_write(port, b);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct Machine {
    cpu: X86,
    space: Arc<AddressSpace>,
    expanded_pages: [Option<(usize, MappingId)>; 4],
    memory: Arc<RamStore>,
    ports: Arc<Ports>,
    host_event: bool,
}

impl Machine {
    pub(super) fn new(registers: &CpuState, ram: &mut GuestRam) -> Self {
        let memory = ram
            .machine_memory
            .get_or_insert_with(|| Arc::new(RamStore::new(RAM_BYTES as u64)))
            .clone();
        let space = AddressSpace::new("dos-memory", 32);
        space
            .topology()
            .map(Region::ram("ram", memory.clone()), 0)
            .unwrap();
        let ports = Arc::new(Ports {
            a20: AtomicBool::new(ram.a20),
        });
        let io = AddressSpace::new("dos-ports", 16);
        io.topology()
            .map(Region::io("ports", 65536, ports.clone()), 0)
            .unwrap();
        if let Some(at) = &ram.at {
            at.map(&io);
        }
        if let Some(g) = &ram.graphics {
            g.attach(&space, &io);
        }
        let cpu = X86::new(Config::I80386);
        let space = Arc::new(space);
        let io = Arc::new(io);
        cpu.attach_space(space.clone());
        cpu.attach_io_space(io);
        cpu.step(); // complete power-on reset before installing the DOS register file
        cpu.set_regs(to_native(registers, cpu.regs()));
        let machine = Self {
            cpu,
            space,
            expanded_pages: [None; 4],
            memory,
            ports,
            host_event: false,
        };
        ram.upload(&machine.memory);
        machine
    }

    pub(super) fn slice(
        &mut self,
        state: &mut CpuState,
        ram: &mut GuestRam,
        budget: u32,
    ) -> (Outcome, u16) {
        if self.host_event {
            self.cpu.set_regs(to_native(state, self.cpu.regs()));
            self.host_event = false;
        }
        for (n, (&physical, mapped)) in ram.frame.iter().zip(&mut self.expanded_pages).enumerate() {
            if physical == mapped.map(|(page, _)| page) {
                continue;
            }
            let mut topology = self.space.topology();
            if let Some((_, id)) = mapped.take() {
                topology.unmap(id).unwrap();
            }
            if let Some(page) = physical {
                let backing = Region::ram("expanded RAM", self.memory.clone());
                let alias =
                    Region::alias("EMS window", backing, page as u64, EMS_PAGE as u64).unwrap();
                let id = topology
                    .map_with_priority(alias, (EMS_FRAME + n * EMS_PAGE) as u64, 1)
                    .unwrap();
                *mapped = Some((page, id));
            }
        }
        ram.upload(&self.memory);
        self.ports.a20.store(ram.a20, Relaxed);
        self.cpu.set_a20(ram.a20);
        let mut event = Outcome::Continue;
        let mut at = state.instruction();
        let graphics = ram.graphics.as_ref().is_some_and(|g| g.active);
        if graphics && let Some(at) = &ram.at {
            at.advance(128);
            if self.cpu.regs().eflags & 0x200 != 0
                && !self.cpu.interrupt_shadow()
                && let Some(vector) = at.interrupt()
            {
                self.cpu.set_intr_vector(vector);
                self.cpu.set_intr(true);
                self.cpu.step();
                self.cpu.set_intr(false);
            }
        }
        for _ in 0..budget {
            let regs = self.cpu.regs();
            at = regs.rip as u16;
            let sys = self.cpu.sys();
            if !sys.protected() {
                let pc = (regs.cs, at);
                if let Some(v) = super::step::firmware(pc.0, pc.1) {
                    event = Outcome::Firmware(v);
                    break;
                }
                if pc == super::run::xms::ENTRY {
                    event = Outcome::Xms;
                    break;
                }
                if pc == super::run::ems::ENTRY {
                    event = Outcome::Ems;
                    break;
                }
                if pc == super::run::ems::RETURN {
                    event = Outcome::EmsReturn;
                    break;
                }
                let addr = ram.address(regs.cs, at) as u64;
                if self.memory.read_u8(addr) == Ok(0xcd) {
                    let v = self
                        .memory
                        .read_u8(ram.address(regs.cs, at.wrapping_add(1)) as u64)
                        .unwrap_or(0);
                    // A hooked interrupt must run its guest handler; the IRET
                    // frame belongs to the CPU in that case.
                    let base = sys.idtr.base + 4 * u64::from(v);
                    let entry = self
                        .memory
                        .read_value(
                            base,
                            rsemu::core::value::Width::U32,
                            rsemu::core::value::Endian::Little,
                        )
                        .unwrap_or(0);
                    if entry == 0 || v == 0x67 && entry == 0xf1000000 {
                        let mut next = regs;
                        next.rip = u64::from(at.wrapping_add(2));
                        self.cpu.set_regs(next);
                        event = Outcome::Interrupt(v);
                        break;
                    }
                }
            }
            self.cpu.step();
            let a20 = self.ports.a20.load(Relaxed);
            if a20 != ram.a20 {
                ram.a20 = a20;
                self.cpu.set_a20(a20);
            }
            if self.cpu.reset_pending() || self.cpu.reset_requested() {
                event = Outcome::Fault(0x0f);
                break;
            }
        }
        let regs = self.cpu.regs();
        from_native(state, regs);
        ram.download(&self.memory);
        self.host_event = event != Outcome::Continue;
        (event, at)
    }
}

fn to_native(state: &CpuState, mut r: Regs) -> Regs {
    for (slot, p) in [
        Slot::Accumulator,
        Slot::Count,
        Slot::Data,
        Slot::Base,
        Slot::Stack,
        Slot::Frame,
        Slot::Source,
        Slot::Dest,
    ]
    .into_iter()
    .zip([
        &mut r.rax, &mut r.rcx, &mut r.rdx, &mut r.rbx, &mut r.rsp, &mut r.rbp, &mut r.rsi,
        &mut r.rdi,
    ]) {
        *p = u64::from(state.dword(slot));
    }
    r.cs = state.selector(Segment::Code);
    r.ds = state.selector(Segment::Data);
    r.es = state.selector(Segment::Extra);
    r.ss = state.selector(Segment::Stack);
    r.rip = state.instruction().into();
    r.eflags = (r.eflags & 0xffff0000) | u32::from(state.flags());
    r
}
fn from_native(state: &mut CpuState, r: Regs) {
    for (i, v) in [r.rax, r.rcx, r.rdx, r.rbx, r.rsp, r.rbp, r.rsi, r.rdi]
        .into_iter()
        .enumerate()
    {
        state.set_dword(Slot::from_index(i as u8), v as u32);
    }
    for (s, v) in [
        (Segment::Code, r.cs),
        (Segment::Data, r.ds),
        (Segment::Extra, r.es),
        (Segment::Stack, r.ss),
    ] {
        state.set_selector(s, v);
    }
    state.set_instruction(r.rip as u16);
    state.set_flags(r.eflags as u16);
}

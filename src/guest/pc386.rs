//! Hidden segment registers and descriptor tables of the guest 80386.
//! All addresses here are offsets on the session's memory bus.
use super::memory::{GuestRam, RAM_BYTES};
use super::state::{CpuState, Segment};

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Table {
    pub base: u32,
    pub limit: u16,
}
#[derive(Debug, Clone, Copy)]
pub(super) struct Descriptor {
    pub base: u32,
    pub limit: u32,
    pub access: u8,
    pub flags: u8,
}
impl Descriptor {
    const REAL: Self = Self {
        base: 0,
        limit: 65535,
        access: 0x93,
        flags: 0,
    };
}
#[derive(Debug)]
pub(super) struct Machine {
    pub gdtr: Table,
    pub idtr: Table,
    pub ldt: u16,
    pub tr: u16,
    pub cr0: u32,
    pub cr2: u32,
    pub cr3: u32,
    pub inhibit: bool,
    pub exception: core::cell::Cell<Option<(u8, Option<u16>)>>,
    segments: [Descriptor; 6],
}
impl Machine {
    pub fn new() -> Self {
        Self {
            gdtr: Table::default(),
            idtr: Table {
                base: 0,
                limit: 1023,
            },
            ldt: 0,
            tr: 0,
            cr0: 0,
            cr2: 0,
            cr3: 0,
            inhibit: false,
            exception: core::cell::Cell::new(None),
            segments: [Descriptor::REAL; 6],
        }
    }
    pub fn protected(&self) -> bool {
        self.cr0 & 1 != 0
    }
    pub fn real_segment(&mut self, s: Segment, v: u16) {
        self.segments[s.index()].base = u32::from(v) << 4;
    }
    pub fn physical(&self, ram: &GuestRam, linear: u32) -> Option<usize> {
        let addr = if ram.a20 { linear } else { linear & !(1 << 20) } as usize;
        (addr < RAM_BYTES).then_some(addr)
    }
    pub fn linear(&self, ram: &GuestRam, s: Segment, off: u32, write: bool) -> Option<usize> {
        let d = self.segments[s.index()];
        let expand_down = d.access & 0x1c == 0x14;
        let valid = if expand_down {
            off > d.limit && (d.flags & 0x40 != 0 || off <= 0xffff)
        } else {
            off <= d.limit
        };
        if !valid || d.access & 0x80 == 0 || self.protected() && write && d.access & 10 != 2 {
            if self.protected() {
                self.exception
                    .set(Some((if s == Segment::Stack { 12 } else { 13 }, Some(0))));
            }
            return None;
        }
        self.physical(ram, d.base.wrapping_add(off))
    }
    fn bytes<const N: usize>(&self, ram: &GuestRam, addr: u32) -> Option<[u8; N]> {
        let mut result = [0; N];
        for (i, b) in result.iter_mut().enumerate() {
            *b = ram.bus_byte(self.physical(ram, addr.checked_add(i as u32)?)?)
        }
        Some(result)
    }
    fn table_entry(&self, ram: &GuestRam, table: Table, offset: u32) -> Option<[u8; 8]> {
        if offset + 7 > u32::from(table.limit) {
            return None;
        }
        self.bytes(ram, table.base.checked_add(offset)?)
    }
    fn raw(&self, ram: &GuestRam, sel: u16) -> Option<[u8; 8]> {
        if sel & !3 == 0 {
            return None;
        }
        let table = if sel & 4 == 0 {
            self.gdtr
        } else {
            if self.ldt & 4 != 0 {
                return None;
            }
            let l = self.descriptor(ram, self.ldt)?;
            if l.access & 0x9f != 0x82 {
                return None;
            }
            Table {
                base: l.base,
                limit: l.limit.min(65535) as u16,
            }
        };
        self.table_entry(ram, table, u32::from(sel & !7))
    }
    pub fn call_gate(&self, ram: &GuestRam, sel: u16) -> Option<(u16, u16, u8)> {
        let b = self.raw(ram, sel)?;
        if b[5] & 0x9f != 0x84 {
            return None;
        }
        Some((
            u16::from_le_bytes([b[2], b[3]]),
            u16::from_le_bytes([b[0], b[1]]),
            b[4] & 31,
        ))
    }
    pub fn descriptor(&self, ram: &GuestRam, sel: u16) -> Option<Descriptor> {
        let b = self.raw(ram, sel)?;
        let raw = u32::from_le_bytes([b[0], b[1], b[6] & 15, 0]);
        Some(Descriptor {
            base: u32::from_le_bytes([b[2], b[3], b[4], b[7]]),
            limit: if b[6] & 128 != 0 {
                (raw << 12) | 4095
            } else {
                raw
            },
            access: b[5],
            flags: b[6],
        })
    }
    pub fn load(&mut self, ram: &GuestRam, s: Segment, sel: u16) -> bool {
        if !self.protected() {
            self.real_segment(s, sel);
            return true;
        }
        if sel & !3 == 0 && !matches!(s, Segment::Code | Segment::Stack) {
            self.segments[s.index()] = Descriptor {
                limit: 0,
                access: 0,
                ..Descriptor::REAL
            };
            return true;
        }
        let Some(d) = self.descriptor(ram, sel) else {
            self.exception.set(Some((13, Some(sel & !3))));
            return false;
        };
        if d.access & 128 == 0 {
            self.exception.set(Some((
                if s == Segment::Stack { 12 } else { 11 },
                Some(sel & !3),
            )));
            return false;
        }
        if d.access & 0x90 != 0x90
            || s == Segment::Code && (d.access & 8 == 0 || d.flags & 0x40 != 0)
            || s == Segment::Stack
                && (d.access & 10 != 2 || u16::from(d.access >> 5 & 3) != sel & 3)
            || !matches!(s, Segment::Code | Segment::Stack) && d.access & 10 == 8
        {
            self.exception.set(Some((13, Some(sel & !3))));
            return false;
        }
        self.segments[s.index()] = d;
        if s == Segment::Stack {
            self.inhibit = true;
        }
        true
    }
}
fn real_vector(cpu: &CpuState, ram: &GuestRam, v: u8) -> Option<(u16, u16)> {
    let offset = u32::from(v) * 4;
    if offset + 3 > u32::from(cpu.machine.idtr.limit) {
        return None;
    }
    let b = cpu
        .machine
        .bytes::<4>(ram, cpu.machine.idtr.base.checked_add(offset)?)?;
    Some((
        u16::from_le_bytes([b[2], b[3]]),
        u16::from_le_bytes([b[0], b[1]]),
    ))
}
pub(super) fn has_vector(cpu: &CpuState, ram: &GuestRam, v: u8) -> bool {
    cpu.machine.protected() || real_vector(cpu, ram, v).is_some_and(|p| p != (0, 0))
}
pub(super) fn interrupt(cpu: &mut CpuState, ram: &mut GuestRam, v: u8) -> Option<()> {
    let (cs, ip, clear_if) = if cpu.machine.protected() {
        let b = cpu
            .machine
            .table_entry(ram, cpu.machine.idtr, u32::from(v) * 8)?;
        if !matches!(b[5] & 0x9f, 0x86 | 0x87) {
            return None;
        }
        (
            u16::from_le_bytes([b[2], b[3]]),
            u16::from_le_bytes([b[0], b[1]]),
            b[5] & 1 == 0,
        )
    } else {
        let (cs, ip) = real_vector(cpu, ram, v)?;
        (cs, ip, true)
    };
    let mut cs = cs;
    if cpu.machine.protected() {
        let d = cpu.machine.descriptor(ram, cs)?;
        let level = u16::from((d.access >> 5) & 3);
        let cpl = cpu.selector(Segment::Code) & 3;
        if level > cpl || d.access & 0x98 != 0x98 {
            return None;
        }
        if level < cpl && d.access & 4 == 0 && !switch_stack(cpu, ram, level, 0) {
            return None;
        }
        cs = (cs & !3) | if d.access & 4 != 0 { cpl } else { level };
    }
    for word in [cpu.flags(), cpu.selector(Segment::Code), cpu.instruction()] {
        if !super::step::push_word(cpu, ram, word) {
            return None;
        }
    }
    if !cpu.machine.load(ram, Segment::Code, cs) {
        return None;
    }
    cpu.set_selector(Segment::Code, cs);
    cpu.set_instruction(ip);
    cpu.set_flags(cpu.flags() & !0x100);
    if clear_if {
        cpu.set_interrupt(false)
    }
    Some(())
}

pub(super) fn switch_stack(cpu: &mut CpuState, ram: &mut GuestRam, level: u16, n: u8) -> bool {
    use super::state::Slot;
    let Some(tss) = cpu.machine.descriptor(ram, cpu.machine.tr) else {
        return false;
    };
    let offset = match tss.access & 15 {
        1 | 3 => 2 + u32::from(level) * 4,
        _ => return false,
    };
    if offset + 3 > tss.limit {
        return false;
    }
    let Some(stack) = cpu.machine.bytes::<4>(ram, tss.base.wrapping_add(offset)) else {
        return false;
    };
    let (ss, sp) = (cpu.selector(Segment::Stack), cpu.wide(Slot::Stack));
    let mut args = [0; 31];
    for i in 0..usize::from(n) {
        let off = sp.wrapping_add(2 * i as u16);
        let (Some(lo), Some(hi)) = (
            cpu.load_byte(ram, Segment::Stack, off),
            cpu.load_byte(ram, Segment::Stack, off.wrapping_add(1)),
        ) else {
            return false;
        };
        args[i] = u16::from_le_bytes([lo, hi]);
    }
    let new_ss = u16::from_le_bytes([stack[2], stack[3]]);
    if new_ss & 3 != level || !cpu.machine.load(ram, Segment::Stack, new_ss) {
        return false;
    }
    cpu.set_selector(Segment::Stack, new_ss);
    cpu.set_wide(Slot::Stack, u16::from_le_bytes([stack[0], stack[1]]));
    for word in [ss, sp]
        .into_iter()
        .chain(args[..usize::from(n)].iter().rev().copied())
    {
        if !super::step::push_word(cpu, ram, word) {
            return false;
        }
    }
    true
}

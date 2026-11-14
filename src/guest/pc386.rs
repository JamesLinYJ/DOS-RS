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
#[derive(Debug, Clone)]
pub(super) struct Machine {
    pub gdtr: Table,
    pub idtr: Table,
    pub ldt: u16,
    pub tr: u16,
    pub cr0: u32,
    pub cr2: core::cell::Cell<u32>,
    pub cr3: u32,
    pub debug: [u32; 8],
    pub vm86: bool,
    pub cpl: u8,
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
            cr2: core::cell::Cell::new(0),
            cr3: 0,
            debug: [0, 0, 0, 0, 0, 0, 0xffff0ff0, 0x400],
            inhibit: false,
            vm86: false,
            cpl: 0,
            exception: core::cell::Cell::new(None),
            segments: [Descriptor::REAL; 6],
        }
    }
    pub fn protected(&self) -> bool {
        self.cr0 & 1 != 0
    }
    pub fn real_segment(&mut self, s: Segment, v: u16) {
        self.segments[s.index()] = Descriptor {
            base: u32::from(v) << 4,
            ..Descriptor::REAL
        };
    }
    pub fn code32(&self) -> bool {
        self.protected() && !self.vm86 && self.segments[Segment::Code.index()].flags & 0x40 != 0
    }
    pub fn stack32(&self) -> bool {
        self.protected() && !self.vm86 && self.segments[Segment::Stack.index()].flags & 0x40 != 0
    }
    fn bus_address(ram: &GuestRam, physical: u32) -> Option<usize> {
        let at = if ram.a20 {
            physical
        } else {
            physical & !(1 << 20)
        } as usize;
        (at < RAM_BYTES).then_some(at)
    }
    fn entry(ram: &GuestRam, physical: u32) -> Option<(usize, u32)> {
        let at = Self::bus_address(ram, physical)?;
        if at + 3 >= RAM_BYTES {
            return None;
        }
        Some((
            at,
            u32::from_le_bytes(core::array::from_fn(|i| ram.bus_byte(at + i))),
        ))
    }
    /// Walk a legacy 4 KiB page mapping without changing CR2. DOS services
    /// also use this walk for a V86 caller's segment:offset buffers.
    pub(super) fn walk(
        ram: &GuestRam,
        cr3: u32,
        linear: u32,
        write: bool,
        user: bool,
    ) -> Result<(usize, usize, usize), bool> {
        let (directory, pde) = Self::entry(ram, (cr3 & !4095) + (linear >> 22) * 4).ok_or(false)?;
        if pde & 1 == 0 {
            return Err(false);
        }
        let (table, pte) =
            Self::entry(ram, (pde & !4095) + ((linear >> 12) & 1023) * 4).ok_or(false)?;
        if pte & 1 == 0 {
            return Err(false);
        }
        if user && (pde & pte & 4 == 0 || write && pde & pte & 2 == 0) {
            return Err(true);
        }
        let address = Self::bus_address(ram, (pte & !4095) | (linear & 4095)).ok_or(false)?;
        Ok((address, directory, table))
    }
    pub(super) fn touch(ram: &mut GuestRam, directory: usize, table: usize, write: bool) {
        for (addr, bits) in [(directory, 0x20), (table, if write { 0x60 } else { 0x20 })] {
            let old = ram.bus_byte(addr);
            if old & bits != bits {
                ram.set_bus_byte(addr, old | bits);
            }
        }
    }
    fn physical(&self, ram: &mut GuestRam, linear: u32, write: bool, user: bool) -> Option<usize> {
        if self.cr0 & 0x8000_0000 == 0 {
            return Self::bus_address(ram, linear);
        }
        match Self::walk(ram, self.cr3, linear, write, user) {
            Ok((address, directory, table)) => {
                Self::touch(ram, directory, table, write);
                Some(address)
            }
            Err(protection) => {
                self.cr2.set(linear);
                let code = u16::from(protection) | u16::from(write) << 1 | u16::from(user) << 2;
                self.exception.set(Some((14, Some(code))));
                None
            }
        }
    }
    pub fn linear(&self, ram: &mut GuestRam, s: Segment, off: u32, write: bool) -> Option<usize> {
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
        self.physical(
            ram,
            d.base.wrapping_add(off),
            write,
            self.vm86 || self.cpl == 3,
        )
    }
    fn bytes<const N: usize>(&self, ram: &mut GuestRam, addr: u32) -> Option<[u8; N]> {
        let mut result = [0; N];
        for (i, b) in result.iter_mut().enumerate() {
            let at = self.physical(ram, addr.checked_add(i as u32)?, false, false)?;
            *b = ram.bus_byte(at)
        }
        Some(result)
    }
    fn table_entry(&self, ram: &mut GuestRam, table: Table, offset: u32) -> Option<[u8; 8]> {
        if offset + 7 > u32::from(table.limit) {
            return None;
        }
        self.bytes(ram, table.base.checked_add(offset)?)
    }
    fn raw(&self, ram: &mut GuestRam, sel: u16) -> Option<[u8; 8]> {
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
    pub fn call_gate(&self, ram: &mut GuestRam, sel: u16) -> Option<(u16, u16, u8)> {
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
    pub fn descriptor(&self, ram: &mut GuestRam, sel: u16) -> Option<Descriptor> {
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
    pub fn io_allowed(&self, ram: &mut GuestRam, flags: u16, port: u16, width: u8) -> bool {
        if !self.protected() || !self.vm86 && self.cpl <= ((flags >> 12) & 3) as u8 {
            return true;
        }
        let Some(tss) = self.descriptor(ram, self.tr) else {
            return false;
        };
        if !matches!(tss.access & 15, 9 | 11) || tss.limit < 103 {
            return false;
        }
        let Some(bytes) = self.bytes::<2>(ram, tss.base.wrapping_add(102)) else {
            return false;
        };
        let map = u32::from(u16::from_le_bytes(bytes));
        (0..u32::from(width)).all(|i| {
            let bit = u32::from(port) + i;
            let at = map + bit / 8;
            at <= tss.limit
                && self
                    .bytes::<1>(ram, tss.base.wrapping_add(at))
                    .is_some_and(|v| v[0] & (1 << (bit % 8)) == 0)
        })
    }
    pub fn load(&mut self, ram: &mut GuestRam, s: Segment, sel: u16) -> bool {
        if !self.protected() || self.vm86 {
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
            if self.exception.get().is_none() {
                self.exception.set(Some((13, Some(sel & !3))));
            }
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
            || s == Segment::Code && d.access & 8 == 0
            || s == Segment::Stack
                && (d.access & 10 != 2 || u16::from(d.access >> 5 & 3) != sel & 3)
            || !matches!(s, Segment::Code | Segment::Stack) && d.access & 10 == 8
        {
            self.exception.set(Some((13, Some(sel & !3))));
            return false;
        }
        if s == Segment::Code {
            self.cpl = (sel & 3) as u8;
        }
        self.segments[s.index()] = d;
        if s == Segment::Stack {
            self.inhibit = true;
        }
        true
    }
}
fn real_vector(cpu: &CpuState, ram: &mut GuestRam, v: u8) -> Option<(u16, u16)> {
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
pub(super) fn has_vector(cpu: &CpuState, ram: &mut GuestRam, v: u8) -> bool {
    cpu.machine.protected() || real_vector(cpu, ram, v).is_some_and(|p| p != (0, 0))
}
pub(super) fn interrupt(
    cpu: &mut CpuState,
    ram: &mut GuestRam,
    v: u8,
    software: bool,
) -> Option<u32> {
    use super::step::push_stack;
    let old_cs = cpu.selector(Segment::Code);
    let flags = cpu.eflags();
    let ip = cpu.eip();
    let (cs, entry, width, clear_if) = if cpu.machine.protected() {
        let b = cpu
            .machine
            .table_entry(ram, cpu.machine.idtr, u32::from(v) * 8)?;
        let kind = b[5] & 0x9f;
        if !matches!(kind, 0x86 | 0x87 | 0x8e | 0x8f) {
            return None;
        }
        if software && cpu.machine.cpl > (b[5] >> 5 & 3) {
            cpu.machine
                .exception
                .set(Some((13, Some(u16::from(v) * 8 + 2))));
            return None;
        }
        let wide = kind & 8 != 0;
        let dest = u16::from_le_bytes([b[2], b[3]]);
        let entry = u32::from_le_bytes([
            b[0],
            b[1],
            if wide { b[6] } else { 0 },
            if wide { b[7] } else { 0 },
        ]);
        let code = cpu.machine.descriptor(ram, dest)?;
        let level = code.access >> 5 & 3;
        let cpl = cpu.machine.cpl;
        if level > cpl || code.access & 0x98 != 0x98 || entry > code.limit {
            return None;
        }
        let target = if code.access & 4 != 0 { cpl } else { level };
        let width = if wide { 4 } else { 2 };
        if cpu.machine.vm86 && (!wide || target != 0) {
            return None;
        }
        if target < cpl {
            let tss = cpu.machine.descriptor(ram, cpu.machine.tr)?;
            let (ss, sp) = match tss.access & 15 {
                9 | 11 => {
                    let at = 4 + u32::from(target) * 8;
                    if at + 5 > tss.limit {
                        return None;
                    }
                    let b = cpu.machine.bytes::<6>(ram, tss.base.checked_add(at)?)?;
                    (
                        u16::from_le_bytes([b[4], b[5]]),
                        u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    )
                }
                1 | 3 => {
                    let at = 2 + u32::from(target) * 4;
                    if at + 3 > tss.limit {
                        return None;
                    }
                    let b = cpu.machine.bytes::<4>(ram, tss.base.checked_add(at)?)?;
                    (
                        u16::from_le_bytes([b[2], b[3]]),
                        u32::from(u16::from_le_bytes([b[0], b[1]])),
                    )
                }
                _ => return None,
            };
            let old_ss = cpu.selector(Segment::Stack);
            let old_sp = cpu.dword(super::state::Slot::Stack);
            let vm = cpu.machine.vm86;
            cpu.machine.vm86 = false;
            cpu.machine.cpl = target;
            if ss & 3 != u16::from(target) || !cpu.machine.load(ram, Segment::Stack, ss) {
                return None;
            }
            cpu.set_selector(Segment::Stack, ss);
            cpu.set_dword(super::state::Slot::Stack, sp);
            if vm {
                for s in [Segment::Gs, Segment::Fs, Segment::Data, Segment::Extra] {
                    push_stack(cpu, ram, u32::from(cpu.selector(s)), 4)?;
                    cpu.machine.load(ram, s, 0);
                    cpu.set_selector(s, 0);
                }
            }
            push_stack(cpu, ram, u32::from(old_ss), width)?;
            push_stack(cpu, ram, old_sp, width)?;
        }
        ((dest & !3) | u16::from(target), entry, width, kind & 1 == 0)
    } else {
        let (cs, ip) = real_vector(cpu, ram, v)?;
        (cs, u32::from(ip), 2, true)
    };
    for word in [flags, u32::from(old_cs), ip] {
        push_stack(cpu, ram, word, width)?;
    }
    if !cpu.machine.load(ram, Segment::Code, cs) {
        return None;
    }
    cpu.set_selector(Segment::Code, cs);
    cpu.set_eip(entry);
    cpu.set_flags(cpu.flags() & !0x4100);
    if clear_if {
        cpu.set_interrupt(false)
    }
    Some(width)
}

pub(super) fn iretd(cpu: &mut CpuState, ram: &mut GuestRam) -> Option<()> {
    use super::step::pop_stack;
    let ip = pop_stack(cpu, ram, 4)?;
    let cs = pop_stack(cpu, ram, 4)? as u16;
    let flags = pop_stack(cpu, ram, 4)?;
    let old_level = cpu.machine.cpl;
    if cpu.machine.protected() && !cpu.machine.vm86 && old_level == 0 && flags & (1 << 17) != 0 {
        if ip > 65535 {
            return None;
        }
        let sp = pop_stack(cpu, ram, 4)?;
        let ss = pop_stack(cpu, ram, 4)? as u16;
        let mut saved = [0u16; 4];
        for sel in &mut saved {
            *sel = pop_stack(cpu, ram, 4)? as u16;
        }
        cpu.machine.vm86 = true;
        cpu.machine.cpl = 3;
        cpu.set_selector(Segment::Stack, ss);
        cpu.set_dword(super::state::Slot::Stack, sp);
        for (s, sel) in [Segment::Extra, Segment::Data, Segment::Fs, Segment::Gs]
            .into_iter()
            .zip(saved)
        {
            cpu.set_selector(s, sel);
        }
    } else if cpu.machine.protected() && !cpu.machine.vm86 {
        if (cs & 3) < u16::from(old_level) {
            return None;
        }
        if cs & 3 > u16::from(old_level) {
            let sp = pop_stack(cpu, ram, 4)?;
            let ss = pop_stack(cpu, ram, 4)? as u16;
            if ss & 3 != cs & 3 || !cpu.machine.load(ram, Segment::Stack, ss) {
                return None;
            }
            cpu.set_selector(Segment::Stack, ss);
            cpu.set_dword(super::state::Slot::Stack, sp);
        }
        if !cpu.machine.load(ram, Segment::Code, cs) {
            return None;
        }
    }
    cpu.set_selector(Segment::Code, cs);
    cpu.set_eip(ip);
    let mut mask = 0xffffu16;
    if old_level != 0 {
        mask &= !0x3000;
        if old_level > (cpu.flags() >> 12 & 3) as u8 {
            mask &= !0x200;
        }
    }
    cpu.set_flags((cpu.flags() & !mask) | (flags as u16 & mask));
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

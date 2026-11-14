//! LIM expanded memory. Page registers map guest bus addresses, so two slots
//! mapped to the same page see each other's writes immediately.
use super::{CpuState, GuestRam, Segment, Slot};
use crate::guest::memory::{EMS_FRAME, EMS_PAGE, EXT_START, RAM_BYTES};
pub(in crate::guest) const ENTRY: (u16, u16) = (0xf100, 0);
pub(in crate::guest) const RETURN: (u16, u16) = (ENTRY.0, 0x20);
const PAGES: usize = (RAM_BYTES - EXT_START) / EMS_PAGE;
const HANDLES: usize = 64;
const EMPTY: u16 = u16::MAX;
type Map = [(u16, u16); 4];
const UNMAPPED: (u16, u16) = (EMPTY, EMPTY);
#[derive(Clone, Copy)]
struct Handle {
    pages: u16,
    name: [u8; 8],
    saved: Option<Map>,
}
const NEW: Handle = Handle {
    pages: 0,
    name: [0; 8],
    saved: None,
};
pub(super) struct Ems {
    handles: [Option<Handle>; HANDLES],
    // Indexed by physical expanded-memory page; no contiguous allocation required.
    owners: [(u16, u16); PAGES],
    map: Map,
    alt_area: Option<Far>,
    os_key: Option<u32>,
    os_enabled: bool,
}
#[derive(Clone, Copy)]
struct Far {
    seg: u16,
    off: u16,
}
impl Far {
    fn pointer(self, ram: &GuestRam, i: u16) -> Self {
        Self {
            off: self.word(ram, i),
            seg: self.word(ram, i.wrapping_add(2)),
        }
    }
    fn byte(self, ram: &GuestRam, i: u16) -> u8 {
        ram.load(self.seg, self.off.wrapping_add(i)).unwrap()
    }
    fn word(self, ram: &GuestRam, i: u16) -> u16 {
        u16::from_le_bytes([self.byte(ram, i), self.byte(ram, i.wrapping_add(1))])
    }
    fn put(self, ram: &mut GuestRam, offset: u16, bytes: &[u8]) {
        for (i, b) in bytes.iter().enumerate() {
            ram.store(
                self.seg,
                self.off.wrapping_add(offset).wrapping_add(i as u16),
                *b,
            );
        }
    }
}
impl Ems {
    pub fn new() -> Self {
        let mut handles = [None; HANDLES];
        handles[0] = Some(NEW);
        Self {
            handles,
            owners: [UNMAPPED; PAGES],
            map: [UNMAPPED; 4],
            alt_area: None,
            os_key: None,
            os_enabled: true,
        }
    }
    fn handle(&self, id: u16) -> Result<Handle, u8> {
        self.handles
            .get(usize::from(id))
            .copied()
            .flatten()
            .ok_or(0x83)
    }
    fn physical(&self, pair: (u16, u16)) -> Result<Option<usize>, u8> {
        if pair.1 == EMPTY {
            return Ok(None);
        }
        self.handle(pair.0)?;
        let n = self.owners.iter().position(|p| *p == pair).ok_or(0x8a)?;
        Ok(Some(EXT_START + n * EMS_PAGE))
    }
    fn apply(&mut self, ram: &mut GuestRam, map: Map) -> Result<(), u8> {
        let mut physical = [None; 4];
        for (slot, page) in map.iter().enumerate() {
            physical[slot] = self.physical(*page)?;
        }
        ram.frame = physical;
        self.map = map;
        Ok(())
    }
    fn slot(value: u16, mode: u8) -> Result<usize, u8> {
        let index = match mode {
            0 => value,
            1 => {
                let delta = value.checked_sub((EMS_FRAME / 16) as u16).ok_or(0x8b)?;
                if delta % 0x400 != 0 {
                    return Err(0x8b);
                }
                delta / 0x400
            }
            _ => return Err(0x8f),
        };
        if index >= 4 {
            return Err(0x8b);
        }
        Ok(usize::from(index))
    }
    fn mapping(
        &self,
        ram: &GuestRam,
        src: Far,
        count: u16,
        id: u16,
        mode: u8,
        mut map: Map,
    ) -> Result<Map, u8> {
        if mode > 1 {
            return Err(0x8f);
        }
        if count > 4 {
            return Err(0x8b);
        }
        self.handle(id)?;
        for n in 0..count {
            let page = src.word(ram, n * 4);
            let slot = Self::slot(src.word(ram, n * 4 + 2), mode)?;
            let pair = if page == EMPTY { UNMAPPED } else { (id, page) };
            self.physical(pair)?;
            map[slot] = pair;
        }
        Ok(map)
    }
    fn save(map: Map, ram: &mut GuestRam, dst: Far) {
        for (i, (id, p)) in map.into_iter().enumerate() {
            let at = i as u16 * 4;
            dst.put(ram, at, &id.to_le_bytes());
            dst.put(ram, at + 2, &p.to_le_bytes());
        }
    }
    fn restore(&self, ram: &GuestRam, src: Far) -> Result<Map, u8> {
        let mut map = [UNMAPPED; 4];
        for (i, p) in map.iter_mut().enumerate() {
            let at = i as u16 * 4;
            *p = (src.word(ram, at), src.word(ram, at + 2));
            self.physical(*p).map_err(|_| 0xa3)?;
        }
        Ok(map)
    }
    /// A normal far return enters this guest BIOS stub. Its stack data belongs to
    /// the calling program, allowing nested mapped calls and independent sessions.
    pub fn return_call(&mut self, cpu: &mut CpuState, ram: &mut GuestRam) {
        let stack = Far {
            seg: cpu.selector(Segment::Stack),
            off: cpu.wide(Slot::Stack),
        };
        let return_to = stack.pointer(ram, 0);
        let map = Far {
            off: stack.off.wrapping_add(4),
            ..stack
        };
        let result = self.restore(ram, map).and_then(|m| self.apply(ram, m));
        cpu.set_wide(Slot::Stack, stack.off.wrapping_add(20));
        cpu.set_selector(Segment::Code, return_to.seg);
        cpu.set_instruction(return_to.off);
        cpu.set_high(Slot::Accumulator, result.err().unwrap_or(0));
    }
    fn resize(&mut self, ram: &mut GuestRam, id: u16, count: u16) -> Result<(), u8> {
        let old = self.handle(id)?.pages;
        if usize::from(count) > PAGES {
            return Err(0x87);
        }
        if usize::from(count.saturating_sub(old)) > ram.free_ems() {
            return Err(0x88);
        }
        if count < old {
            for (n, pair) in self.owners.iter_mut().enumerate() {
                if pair.0 == id && pair.1 >= count {
                    *pair = UNMAPPED;
                    ram.mark(EXT_START + n * EMS_PAGE, EMS_PAGE, false);
                }
            }
            let invalidate = |map: &mut Map| {
                for pair in map {
                    if pair.0 == id && pair.1 >= count {
                        *pair = UNMAPPED;
                    }
                }
            };
            let mut map = self.map;
            invalidate(&mut map);
            self.apply(ram, map)?;
            for h in self.handles.iter_mut().flatten() {
                if let Some(saved) = h.saved.as_mut() {
                    invalidate(saved);
                }
            }
        } else {
            for logical in old..count {
                let addr = ram
                    .allocate_ext(EMS_PAGE, true)
                    .expect("free aligned pages were counted");
                self.owners[(addr - EXT_START) / EMS_PAGE] = (id, logical);
            }
        }
        self.handles[usize::from(id)].as_mut().unwrap().pages = count;
        Ok(())
    }
    pub fn call(&mut self, cpu: &mut CpuState, ram: &mut GuestRam) {
        let status = self.service(cpu, ram).err().unwrap_or(0);
        cpu.set_high(Slot::Accumulator, status);
    }
    fn service(&mut self, cpu: &mut CpuState, ram: &mut GuestRam) -> Result<(), u8> {
        let ah = cpu.high(Slot::Accumulator);
        let al = cpu.low(Slot::Accumulator);
        let bx = cpu.wide(Slot::Base);
        let dx = cpu.wide(Slot::Data);
        let src = Far {
            seg: cpu.selector(Segment::Data),
            off: cpu.wide(Slot::Source),
        };
        let dst = Far {
            seg: cpu.selector(Segment::Extra),
            off: cpu.wide(Slot::Dest),
        };
        let frame = (EMS_FRAME / 16) as u16;
        if !self.os_enabled && (ah == 0x5b || (ah == 0x59 && al == 0)) {
            return Err(0xa4);
        }
        match ah {
            0x40 => {}
            0x41 => cpu.set_wide(Slot::Base, frame),
            0x42 | 0x59 if ah == 0x42 || al == 1 => {
                cpu.set_wide(Slot::Base, ram.free_ems() as u16);
                cpu.set_wide(Slot::Data, PAGES as u16);
            }
            0x43 | 0x5a => {
                if ah == 0x5a && al > 1 {
                    return Err(0x8f);
                }
                if bx == 0 && ah == 0x43 {
                    return Err(0x89);
                }
                let id = self.handles.iter().position(Option::is_none).ok_or(0x85)? as u16;
                self.handles[usize::from(id)] = Some(NEW);
                if let Err(e) = self.resize(ram, id, bx) {
                    self.handles[usize::from(id)] = None;
                    return Err(e);
                }
                cpu.set_wide(Slot::Data, id);
            }
            0x44 => {
                let mut map = self.map;
                let target = map.get_mut(usize::from(al)).ok_or(0x8b)?;
                *target = if bx == EMPTY { UNMAPPED } else { (dx, bx) };
                self.apply(ram, map)?;
            }
            0x45 => {
                let h = self.handle(dx)?;
                if dx == 0 {
                    return Err(0x83);
                }
                if h.saved.is_some() {
                    return Err(0x86);
                }
                self.resize(ram, dx, 0)?;
                self.handles[usize::from(dx)] = None;
            }
            0x46 => cpu.set_low(Slot::Accumulator, 0x40),
            0x47 => {
                let h = self.handle(dx)?;
                if h.saved.is_some() {
                    return Err(0x8d);
                }
                self.handles[usize::from(dx)].as_mut().unwrap().saved = Some(self.map);
            }
            0x48 => {
                let saved = self.handle(dx)?.saved.ok_or(0x8e)?;
                self.apply(ram, saved)?;
                self.handles[usize::from(dx)].as_mut().unwrap().saved = None;
            }
            0x4b => cpu.set_wide(Slot::Base, self.handles.iter().flatten().count() as u16),
            0x4c => cpu.set_wide(Slot::Base, self.handle(dx)?.pages),
            0x4d => {
                let mut count = 0u16;
                for (id, h) in self.handles.iter().enumerate() {
                    if let Some(h) = h {
                        dst.put(ram, count * 4, &(id as u16).to_le_bytes());
                        dst.put(ram, count * 4 + 2, &h.pages.to_le_bytes());
                        count += 1;
                    }
                }
                cpu.set_wide(Slot::Base, count);
            }
            0x4e => {
                if al == 3 {
                    cpu.set_low(Slot::Accumulator, 16);
                    return Ok(());
                }
                if al > 2 {
                    return Err(0x8f);
                }
                let new = if al != 0 {
                    Some(self.restore(ram, src)?)
                } else {
                    None
                };
                if al != 1 {
                    Self::save(self.map, ram, dst);
                }
                if let Some(map) = new {
                    self.apply(ram, map)?;
                }
            }
            0x4f => {
                if al > 2 {
                    return Err(0x8f);
                }
                let count = if al == 2 { bx } else { src.word(ram, 0) };
                if count > 4 {
                    return Err(0x8b);
                }
                if al == 2 {
                    cpu.set_low(Slot::Accumulator, (2 + count * 6) as u8);
                    return Ok(());
                }
                let mut entries = [(0u16, UNMAPPED); 4];
                let mut new = self.map;
                for i in 0..count {
                    let offset = 2 + i * (if al == 0 { 2 } else { 6 });
                    let seg = src.word(ram, offset);
                    let slot = Self::slot(seg, 1)?;
                    let pair = if al == 0 {
                        self.map[slot]
                    } else {
                        (src.word(ram, offset + 2), src.word(ram, offset + 4))
                    };
                    self.physical(pair).map_err(|_| 0xa3)?;
                    entries[usize::from(i)] = (seg, pair);
                    new[slot] = pair;
                }
                if al == 1 {
                    self.apply(ram, new)?;
                } else {
                    dst.put(ram, 0, &count.to_le_bytes());
                    for (i, (seg, (id, page))) in entries[..usize::from(count)].iter().enumerate() {
                        let n = 2 + i as u16 * 6;
                        dst.put(ram, n, &seg.to_le_bytes());
                        dst.put(ram, n + 2, &id.to_le_bytes());
                        dst.put(ram, n + 4, &page.to_le_bytes());
                    }
                }
            }
            0x50 => {
                let new = self.mapping(ram, src, cpu.wide(Slot::Count), dx, al, self.map)?;
                self.apply(ram, new)?;
            }
            0x51 => self.resize(ram, dx, bx)?,
            0x52 => match al {
                0 => {
                    self.handle(dx)?;
                    cpu.set_low(Slot::Accumulator, 0);
                }
                1 => {
                    self.handle(dx)?;
                    match bx as u8 {
                        0 => {}
                        1 => return Err(0x91),
                        _ => return Err(0x90),
                    }
                }
                2 => cpu.set_low(Slot::Accumulator, 0),
                _ => return Err(0x8f),
            },
            0x53 => {
                let h = self.handle(dx)?;
                match al {
                    0 => dst.put(ram, 0, &h.name),
                    1 => {
                        let name = core::array::from_fn(|i| src.byte(ram, i as u16));
                        if name != [0; 8]
                            && self.handles.iter().enumerate().any(|(i, h)| {
                                i != usize::from(dx) && h.is_some_and(|h| h.name == name)
                            })
                        {
                            return Err(0xa1);
                        }
                        self.handles[usize::from(dx)].as_mut().unwrap().name = name;
                    }
                    _ => return Err(0x8f),
                }
            }
            0x54 => match al {
                0 => {
                    let mut n = 0u16;
                    for (i, h) in self.handles.iter().enumerate() {
                        if let Some(h) = h {
                            dst.put(ram, n * 10, &(i as u16).to_le_bytes());
                            dst.put(ram, n * 10 + 2, &h.name);
                            n += 1;
                        }
                    }
                    cpu.set_low(Slot::Accumulator, n as u8);
                }
                1 => {
                    let name = core::array::from_fn(|i| src.byte(ram, i as u16));
                    if name == [0; 8] {
                        return Err(0xa0);
                    }
                    let id = self
                        .handles
                        .iter()
                        .position(|h| h.is_some_and(|h| h.name == name))
                        .ok_or(0xa0)?;
                    cpu.set_wide(Slot::Data, id as u16);
                }
                2 => cpu.set_wide(Slot::Base, HANDLES as u16),
                _ => return Err(0x8f),
            },
            0x55 | 0x56 => {
                if ah == 0x56 && al == 2 {
                    cpu.set_wide(Slot::Base, 24);
                    return Ok(());
                }
                let target = src.pointer(ram, 0);
                let new = self.mapping(
                    ram,
                    src.pointer(ram, 5),
                    u16::from(src.byte(ram, 4)),
                    dx,
                    al,
                    self.map,
                )?;
                if ah == 0x56 {
                    let old = self.mapping(
                        ram,
                        src.pointer(ram, 10),
                        u16::from(src.byte(ram, 9)),
                        dx,
                        al,
                        new,
                    )?;
                    let sp = cpu.wide(Slot::Stack).wrapping_sub(24);
                    let stack = Far {
                        seg: cpu.selector(Segment::Stack),
                        off: sp,
                    };
                    Self::save(
                        old,
                        ram,
                        Far {
                            off: sp.wrapping_add(8),
                            ..stack
                        },
                    );
                    stack.put(ram, 0, &RETURN.1.to_le_bytes());
                    stack.put(ram, 2, &RETURN.0.to_le_bytes());
                    stack.put(ram, 4, &cpu.instruction().to_le_bytes());
                    stack.put(ram, 6, &cpu.selector(Segment::Code).to_le_bytes());
                    cpu.set_wide(Slot::Stack, sp);
                }
                self.apply(ram, new)?;
                cpu.set_selector(Segment::Code, target.seg);
                cpu.set_instruction(target.off);
            }
            0x57 => self.move_region(ram, src, al)?,
            0x58 => {
                if al > 1 {
                    return Err(0x8f);
                }
                cpu.set_wide(Slot::Count, 4);
                if al == 0 {
                    for i in 0..4u16 {
                        dst.put(ram, i * 4, &(frame + i * 0x400).to_le_bytes());
                        dst.put(ram, i * 4 + 2, &i.to_le_bytes());
                    }
                }
            }
            0x59 if al == 0 => {
                for (i, w) in [1024u16, 0, 16, 0, 0].into_iter().enumerate() {
                    dst.put(ram, i as u16 * 2, &w.to_le_bytes());
                }
            }
            0x5b => match al {
                0 => {
                    cpu.set_low(Slot::Base, 0);
                    let addr = self.alt_area.unwrap_or(Far { seg: 0, off: 0 });
                    if self.alt_area.is_some() {
                        Self::save(self.map, ram, addr);
                    }
                    cpu.set_selector(Segment::Extra, addr.seg);
                    cpu.set_wide(Slot::Dest, addr.off);
                }
                1 => {
                    if bx as u8 != 0 {
                        return Err(0x9c);
                    }
                    let new = self.restore(ram, dst)?;
                    self.apply(ram, new)?;
                    self.alt_area = Some(dst);
                }
                2 => cpu.set_wide(Slot::Data, 16),
                3 | 5 => cpu.set_low(Slot::Base, 0),
                4 | 8 if bx as u8 == 0 => {}
                4 | 8 => return Err(0x9c),
                6 | 7 => return Err(if bx as u8 == 0 { 0x9e } else { 0x9c }),
                _ => return Err(0x8f),
            },
            0x5d => {
                if al > 2 {
                    return Err(0x8f);
                }
                let supplied = u32::from(bx) | (u32::from(cpu.wide(Slot::Count)) << 16);
                if let Some(key) = self.os_key {
                    if key != supplied {
                        return Err(0xa4);
                    }
                } else if al == 2 {
                    return Err(0xa4);
                } else {
                    let key = crate::arch::platform::session_nonce();
                    self.os_key = Some(key);
                    cpu.set_wide(Slot::Base, key as u16);
                    cpu.set_wide(Slot::Count, (key >> 16) as u16);
                }
                if al == 2 {
                    self.os_key = None;
                    self.os_enabled = true;
                } else {
                    self.os_enabled = al == 0;
                }
            }
            0x5c => {} // All exposed memory is volatile; the next boot clears it.
            _ => return Err(0x84),
        }
        Ok(())
    }
    fn move_region(&self, ram: &mut GuestRam, src: Far, operation: u8) -> Result<(), u8> {
        if operation > 1 {
            return Err(0x8f);
        }
        let len = usize::from(src.word(ram, 0)) | (usize::from(src.word(ram, 2)) << 16);
        if len > 0x100000 {
            return Err(0x96);
        }
        let parse = |at| {
            let off = usize::from(src.word(ram, at + 3));
            let seg = usize::from(src.word(ram, at + 5));
            match src.byte(ram, at) {
                0 => {
                    let address = seg * 16 + off;
                    if address + len > 0x100000 {
                        return Err(0xa2);
                    }
                    Ok((None, address))
                }
                1 => {
                    let id = src.word(ram, at + 1);
                    let h = self.handle(id)?;
                    if seg >= usize::from(h.pages) {
                        return Err(0x8a);
                    }
                    if off >= EMS_PAGE {
                        return Err(0x95);
                    }
                    let address = seg * EMS_PAGE + off;
                    if address + len > usize::from(h.pages) * EMS_PAGE {
                        return Err(0x93);
                    }
                    Ok((Some(id), address))
                }
                _ => Err(0x98),
            }
        };
        let (from, a) = parse(4)?;
        let (to, b) = parse(11)?;
        let overlap = from == to && a.abs_diff(b) < len;
        if overlap && operation == 1 {
            return Err(0x97);
        }
        let locate = |id: Option<u16>, address: usize| {
            if let Some(id) = id {
                self.physical((id, (address / EMS_PAGE) as u16))
                    .unwrap()
                    .unwrap()
                    + address % EMS_PAGE
            } else {
                address
            }
        };
        let backward = overlap && b > a;
        let mut remaining = len;
        while remaining > 0 {
            let offset = if backward {
                remaining - 1
            } else {
                len - remaining
            };
            let sa = locate(from, a + offset);
            let da = locate(to, b + offset);
            let available = |x: usize| {
                if backward {
                    x % EMS_PAGE + 1
                } else {
                    EMS_PAGE - x % EMS_PAGE
                }
            };
            let chunk = remaining.min(available(sa)).min(available(da));
            for n in 0..chunk {
                let (s, d) = if backward {
                    (sa - n, da - n)
                } else {
                    (sa + n, da + n)
                };
                let v = ram.bus_byte(s);
                if operation == 1 {
                    let old = ram.bus_byte(d);
                    ram.set_bus_byte(s, old);
                }
                ram.set_bus_byte(d, v);
            }
            remaining -= chunk;
        }
        if overlap && a != b {
            return Err(0x92);
        }
        Ok(())
    }
}

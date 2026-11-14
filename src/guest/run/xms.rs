//! Session-local XMS 3.0 manager. Addresses and handles are always guest values.
use super::{CpuState, GuestRam, Segment, Slot};
use crate::guest::memory::EXT_START as START;
mod umb;
const BLOCKS: usize = 32;
pub(in crate::guest) const ENTRY: (u16, u16) = (0xf000, 0x0100);
#[derive(Clone, Copy)]
struct Block {
    start: usize,
    len: usize,
    locks: u8,
}
pub(super) struct Xms {
    blocks: [Option<Block>; BLOCKS],
    umb: umb::Upper,
    hma: bool,
    global: bool,
    local: u16,
}
impl Xms {
    pub fn new() -> Self {
        Self {
            blocks: [None; BLOCKS],
            umb: umb::Upper::new(),
            hma: false,
            global: false,
            local: 0,
        }
    }
    fn index(&self, handle: u16) -> Result<usize, u8> {
        usize::from(handle)
            .checked_sub(1)
            .filter(|i| self.blocks.get(*i).is_some_and(Option::is_some))
            .ok_or(0xa2)
    }
    fn range(&self, handle: u16, offset: u32, len: usize, ram: &GuestRam) -> Option<usize> {
        if handle == 0 {
            let start = ram.address((offset >> 16) as u16, offset as u16);
            return (start.checked_add(len)? <= START).then_some(start);
        }
        let b = self.blocks[self.index(handle).ok()?]?;
        ((offset as usize).checked_add(len)? <= b.len).then_some(b.start + offset as usize)
    }
    pub fn call(&mut self, cpu: &mut CpuState, ram: &mut GuestRam) {
        let function = cpu.high(Slot::Accumulator);
        let dx = cpu.wide(Slot::Data);
        let result = (|| -> Result<(), u8> {
            match function {
                0 => {
                    cpu.set_wide(Slot::Accumulator, 0x0300);
                    cpu.set_wide(Slot::Base, 0x0300);
                    cpu.set_wide(Slot::Data, 1);
                    return Ok(());
                }
                1 => {
                    if self.hma {
                        return Err(0x91);
                    }
                    self.hma = true;
                }
                2 => {
                    if !self.hma {
                        return Err(0x93);
                    }
                    self.hma = false;
                }
                3..=6 => {
                    match function {
                        3 => self.global = true,
                        4 => self.global = false,
                        5 => self.local = self.local.checked_add(1).ok_or(0x82)?,
                        _ => self.local = self.local.checked_sub(1).ok_or(0x82)?,
                    };
                    ram.a20 = self.global || self.local != 0;
                }
                7 => {
                    cpu.set_wide(Slot::Accumulator, u16::from(ram.a20));
                    cpu.set_low(Slot::Base, 0);
                    return Ok(());
                }
                8 | 0x88 => {
                    let (mut max, mut total) = (0, 0);
                    ram.free_runs(|_, len| {
                        max = max.max(len);
                        total += len
                    });
                    if function == 0x88 {
                        cpu.set_dword(Slot::Accumulator, (max / 1024) as u32);
                        cpu.set_dword(Slot::Data, (total / 1024) as u32);
                        cpu.set_dword(Slot::Count, (super::super::memory::RAM_BYTES - 1) as u32);
                    } else {
                        cpu.set_wide(Slot::Accumulator, (max / 1024) as u16);
                        cpu.set_wide(Slot::Data, (total / 1024) as u16);
                    }
                    cpu.set_low(Slot::Base, if total == 0 { 0xa0 } else { 0 });
                    return Ok(());
                }
                9 | 0x89 => {
                    let i = self.blocks.iter().position(Option::is_none).ok_or(0xa1)?;
                    let size = if function == 0x89 {
                        cpu.dword(Slot::Data)
                    } else {
                        u32::from(dx)
                    };
                    let len = (size as usize).checked_mul(1024).ok_or(0xa0)?;
                    let pos = ram.allocate_ext(len, false);
                    self.blocks[i] = Some(Block {
                        start: pos.ok_or(0xa0)?,
                        len,
                        locks: 0,
                    });
                    cpu.set_wide(Slot::Data, (i + 1) as u16);
                }
                10 => {
                    let i = self.index(dx)?;
                    let block = self.blocks[i].unwrap();
                    if block.locks != 0 {
                        return Err(0xab);
                    }
                    ram.mark(block.start, block.len, false);
                    self.blocks[i] = None;
                }
                11 => {
                    let mut raw = [0; 16];
                    for (n, b) in raw.iter_mut().enumerate() {
                        *b = ram
                            .load(
                                cpu.selector(Segment::Data),
                                cpu.wide(Slot::Source).wrapping_add(n as u16),
                            )
                            .ok_or(0xa7)?;
                    }
                    let u16at = |n| u16::from_le_bytes([raw[n], raw[n + 1]]);
                    let u32at = |n| u32::from_le_bytes(raw[n..n + 4].try_into().unwrap());
                    let len = u32at(0) as usize;
                    if len % 2 != 0 {
                        return Err(0xa7);
                    }
                    if u16at(4) != 0 {
                        self.index(u16at(4)).map_err(|_| 0xa3)?;
                    }
                    if u16at(10) != 0 {
                        self.index(u16at(10)).map_err(|_| 0xa5)?;
                    }
                    let src = self.range(u16at(4), u32at(6), len, ram).ok_or(0xa4)?;
                    let dst = self.range(u16at(10), u32at(12), len, ram).ok_or(0xa6)?;
                    if dst > src && dst - src < len {
                        return Err(0xa8);
                    }
                    ram.move_physical(src, dst, len).ok_or(0xa7)?;
                }
                12 | 13 => {
                    let i = self.index(dx)?;
                    let b = self.blocks[i].as_mut().unwrap();
                    if function == 12 {
                        b.locks = b.locks.checked_add(1).ok_or(0xac)?;
                        cpu.set_wide(Slot::Base, b.start as u16);
                        cpu.set_wide(Slot::Data, (b.start >> 16) as u16);
                        cpu.set_wide(Slot::Accumulator, 1);
                        return Ok(());
                    }
                    b.locks = b.locks.checked_sub(1).ok_or(0xaa)?;
                }
                14 | 0x8e => {
                    let b = self.blocks[self.index(dx)?].unwrap();
                    cpu.set_high(Slot::Base, b.locks);
                    cpu.set_low(
                        Slot::Base,
                        self.blocks.iter().filter(|b| b.is_none()).count() as u8,
                    );
                    if function == 0x8e {
                        cpu.set_dword(Slot::Data, (b.len / 1024) as u32);
                        cpu.set_wide(
                            Slot::Count,
                            self.blocks.iter().filter(|b| b.is_none()).count() as u16,
                        );
                    } else {
                        cpu.set_wide(Slot::Data, (b.len / 1024) as u16);
                    }
                    cpu.set_wide(Slot::Accumulator, 1);
                    return Ok(());
                }
                15 | 0x8f => {
                    let i = self.index(dx)?;
                    let b = self.blocks[i].unwrap();
                    if b.locks != 0 {
                        return Err(0xab);
                    }
                    let size = if function == 0x8f {
                        cpu.dword(Slot::Base)
                    } else {
                        u32::from(cpu.wide(Slot::Base))
                    };
                    let len = (size as usize).checked_mul(1024).ok_or(0xa0)?;
                    ram.mark(b.start, b.len, false);
                    let Some(start) = ram.allocate_ext(len, false) else {
                        ram.mark(b.start, b.len, true);
                        return Err(0xa0);
                    };
                    ram.move_physical(b.start, start, b.len.min(len)).unwrap();
                    self.blocks[i] = Some(Block {
                        start,
                        len,
                        locks: 0,
                    });
                }
                0x10..=0x12 => {
                    self.umb.call(cpu, function)?;
                    cpu.set_wide(Slot::Accumulator, 1);
                    return Ok(());
                }
                _ => return Err(0x80),
            }
            cpu.set_wide(Slot::Accumulator, 1);
            cpu.set_low(Slot::Base, 0);
            Ok(())
        })();
        if let Err(code) = result {
            cpu.set_wide(Slot::Accumulator, 0);
            cpu.set_low(Slot::Base, code);
        }
    }
}

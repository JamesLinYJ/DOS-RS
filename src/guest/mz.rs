//! Validate an MZ load module before reserving RAM or applying relocations.

use super::memory::GuestRam;
use super::state::{CpuState, Segment, Slot};

pub(super) struct Mz<'a> {
    payload: &'a [u8],
    relocations: &'a [u8],
    cs: u16,
    ip: u16,
    ss: u16,
    sp: u16,
    pub minimum: u16,
    pub maximum: u16,
}

impl<'a> Mz<'a> {
    pub fn parse(file: &'a [u8]) -> Option<Self> {
        let word = |at: usize| Some(u16::from_le_bytes(file.get(at..at + 2)?.try_into().ok()?));
        if !file.starts_with(b"MZ") {
            return None;
        }
        let last = usize::from(word(2)?);
        let pages = usize::from(word(4)?);
        if last > 511 || pages == 0 {
            return None;
        }
        let length = (pages - 1) * 512 + if last == 0 { 512 } else { last };
        let header = usize::from(word(8)?) * 16;
        if header < 28 {
            return None;
        }
        let payload = file.get(header..length)?;
        let count = usize::from(word(6)?);
        let at = usize::from(word(24)?);
        let relocations = if count == 0 {
            &[]
        } else {
            if at < 28 {
                return None;
            }
            file.get(..header)?.get(at..at + count * 4)?
        };
        for reloc in relocations.chunks_exact(4) {
            let at = relocation_offset(reloc);
            payload.get(at..at + 2)?;
        }
        let (cs, ip, ss, sp) = (word(22)?, word(20)?, word(14)?, word(16)?);
        let entry = (usize::from(cs) * 16 + usize::from(ip)) & 0xFFFFF;
        payload.get(entry)?;
        let base = 0x10 + payload.len().div_ceil(16);
        let stack_end = 0x10
            + usize::from(ss)
            + if sp == 0 {
                0x1000
            } else {
                usize::from(sp).div_ceil(16)
            };
        let minimum = u16::try_from((base + usize::from(word(10)?)).max(stack_end)).ok()?;
        let maximum = u16::try_from(base + usize::from(word(12)?))
            .unwrap_or(u16::MAX)
            .max(minimum);
        Some(Self {
            payload,
            relocations,
            cs,
            ip,
            ss,
            sp,
            minimum,
            maximum,
        })
    }

    pub fn load(&self, ram: &mut GuestRam, psp: u16, end: u16) -> CpuState {
        let load = psp + 0x10;
        let bytes = ram.paragraphs_mut(load, end);
        bytes[..self.payload.len()].copy_from_slice(self.payload);
        for reloc in self.relocations.chunks_exact(4) {
            let i = relocation_offset(reloc);
            let word = u16::from_le_bytes([bytes[i], bytes[i + 1]]).wrapping_add(load);
            bytes[i..i + 2].copy_from_slice(&word.to_le_bytes());
        }
        let mut cpu = CpuState::at_com(psp);
        cpu.set_instruction(self.ip);
        cpu.set_selector(Segment::Code, load.wrapping_add(self.cs));
        cpu.set_selector(Segment::Stack, load.wrapping_add(self.ss));
        cpu.set_wide(Slot::Stack, self.sp);
        cpu
    }
}

fn relocation_offset(entry: &[u8]) -> usize {
    usize::from(u16::from_le_bytes([entry[2], entry[3]])) * 16
        + usize::from(u16::from_le_bytes([entry[0], entry[1]]))
}

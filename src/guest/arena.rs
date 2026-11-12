//! Conventional-memory ownership. All bounds are paragraph segments, end exclusive.

use super::memory::{CONVENTIONAL_END, GuestRam};

pub(super) const FIRST_MCB: u16 = 0xa0;

#[derive(Clone, Copy, Default)]
struct Block {
    start: u16,
    end: u16,
    owner: u16,
}

#[derive(Debug)]
pub(super) enum MemoryError {
    InvalidBlock,
    NoRoom(u16),
}

/// Only occupied ranges are stored. Removing a block joins the adjacent free ranges.
pub(super) struct Arena {
    blocks: [Block; 128],
    used: usize,
}

impl Arena {
    pub fn new() -> Self {
        Self {
            blocks: [Block::default(); 128],
            used: 0,
        }
    }

    /// Find a contiguous gap using the DOS first, best or last fit strategy.
    fn gap(&self, floor: u16, size: u16, strategy: u8) -> Result<(u16, u16), MemoryError> {
        let mut start = floor;
        let mut largest = 0;
        let mut chosen = None;
        for (next_start, next_end) in self.blocks[..self.used]
            .iter()
            .map(|b| (b.start - 1, b.end.saturating_add(1)))
            .chain(core::iter::once((CONVENTIONAL_END, CONVENTIONAL_END)))
        {
            let room = next_start.saturating_sub(start);
            largest = largest.max(room);
            if size > 0 && room >= size {
                match strategy {
                    0 => return Ok((start, room)),
                    1 if chosen.is_none_or(|(_, old)| room < old) => chosen = Some((start, room)),
                    2 => chosen = Some((next_start - size, size)),
                    _ => {}
                }
            }
            start = start.max(next_end);
        }
        chosen.ok_or(MemoryError::NoRoom(largest))
    }

    fn insert(&mut self, block: Block) {
        let index = self.blocks[..self.used].partition_point(|b| b.start < block.start);
        self.blocks.copy_within(index..self.used, index + 1);
        self.blocks[index] = block;
        self.used += 1;
    }

    /// Allocate the private environment immediately below the PSP, then the PSP,
    /// image and initial stack together. The two blocks can be released separately.
    pub fn process(
        &mut self,
        preferred_psp: u16,
        env: u16,
        min: u16,
        max: u16,
    ) -> Result<(u16, u16), MemoryError> {
        if self.used + 2 > self.blocks.len() {
            return Err(MemoryError::NoRoom(0));
        }
        let total = min
            .checked_add(env)
            .and_then(|n| n.checked_add(1))
            .ok_or(MemoryError::NoRoom(0))?;
        let floor = preferred_psp
            .checked_sub(env)
            .and_then(|n| n.checked_sub(1))
            .filter(|s| *s > FIRST_MCB)
            .ok_or(MemoryError::NoRoom(0))?;
        let (base, room) = self.gap(floor, total, 0)?;
        let psp = base + env + 1;
        let end = psp + max.min(room - env - 1);
        self.insert(Block {
            start: base,
            end: psp - 1,
            owner: psp,
        });
        self.insert(Block {
            start: psp,
            end,
            owner: psp,
        });
        Ok((psp, end))
    }

    pub fn alloc(&mut self, owner: u16, size: u16, strategy: u8) -> Result<u16, MemoryError> {
        if self.used == self.blocks.len() {
            return Err(MemoryError::NoRoom(0));
        }
        let (start, _) = self.gap(0x0C00, size, strategy)?;
        self.insert(Block {
            start,
            end: start + size,
            owner,
        });
        Ok(start)
    }

    fn locate(&self, owner: u16, start: u16) -> Result<usize, MemoryError> {
        self.blocks[..self.used]
            .iter()
            .position(|b| b.start == start && b.owner == owner)
            .ok_or(MemoryError::InvalidBlock)
    }

    pub fn free(&mut self, owner: u16, start: u16) -> Result<(), MemoryError> {
        let i = self.locate(owner, start)?;
        if start == owner {
            return Err(MemoryError::InvalidBlock);
        }
        self.blocks.copy_within(i + 1..self.used, i);
        self.used -= 1;
        Ok(())
    }

    pub fn resize(&mut self, owner: u16, start: u16, size: u16) -> Result<u16, MemoryError> {
        let i = self.locate(owner, start)?;
        let next = self.blocks[..self.used]
            .get(i + 1)
            .map_or(CONVENTIONAL_END, |b| b.start - 1);
        let room = next - start;
        if size == 0 || size > room || (start == owner && size < 0x10) {
            return Err(MemoryError::NoRoom(room));
        }
        let end = start + size;
        self.blocks[i].end = end;
        Ok(end)
    }

    /// Publish DOS M/Z headers in the paragraphs reserved by the allocator.
    pub fn publish(&self, ram: &mut GuestRam) {
        let mut next = FIRST_MCB;
        for block in &self.blocks[..self.used] {
            let header = block.start - 1;
            if next < header {
                Self::header(ram, next, header, 0);
            }
            Self::header(ram, header, block.end, block.owner);
            next = block.end;
        }
        if next < CONVENTIONAL_END {
            Self::header(ram, next, CONVENTIONAL_END, 0);
        }
    }

    fn header(ram: &mut GuestRam, at: u16, end: u16, owner: u16) {
        let bytes = ram.paragraphs_mut(at, at + 1);
        bytes[0] = if end == CONVENTIONAL_END { b'Z' } else { b'M' };
        bytes[1..3].copy_from_slice(&owner.to_le_bytes());
        bytes[3..5].copy_from_slice(&(end - at - 1).to_le_bytes());
    }

    pub fn release_process(&mut self, owner: u16) {
        let mut kept = 0;
        for i in 0..self.used {
            if self.blocks[i].owner != owner {
                self.blocks[kept] = self.blocks[i];
                kept += 1;
            }
        }
        self.used = kept;
    }
}

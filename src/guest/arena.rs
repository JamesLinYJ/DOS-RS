//! DOS conventional and upper memory ownership. All bounds are paragraph segments, end exclusive.

use super::memory::{CONVENTIONAL_END, GuestRam};

pub(super) const FIRST_MCB: u16 = 0xa0;
pub(super) const UMB_ROOT: u16 = CONVENTIONAL_END - 1;
const LOW: (u16, u16) = (0x0c00, UMB_ROOT);
// Segment intervals for data. Each has room for its first MCB and the
// reserved link over the following video ROM / EMS window.
const UPPER: [(u16, u16); 2] = [(0xc801, 0xcfff), (0xe001, 0xf000)];
pub(super) const XMS_OWNER: u16 = 0xfffe;

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
    pub upper_linked: bool,
}

impl Arena {
    pub fn new() -> Self {
        Self {
            blocks: [Block::default(); 128],
            used: 0,
            upper_linked: false,
        }
    }

    /// Find a contiguous gap using the DOS first, best or last fit strategy.
    fn gap(&self, regions: &[(u16, u16)], size: u16, fit: u8) -> Result<(u16, u16), MemoryError> {
        let mut largest = 0;
        let mut chosen = None;
        for &(floor, end) in regions {
            let mut start = floor;
            for (next, after) in self.blocks[..self.used]
                .iter()
                .filter(|b| b.end >= floor && b.start < end)
                .map(|b| (b.start - 1, b.end + 1))
                .chain(core::iter::once((end, end)))
            {
                let room = next.saturating_sub(start);
                largest = largest.max(room);
                if size > 0 && room >= size {
                    match fit {
                        0 => return Ok((start, room)),
                        1 if chosen.is_none_or(|(_, old)| room < old) => {
                            chosen = Some((start, room))
                        }
                        2 => chosen = Some((next - size, size)),
                        _ => {}
                    }
                }
                start = start.max(after);
            }
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
        let (base, room) = self.gap(&[(floor, UMB_ROOT)], total, 0)?;
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

    /// Reserve the largest available upper workspace for a DEVICEHIGH INIT call.
    /// Fall back to conventional memory if the driver cannot fit there.
    pub fn driver(&mut self, high: bool, minimum: usize) -> Result<(u16, u16), MemoryError> {
        const DOS: u16 = 8;
        if minimum > 0x1000 || self.used == self.blocks.len() {
            return Err(MemoryError::NoRoom(0));
        }
        if high
            && let Err(MemoryError::NoRoom(room)) = self.gap(&UPPER, u16::MAX, 0)
            && usize::from(room) >= minimum
            && room != 0
        {
            let (start, _) = self.gap(&UPPER, room, 0)?;
            self.insert(Block {
                start,
                end: start + room,
                owner: DOS,
            });
            return Ok((start, room));
        }
        self.alloc(DOS, 0x1000, 0).map(|start| (start, 0x1000))
    }

    pub fn alloc(&mut self, owner: u16, size: u16, strategy: u8) -> Result<u16, MemoryError> {
        if self.used == self.blocks.len() {
            return Err(MemoryError::NoRoom(0));
        }
        let fit = strategy & 3;
        let upper = self.upper_linked || owner == XMS_OWNER;
        let (start, _) = match (upper, strategy & 0xc0) {
            (true, 0x40) => self.gap(&UPPER, size, fit),
            (true, 0x80) => self
                .gap(&UPPER, size, fit)
                .or_else(|_| self.gap(&[LOW, UPPER[0], UPPER[1]], size, fit)),
            (true, _) => self.gap(&[LOW, UPPER[0], UPPER[1]], size, fit),
            (false, _) => self.gap(&[LOW], size, fit),
        }?;
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
        let limit = UPPER
            .iter()
            .find(|&&(lo, hi)| (lo..hi).contains(&start))
            .map_or(UMB_ROOT, |&(_, hi)| hi);
        let next = self.blocks[..self.used]
            .get(i + 1)
            .map_or(limit, |b| limit.min(b.start - 1));
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
        self.publish_region(ram, FIRST_MCB, UMB_ROOT, !self.upper_linked);
        Self::header(ram, UMB_ROOT, UPPER[0].0 - 1, 8, false);
        self.publish_region(ram, UPPER[0].0 - 1, UPPER[0].1, false);
        Self::header(ram, UPPER[0].1, UPPER[1].0 - 1, 8, false);
        self.publish_region(ram, UPPER[1].0 - 1, UPPER[1].1, true);
    }

    fn publish_region(&self, ram: &mut GuestRam, first: u16, end: u16, last: bool) {
        let mut next = first;
        for block in self.blocks[..self.used]
            .iter()
            .filter(|b| first < b.start && b.start < end)
        {
            let header = block.start - 1;
            if next < header {
                Self::header(ram, next, header, 0, false);
            }
            Self::header(
                ram,
                header,
                block.end,
                block.owner,
                last && block.end == end,
            );
            next = block.end;
        }
        if next < end {
            Self::header(ram, next, end, 0, last);
        }
    }

    fn header(ram: &mut GuestRam, at: u16, end: u16, owner: u16, last: bool) {
        let bytes = ram.paragraphs_mut(at, at + 1);
        bytes[0] = if last { b'Z' } else { b'M' };
        let owner = if owner == XMS_OWNER { 8 } else { owner };
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

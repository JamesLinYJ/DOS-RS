//! Paragraph-granular contiguous UMB allocation within a verified option-ROM gap.
pub(super) const XMS_OWNER: u16 = 0xfffe;
pub enum MemoryError {
    NoRoom(u16),
    InvalidBlock,
}
#[derive(Copy, Clone)]
struct Block {
    start: u16,
    size: u16,
    owner: u16,
}
pub struct Arena {
    first: u16,
    end: u16,
    linked: bool,
    strategy: u16,
    blocks: [Block; 32],
}
impl Arena {
    pub const fn new() -> Self {
        Self {
            first: 0,
            end: 0,
            linked: false,
            strategy: 0,
            blocks: [Block {
                start: 0,
                size: 0,
                owner: 0,
            }; 32],
        }
    }
    pub fn init(&mut self, first: u16, end: u16) {
        self.first = first;
        self.end = end
    }
    fn room(&self, at: u16) -> u16 {
        self.blocks
            .iter()
            .filter(|b| b.size > 0 && b.start >= at)
            .map(|b| b.start - 1)
            .min()
            .unwrap_or(self.end)
            .saturating_sub(at)
    }
    fn free_spans(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        core::iter::once(self.first)
            .chain(
                self.blocks
                    .iter()
                    .filter(|b| b.size != 0)
                    .map(|b| b.start + b.size),
            )
            .map(|at| (at, self.room(at).saturating_sub(1)))
    }

    fn set_owner(&mut self, start: u16, old: u16, new: u16) -> Result<(), MemoryError> {
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.size != 0 && b.start == start && b.owner == old)
            .ok_or(MemoryError::InvalidBlock)?;
        block.owner = new;
        Ok(())
    }

    fn owns(&self, owner: u16, first: u16, last: u16) -> bool {
        first <= last
            && self.blocks.iter().any(|b| {
                b.owner == owner && b.size != 0 && first >= b.start && last < b.start + b.size
            })
    }

    fn executable(&mut self, minimum: u16, maximum: u16, owner: u16) -> Result<u16, MemoryError> {
        let largest = self.free_spans().map(|(_, n)| n).max().unwrap_or(0);
        if largest < minimum || !self.linked {
            return Err(MemoryError::NoRoom(largest));
        }
        let seg = self.alloc(owner, largest.min(maximum), self.strategy as u8)?;
        self.set_owner(seg, owner, seg)?;
        Ok(seg)
    }

    pub fn alloc(&mut self, owner: u16, size: u16, strategy: u8) -> Result<u16, MemoryError> {
        let mut largest = 0;
        let mut chosen = None;
        let mut best = u16::MAX;
        for (at, room) in self.free_spans() {
            largest = largest.max(room);
            if size != 0 && room >= size {
                let dest = if strategy & 3 == 2 {
                    at + 1 + room - size
                } else {
                    at + 1
                };
                let better = match strategy & 3 {
                    1 => room < best,
                    2 => chosen.is_none_or(|p| dest > p),
                    _ => chosen.is_none_or(|p| dest < p),
                };
                if chosen.is_none() || better {
                    chosen = Some(dest);
                    best = room
                }
            }
        }
        let at = chosen.ok_or(MemoryError::NoRoom(largest))?;
        let b = self
            .blocks
            .iter_mut()
            .find(|b| b.size == 0)
            .ok_or(MemoryError::NoRoom(0))?;
        *b = Block {
            start: at,
            size,
            owner,
        };
        clear(at, size);
        Ok(at)
    }
    pub fn free(&mut self, owner: u16, at: u16) -> Result<(), MemoryError> {
        let b = self
            .blocks
            .iter_mut()
            .find(|b| b.size != 0 && b.start == at && b.owner == owner)
            .ok_or(MemoryError::InvalidBlock)?;
        b.size = 0;
        Ok(())
    }
    pub fn resize(&mut self, owner: u16, at: u16, size: u16) -> Result<u16, MemoryError> {
        let i = self
            .blocks
            .iter()
            .position(|b| b.size != 0 && b.start == at && b.owner == owner)
            .ok_or(MemoryError::InvalidBlock)?;
        let old = self.blocks[i].size;
        let largest = old + self.room(at + old);
        if size == 0 || size > largest {
            return Err(MemoryError::NoRoom(largest));
        }
        if size > old {
            clear(at + old, size - old)
        }
        self.blocks[i].size = size;
        Ok(size)
    }
    pub fn publish(&self, _ram: &mut super::GuestRam) {
        self.headers();
    }
    fn headers(&self) {
        let bridge = super::layout::arena_limit(super::word(0x413));
        let mut at = self.first;
        if self.linked && at != 0 {
            header(bridge, 8, at - bridge - 1, false)
        } else {
            header(bridge, 8, 0, true)
        }
        while at < self.end {
            let block = self
                .blocks
                .iter()
                .filter(|b| b.size != 0 && b.start > at)
                .min_by_key(|b| b.start);
            let next = block.map_or(self.end, |b| b.start - 1);
            if next > at {
                header(at, 0, next - at - 1, next == self.end);
            }
            let Some(b) = block else { break };
            at = b.start + b.size;
            header(
                b.start - 1,
                if b.owner == XMS_OWNER { 8 } else { b.owner },
                b.size,
                at == self.end,
            );
        }
    }
}

fn clear(seg: u16, size: u16) {
    unsafe {
        core::ptr::write_bytes(
            super::address(u32::from(seg) * 16),
            0,
            usize::from(size) * 16,
        )
    }
}

pub fn map(ram: &mut super::GuestRam, arena: &mut Arena) {
    // C8000h-CFFFFh is optional adapter space. Preserve every nonempty page and
    // every option ROM, including a VGA BIOS that extends past C8000h.
    let mut rom_end = 0xc0000;
    let mut first = 0;
    let mut last = 0;
    for page in (0xc0000..0xd0000).step_by(4096) {
        for at in [page, page + 2048] {
            if super::word(at) == 0xaa55 {
                rom_end = rom_end.max(at + 512 * u32::from(super::get(at + 2)));
            }
        }
        if page < 0xc8000
            || page < rom_end
            || (0..4096).any(|i| !matches!(super::get(page + i), 0 | 255))
        {
            if first != 0 {
                break;
            }
            continue;
        }
        let Some(mem) = ram.allocate_ext(4096) else {
            break;
        };
        // All earlier reservations here start on a 4KiB boundary.
        ram.zero_ext(mem, 4096);
        super::set_pte(page / 4096, mem as u32 | 7);
        if first == 0 {
            first = page
        }
        last = page + 4096;
    }
    super::flush();
    arena.init((first >> 4) as u16, (last >> 4) as u16);
}

// The kernel calls before searching low memory and again if that search fails.
// AX=0 delegates to conventional DOS; 1 returns DX; other values are DOS errors.
impl Arena {
    pub fn dos(&mut self, f: &mut super::Frame) {
        let owner = super::current_psp();
        let size = f.bx as u16;
        let target = f.dx as u16;
        let phase = f.si as u16;
        let result: Result<Option<u16>, MemoryError> = match f.cx as u16 {
            0x5800 => Ok(Some(self.strategy)),
            0x5801 if size & !0xc3 == 0 && size & 3 < 3 && size & 0xc0 != 0xc0 => {
                self.strategy = size;
                Ok(Some(0))
            }
            0x5802 => Ok(Some(self.linked as u16)),
            0x5803 if size < 2 => {
                self.linked = size != 0;
                Ok(Some(0))
            }
            0x5800..=0x58ff => {
                f.ax = 1 + 0x100;
                f.dx = 0;
                return;
            }
            0x4800
                if self.linked
                    && ((phase == 0 && self.strategy >= 0x40)
                        || (phase == 1 && self.strategy < 0x40)) =>
            {
                let r = self.alloc(owner, size, self.strategy as u8);
                match r {
                    Ok(at) => Ok(Some(at)),
                    Err(_) if self.strategy >= 0x80 => Ok(None),
                    Err(e) => Err(e),
                }
            }
            0x4900 | 0x4a00 if target >= self.first && target <= self.end && target != 0 => {
                if f.cx as u16 == 0x4900 {
                    self.free(owner, target).map(|_| Some(0))
                } else {
                    self.resize(owner, target, size).map(|_| Some(0))
                }
            }
            super::layout::UMB_EXEC => self.executable(size, target, owner).map(Some),
            super::layout::UMB_SET_OWNER => self.set_owner(target, owner, size).map(|_| Some(0)),
            super::layout::UMB_OWNS_RANGE => Ok(Some(u16::from(self.owns(owner, target, size)))),
            0xf000 => {
                let owner = if target == 0 { owner } else { target };
                for b in &mut self.blocks {
                    if b.owner == owner {
                        b.size = 0
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        };
        if result.is_ok()
            && matches!(
                f.cx as u16,
                0x5803
                    | 0x4800
                    | 0x4900
                    | 0x4a00
                    | 0xf000
                    | super::layout::UMB_EXEC
                    | super::layout::UMB_SET_OWNER
            )
        {
            self.headers();
        }
        (f.ax, f.dx) = match result {
            Ok(None) => (0, 0),
            Ok(Some(v)) => (1, u32::from(v)),
            Err(MemoryError::InvalidBlock) => (9, 0),
            Err(MemoryError::NoRoom(n)) => (8, u32::from(n)),
        };
    }
}
fn header(seg: u16, owner: u16, size: u16, last: bool) {
    let at = u32::from(seg) * 16;
    super::put(at, if last { b'Z' } else { b'M' });
    super::write32(at + 1, u32::from(owner) | u32::from(size) << 16);
}

//! Guest-only upper memory. The VGA ROM, EMS frame and system BIOS are reserved.
use super::{CpuState, Slot};

pub(super) struct Upper {
    blocks: [Option<(u16, u16)>; 32],
}
const REGIONS: [(u16, u16); 2] = [(0xc800, 0xd000), (0xe000, 0xf000)];
impl Upper {
    pub fn new() -> Self {
        Self { blocks: [None; 32] }
    }
    fn gaps(&self, mut visit: impl FnMut(u16, u16)) {
        for (mut cursor, end) in REGIONS {
            loop {
                let next = self
                    .blocks
                    .iter()
                    .flatten()
                    .filter(|(s, _)| *s >= cursor && *s < end)
                    .min_by_key(|(s, _)| *s);
                let stop = next.map_or(end, |b| b.0);
                if stop > cursor {
                    visit(cursor, stop - cursor);
                }
                let Some(&(s, n)) = next else { break };
                cursor = s + n;
            }
        }
    }
    pub fn call(&mut self, cpu: &mut CpuState, function: u8) -> Result<(), u8> {
        let dx = cpu.wide(Slot::Data);
        if function == 0x10 {
            let mut best = 0;
            let mut place = None;
            self.gaps(|s, n| {
                best = best.max(n);
                if dx != 0 && n >= dx && place.is_none() {
                    place = Some(s);
                }
            });
            if let Some(s) = place
                && let Some(slot) = self.blocks.iter_mut().find(|b| b.is_none())
            {
                *slot = Some((s, dx));
                cpu.set_wide(Slot::Base, s);
                return Ok(());
            }
            cpu.set_wide(Slot::Data, best);
            return Err(if best == 0 { 0xb1 } else { 0xb0 });
        }
        let i = self
            .blocks
            .iter()
            .position(|b| b.is_some_and(|(s, _)| s == dx))
            .ok_or(0xb2)?;
        if function == 0x11 {
            self.blocks[i] = None;
            return Ok(());
        }
        let size = cpu.wide(Slot::Base);
        let end = self
            .blocks
            .iter()
            .flatten()
            .map(|b| b.0)
            .filter(|&s| s > dx)
            .chain(REGIONS.iter().map(|r| r.1).filter(|&s| s > dx))
            .min()
            .unwrap();
        if size == 0 || size > end - dx {
            cpu.set_wide(Slot::Data, end - dx);
            return Err(0xb0);
        }
        self.blocks[i] = Some((dx, size));
        Ok(())
    }
}

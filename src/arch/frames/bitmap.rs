//! Free physical frames in the four-GiB supervisor identity map.
const SHIFT: usize = 12;
const LIMIT: u64 = 1 << 32;
const WORDS: usize = (LIMIT >> SHIFT) as usize / 32;

pub(super) struct FreeFrames {
    words: [u32; WORDS],
    first: usize,
    pub(super) free: usize,
    pub(super) total: usize,
}

impl FreeFrames {
    pub(super) const fn new() -> Self {
        Self {
            words: [0; WORDS],
            first: 0,
            free: 0,
            total: 0,
        }
    }

    // Only whole usable frames can be donated. Even partial reservations
    // take precedence, regardless of how firmware ordered its descriptors.
    pub(super) fn region(&mut self, start: u64, length: u64, usable: bool) {
        let end = start.saturating_add(length).min(LIMIT);
        if start >= end {
            return;
        }
        let round = (1 << SHIFT) - 1;
        let (lo, hi) = if usable {
            ((start + round) >> SHIFT, end >> SHIFT)
        } else {
            (start >> SHIFT, (end + round) >> SHIFT)
        };
        for frame in lo as usize..hi as usize {
            let i = frame / 32;
            let mask = 1u32 << (frame % 32);
            if usable && self.words[i] & mask == 0 {
                self.words[i] |= mask;
                self.free += 1;
                self.first = self.first.min(i);
            } else if !usable && self.words[i] & mask != 0 {
                self.words[i] &= !mask;
                self.free -= 1;
            }
        }
        self.total = self.free;
    }

    pub(super) fn take(&mut self) -> Option<usize> {
        if self.free == 0 {
            return None;
        }
        while self.first < WORDS {
            let word = &mut self.words[self.first];
            if *word != 0 {
                let bit = word.trailing_zeros();
                *word &= !(1u32 << bit);
                self.free -= 1;
                return Some((self.first * 32 + bit as usize) << SHIFT);
            }
            self.first += 1;
        }
        None
    }

    pub(super) fn release(&mut self, address: usize) {
        let frame = address >> SHIFT;
        let i = frame / 32;
        let mask = 1u32 << (frame % 32);
        debug_assert_eq!(self.words[i] & mask, 0);
        self.words[i] |= mask;
        self.first = self.first.min(i);
        self.free += 1;
    }
}

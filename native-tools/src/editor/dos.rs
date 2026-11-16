use super::gap_buffer::Storage;
use dosrs_realmode::{byte, copy_bytes, store};

pub struct Segment {
    pub base: u16,
    pub size: u16,
}

impl Storage for Segment {
    fn capacity(&self) -> usize {
        self.size as usize
    }
    fn byte(&self, at: usize) -> u8 {
        byte(self.base, at as u16) as u8
    }
    fn write(&mut self, at: usize, value: u8) {
        store(self.base, at as u16, value as u16);
    }
    fn move_bytes(&mut self, from: usize, to: usize, count: usize) {
        if from == to || count == 0 {
            return;
        }
        if from < to && to - from < count {
            // Firmware copies forward. Move overlapping runs from their high end.
            let gap = to - from;
            let mut remaining = count;
            while remaining > 0 {
                let n = if remaining < gap { remaining } else { gap };
                remaining -= n;
                copy_bytes(
                    self.base,
                    (from + remaining) as u16,
                    self.base,
                    (to + remaining) as u16,
                    n as u16,
                );
            }
        } else {
            copy_bytes(self.base, from as u16, self.base, to as u16, count as u16);
        }
    }
}

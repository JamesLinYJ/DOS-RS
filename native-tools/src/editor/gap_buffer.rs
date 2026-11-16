// Copyright (c) Microsoft Corporation. Licensed under the MIT License.
// Adapted from microsoft/edit crates/edit/src/buffer/gap_buffer.rs. The DOS
// backend supplies storage instead of reserving virtual memory.
pub trait Storage {
    fn capacity(&self) -> usize;
    fn byte(&self, at: usize) -> u8;
    fn write(&mut self, at: usize, value: u8);
    fn move_bytes(&mut self, from: usize, to: usize, count: usize);
}
pub struct GapBuffer<S> {
    storage: S,
    pub capacity: usize,
    pub text_length: usize,
    gap_off: usize,
    gap_len: usize,
    pub generation: u32,
    generation_counter: u32,
}
impl<S: Storage> GapBuffer<S> {
    pub fn open(storage: S, length: usize) -> Self {
        let capacity = storage.capacity();
        Self {
            storage,
            capacity,
            text_length: length,
            gap_off: length,
            gap_len: capacity - length,
            generation: 0,
            generation_counter: 0,
        }
    }
    pub fn byte(&self, off: usize) -> u8 {
        self.storage.byte(if off < self.gap_off {
            off
        } else {
            off + self.gap_len
        })
    }
    fn move_gap(&mut self, off: usize) {
        if self.gap_len > 0 {
            let left = off < self.gap_off;
            let move_src = if left {
                off
            } else {
                self.gap_off + self.gap_len
            };
            let move_dst = if left {
                off + self.gap_len
            } else {
                self.gap_off
            };
            let move_len = if left {
                self.gap_off - off
            } else {
                off - self.gap_off
            };
            self.storage.move_bytes(move_src, move_dst, move_len);
        }
        self.gap_off = off
    }
    pub fn replace(&mut self, off: usize, delete: usize, insert: &[u8], n: usize) -> bool {
        if off > self.text_length
            || delete > self.text_length - off
            || n > insert.len()
            || n > self.gap_len + delete
        {
            return false;
        }
        self.move_gap(off);
        self.gap_len += delete;
        self.text_length -= delete;
        let mut i = 0;
        while i < n {
            self.storage.write(self.gap_off + i, insert[i]);
            i += 1;
        }
        self.gap_off += n;
        self.gap_len -= n;
        self.text_length += n;
        self.generation_counter = self.generation_counter.wrapping_add(1);
        self.generation = self.generation_counter;
        true
    }
    pub fn flatten(&mut self) -> &S {
        self.move_gap(self.text_length);
        &self.storage
    }
}

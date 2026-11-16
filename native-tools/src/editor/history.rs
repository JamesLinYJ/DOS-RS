// Copyright (c) Microsoft Corporation. Licensed under the MIT License.
// DOS representation of Edit's text history entries and saved-generation tracking.
use super::Document;
#[derive(Clone, Copy)]
struct Entry {
    before: u32,
    after: u32,
    pos: usize,
    cursor_before: usize,
    cursor_after: usize,
    removed: [u8; 4],
    added: [u8; 4],
    old: u16,
    new: u16,
}
const EMPTY: Entry = Entry {
    before: 0,
    after: 0,
    pos: 0,
    cursor_before: 0,
    cursor_after: 0,
    removed: [0; 4],
    added: [0; 4],
    old: 0,
    new: 0,
};
pub struct History {
    entries: [Entry; 64],
    head: u16,
    len: u16,
    applied: u16,
}
impl History {
    pub fn new() -> Self {
        Self {
            entries: [EMPTY; 64],
            head: 0,
            len: 0,
            applied: 0,
        }
    }
    pub fn replace(
        &mut self,
        doc: &mut Document,
        cursor: usize,
        at: usize,
        delete: usize,
        bytes: &[u8],
        size: usize,
    ) -> bool {
        let mut e = EMPTY;
        e.before = doc.generation;
        e.cursor_before = cursor;
        e.cursor_after = at + size;
        e.pos = at;
        e.old = delete as u16;
        e.new = size as u16;
        let mut i = 0;
        while i < delete {
            e.removed[i] = doc.byte(at + i);
            i += 1
        }
        i = 0;
        while i < size {
            e.added[i] = bytes[i];
            i += 1
        }
        if !doc.replace(at, delete, bytes, size) {
            return false;
        }
        e.after = doc.generation;
        self.len = self.applied;
        if self.len == 64 {
            self.head = (self.head + 1) & 63;
            self.len -= 1;
        }
        let i = (self.head + self.len) & 63;
        self.entries[i as usize] = e;
        self.len += 1;
        self.applied = self.len;
        true
    }
    pub fn restore(&mut self, d: &mut Document, redo: bool) -> Option<usize> {
        if redo {
            if self.applied == self.len {
                return None;
            }
        } else {
            if self.applied == 0 {
                return None;
            }
            self.applied -= 1
        }
        let i = (self.head + self.applied) & 63;
        let e = self.entries[i as usize];
        if redo {
            d.replace(e.pos, e.old as usize, &e.added, e.new as usize);
            d.generation = e.after;
            self.applied += 1;
            Some(e.cursor_after)
        } else {
            d.replace(e.pos, e.new as usize, &e.removed, e.old as usize);
            d.generation = e.before;
            Some(e.cursor_before)
        }
    }
}

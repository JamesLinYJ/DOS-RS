//! Open files are identified by a directory sector and slot. The first cluster
//! and length are read from disk, since another open handle can resize the file.
use super::*;

const ACCESS: u16 = 5;
const BAD_HANDLE: u16 = 6;
const WRITE_FAULT: u16 = 29;
const READ_FAULT: u16 = 30;
const DISK_FULL: u16 = 39;

fn error(code: u16) -> u32 {
    0x10000 | u32::from(code)
}

impl Volume {
    fn load_entry(&self, lba: u32, at: u16, e: &mut Entry) -> u16 {
        if !self.mounted()
            || lba < self.root_sector
            || lba - self.start >= self.total
            || at > 480
            || at & 31 != 0
        {
            return BAD_HANDLE;
        }
        if !sector(lba, false) {
            return READ_FAULT;
        }
        if matches!(b(at), 0 | 229) || b(at + 11) & 0x18 != 0 {
            return BAD_HANDLE;
        }
        self.entry(e, lba, at);
        if (e.first == 0 && e.size != 0) || (e.first != 0 && !self.valid(e.first)) {
            return READ_FAULT;
        }
        0
    }
    pub fn file_stat(
        &self,
        lba: u32,
        at: u16,
        first: &mut u32,
        size: &mut u32,
        attr: &mut u16,
    ) -> u16 {
        let mut e = Entry::EMPTY;
        let err = self.load_entry(lba, at, &mut e);
        if err == 0 {
            *first = e.first;
            *size = e.size;
            *attr = e.attr;
        }
        err
    }
    pub fn file_open(&mut self, create: bool, attr: u16, lba: &mut u32, at: &mut u16) -> u16 {
        let mut e = Entry::EMPTY;
        if !self.mounted() {
            return 15;
        }
        if create && attr & !0x27 != 0 {
            return ACCESS;
        }
        if self.scan(0, &mut e) {
            if e.attr & 16 != 0 || create && e.attr & 1 != 0 {
                return ACCESS;
            }
            if create {
                let err = self.shrink(&e, 0);
                if err != 0 {
                    return err;
                }
                if !sector(e.lba, false) {
                    return READ_FAULT;
                }
                store(BUFFER, e.offset + 11, attr | 32);
                if !sector(e.lba, true) {
                    return WRITE_FAULT;
                }
            }
        } else {
            if e.lba == u32::MAX {
                return READ_FAULT;
            }
            if !create {
                return 2;
            }
            if !self.new_slot(&mut e) {
                return DISK_FULL;
            }
            if !sector(e.lba, false) {
                return READ_FAULT;
            }
            let mut i = 0;
            while i < 32 {
                store(
                    BUFFER,
                    e.offset + i,
                    if i < 11 { byte(COMMAND, 128 + i) } else { 0 },
                );
                i += 1;
            }
            store(BUFFER, e.offset + 11, attr | 32);
            set_word(e.offset + 16, 0x21);
            set_word(e.offset + 24, 0x21);
            if !sector(e.lba, true) {
                return WRITE_FAULT;
            }
        }
        *lba = e.lba;
        *at = e.offset;
        0
    }
    fn nth_cluster(&self, mut c: u32, mut skip: u32) -> u32 {
        if skip >= self.limit - 1 {
            return 0;
        }
        while skip != 0 && self.valid(c) {
            c = self.next(c);
            skip -= 1;
        }
        if self.valid(c) { c } else { 0 }
    }
    fn account(&self, added: u32, freed: u32) -> bool {
        // FSInfo is a hint. A transient fault on one copy leaves its original
        // count intact, so retry only the copy that missed this delta.
        let a = self.info(self.info, added, freed);
        let b = self.info(self.backup_info, added, freed);
        let a = a || self.info(self.info, added, freed);
        let b = b || self.info(self.backup_info, added, freed);
        a && b
    }
    fn publish(&self, e: &Entry, first: u32, size: u32) -> bool {
        if !sector(e.lba, false) {
            return false;
        }
        set_word(e.offset + 20, (first >> 16) as u16);
        set_word(e.offset + 26, first as u16);
        set_dw(e.offset + 28, size);
        store(BUFFER, e.offset + 11, e.attr | 32);
        sector(e.lba, true)
    }
    fn shrink(&self, e: &Entry, size: u32) -> u16 {
        if size == e.size && !(size == 0 && e.first != 0) {
            return 0;
        }
        let mut keep = 0;
        let mut rest = e.first;
        if size != 0 {
            keep = self.nth_cluster(e.first, (size - 1) >> (9 + self.shift));
            if keep == 0 {
                return READ_FAULT;
            }
            rest = self.next(keep);
            if rest < EOC && !self.valid(rest) {
                return READ_FAULT;
            }
        }
        // Publish the shorter length before freeing storage reachable by the old
        // entry. Do not reclaim a chain that still belongs to a visible file.
        if !self.publish(e, if size == 0 { 0 } else { e.first }, size) {
            return WRITE_FAULT;
        }
        if rest >= EOC || rest == 0 {
            return 0;
        }
        if keep != 0 && !self.change(keep, 0x0fff_ffff) {
            self.publish(e, e.first, e.size);
            return WRITE_FAULT;
        }
        let freed = self.release(rest);
        if freed == u32::MAX || !self.account(0, freed) {
            return WRITE_FAULT;
        }
        0
    }
    /// AH=40h with CX=0 sets EOF, including zero-filled extension.
    pub fn file_resize(&mut self, lba: u32, at: u16, size: u32) -> u16 {
        let mut e = Entry::EMPTY;
        let err = self.load_entry(lba, at, &mut e);
        if err != 0 {
            return err;
        }
        if size <= e.size {
            return self.shrink(&e, size);
        }
        let r = self.write_range(&e, size, 0, 0, 0);
        if r > 65535 { r as u16 } else { 0 }
    }
    /// The caller checks the SFT access mode. The creating handle may write
    /// to its newly created read-only file until that handle is closed.
    pub fn file_write(
        &mut self,
        lba: u32,
        at: u16,
        pos: u32,
        count: u16,
        seg: u16,
        off: u16,
    ) -> u32 {
        if count == 0 {
            let err = self.file_resize(lba, at, pos);
            return if err == 0 { 0 } else { error(err) };
        }
        if u32::from(off) + u32::from(count) > 65536 || pos > u32::MAX - u32::from(count) {
            return error(ACCESS);
        }
        let mut e = Entry::EMPTY;
        let err = self.load_entry(lba, at, &mut e);
        if err != 0 {
            return error(err);
        }
        self.write_range(&e, pos, count, seg, off)
    }
    /// A write beyond EOF only makes the skipped bytes visible after at least
    /// one caller byte has been stored. A zero-length extension is all-or-none.
    fn write_range(&mut self, e: &Entry, pos: u32, count: u16, seg: u16, off: u16) -> u32 {
        let shift = 9 + self.shift;
        let cluster_bytes = 1u32 << shift;
        let target = pos + u32::from(count);
        let mut end = target;
        let start = if pos < e.size { pos } else { e.size };
        let mut first = e.first;
        let mut old_tail = 0;
        let mut staged = 0;
        let mut added = 0;
        let mut failure = 0;
        // Keep the new chain detached until we know its usable capacity.
        if target > e.size {
            let have = if e.size == 0 {
                if first == 0 { 0 } else { 1 }
            } else {
                ((e.size - 1) >> shift) + 1
            };
            let want = ((target - 1) >> shift) + 1;
            if want > have && first != 0 {
                old_tail = self.nth_cluster(first, have - 1);
                if old_tail == 0 || self.next(old_tail) < EOC {
                    return error(READ_FAULT);
                }
            }
            let mut tail = 0;
            while have + added < want {
                let mut alloc_error = 0;
                let next = self.allocate_checked(&mut alloc_error);
                if next == 0 {
                    failure = alloc_error;
                    end = (have + added) << shift;
                    break;
                }
                if tail == 0 {
                    staged = next
                } else if !self.change(tail, next) {
                    self.release(next);
                    self.release(staged);
                    return error(WRITE_FAULT);
                }
                tail = next;
                added += 1;
            }
            if end < target && (count == 0 || end <= pos) {
                self.release(staged);
                return if count != 0 && failure == DISK_FULL {
                    0
                } else {
                    error(failure)
                };
            }
            if staged != 0 {
                if first == 0 {
                    first = staged
                } else if !self.change(old_tail, staged) {
                    self.release(staged);
                    return error(WRITE_FAULT);
                }
            }
        }
        let mut c = self.nth_cluster(first, start >> shift);
        let mut p = start;
        while p < end {
            if c == 0 {
                failure = READ_FAULT;
                break;
            }
            let lba = self.first(c) + ((p & (cluster_bytes - 1)) >> 9);
            let at = (p & 511) as u16;
            let upto = if p < pos && pos < end { pos } else { end };
            let room = 512 - at;
            let take = if upto - p < u32::from(room) {
                (upto - p) as u16
            } else {
                room
            };
            if take != 512 && !sector(lba, false) {
                failure = READ_FAULT;
                break;
            }
            if p < pos {
                let mut i = 0;
                while i < take {
                    store(BUFFER, at + i, 0);
                    i += 1
                }
            } else {
                super::super::copy_bytes(seg, off + (p - pos) as u16, BUFFER, at, take);
            }
            if !sector(lba, true) {
                failure = WRITE_FAULT;
                break;
            }
            p += u32::from(take);
            if p < end && p & (cluster_bytes - 1) == 0 {
                c = self.next(c);
                if !self.valid(c) {
                    c = 0
                }
            }
        }
        let mut bytes = if p > pos { p - pos } else { 0 };
        let mut visible = if p > e.size && (bytes > 0 || count == 0 && p == target) {
            p
        } else {
            e.size
        };
        if (visible != e.size || bytes != 0 && e.attr & 32 == 0) && !self.publish(e, first, visible)
        {
            failure = WRITE_FAULT;
            visible = e.size;
            let existing = if pos < visible { visible - pos } else { 0 };
            if bytes > existing {
                bytes = existing;
            }
        }
        // Keep only the clusters containing committed caller data; any reserved
        // suffix from a failed write or an uncommitted seek gap is released.
        if added != 0 {
            let old_count = if e.size == 0 {
                if e.first == 0 { 0 } else { 1 }
            } else {
                ((e.size - 1) >> shift) + 1
            };
            let used = if visible <= e.size {
                0
            } else {
                (((visible - 1) >> shift) + 1) - old_count
            };
            if used < added {
                let mut free = staged;
                let keep = if used == 0 {
                    old_tail
                } else {
                    self.nth_cluster(staged, used - 1)
                };
                if keep != 0 {
                    free = self.next(keep);
                    if !self.change(keep, 0x0fff_ffff) {
                        return error(WRITE_FAULT);
                    }
                }
                if self.release(free) == u32::MAX {
                    return error(WRITE_FAULT);
                }
            }
            if used != 0 && !self.account(used, 0) {
                failure = WRITE_FAULT;
            }
        }
        if bytes == 0 && failure != 0 && !(count != 0 && failure == DISK_FULL) {
            error(failure)
        } else {
            bytes
        }
    }
}

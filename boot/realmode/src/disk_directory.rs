//! Directory iteration and mutation for the native FAT12/FAT16/FAT32 volume.
use super::*;

impl Volume {
    /// Read the next physical short or long entry. The caller retains the cluster
    /// and slot across calls, so continuation never rescans a large directory.
    /// Return 0 at end, 1 for a record and 0xffff on damaged media.
    pub fn record(&self, cluster: &mut u32, slot: &mut u16, lba: &mut u32, at: &mut u16) -> u16 {
        if *cluster >= EOC {
            return 0;
        }
        let sectors = if *cluster == 0 {
            self.root_sectors
        } else {
            1u16 << self.shift
        };
        if *slot >> 4 >= sectors {
            if *cluster == 0 {
                return 0;
            }
            let next = self.next(*cluster);
            if next >= EOC {
                *cluster = EOC;
                return 0;
            }
            if !self.valid(next) || next == *cluster {
                return u16::MAX;
            }
            *cluster = next;
            *slot = 0;
        }
        let base = if *cluster == 0 {
            self.root_sector
        } else {
            self.first(*cluster)
        };
        *lba = base + u32::from(*slot >> 4);
        *at = (*slot & 15) << 5;
        *slot += 1;
        if !self.sector(*lba, false) {
            return u16::MAX;
        }
        if b(*at) == 0 {
            *cluster = EOC;
            0
        } else {
            1
        }
    }
    pub fn root_cluster(&self) -> u32 {
        self.root
    }
    /// DOS IOCTL 69h level-zero information buffer.
    pub fn volume_info(&self, seg: u16, at: u16) -> bool {
        if !self.sector(self.start, false) {
            return false;
        }
        let ext = if self.is32() { 66 } else { 38 };
        let serial = if b(ext) == 0x29 { dw(ext + 1) } else { 0 };
        store(seg, at, 0);
        store(seg, at + 1, 0);
        let mut j = 0;
        while j < 4 {
            store(seg, at + 2 + j, (serial >> (8 * j)) as u16);
            j += 1;
        }
        let mut i = 0;
        while i < 11 {
            let ch = if b(ext) == 0x29 {
                b(ext + 5 + i)
            } else {
                u16::from(b"NO NAME    "[usize::from(i)])
            };
            store(seg, at + 6 + i, ch);
            i += 1;
        }
        let kind = if self.fat12 {
            b"FAT12   "
        } else if self.is32() {
            b"FAT32   "
        } else {
            b"FAT16   "
        };
        i = 0;
        while i < 8 {
            store(seg, at + 17 + i, u16::from(kind[usize::from(i)]));
            i += 1;
        }
        let mut c = self.root;
        let mut slot = 0;
        let mut lba = 0;
        let mut off = 0;
        let mut seen = 0u32;
        // A volume label in the root is authoritative after LABEL or a rename.
        while seen < 65536 {
            match self.record(&mut c, &mut slot, &mut lba, &mut off) {
                0 => return true,
                1 if b(off) != 0xe5 && b(off + 11) == 8 => {
                    super::super::copy_bytes(BUFFER, off, seg, at + 6, 11);
                    return true;
                }
                1 => seen += 1,
                _ => return false,
            }
        }
        false
    }
    /// AH=36h has three 16-bit outputs. Group clusters on large FAT32
    /// media to keep the reported free and total allocation units comparable.
    pub fn free_space(&self, unit: &mut u16, free: &mut u16, total: &mut u16) -> bool {
        let mut count = 0u32;
        let mut c = 2;
        let mut table = u32::MAX;
        while c <= self.limit {
            let lba = self.table(self.first_table, c);
            if lba != table {
                if !self.sector(lba, false) {
                    return false;
                }
                table = lba;
            }
            match self.raw_entry(self.first_table, c) {
                0 => count += 1,
                u32::MAX => return false,
                _ => (),
            }
            c += 1;
        }
        let mut all = self.limit - 1;
        let mut sectors = 1u16 << self.shift;
        while all > 65535 && sectors < 0x8000 {
            all >>= 1;
            count >>= 1;
            sectors <<= 1
        }
        *unit = sectors;
        *free = if count > 65535 { 65535 } else { count as u16 };
        *total = if all > 65535 { 65535 } else { all as u16 };
        true
    }
    // Construct a new named entry in an already-read directory sector.
    fn new_directory_entry(&self, at: u16, cluster: u32, dot: u16) {
        let mut i = 0;
        while i < 32 {
            let value = if i >= 11 {
                0
            } else if dot == 0 {
                byte(COMMAND, 128 + i)
            } else if i < dot {
                46
            } else {
                32
            };
            store(BUFFER, at + i, value);
            i += 1;
        }
        store(BUFFER, at + 11, 16);
        set_word(at + 16, 33);
        set_word(at + 24, 33);
        set_word(at + 26, cluster as u16);
        set_word(at + 20, (cluster >> 16) as u16);
    }
    pub fn mkdir(&mut self) -> u16 {
        let mut e = Entry::EMPTY;
        if self.scan(0, &mut e) {
            return 5;
        }
        if e.lba == u32::MAX || !self.new_slot(&mut e) {
            return 5;
        }
        let c = self.allocate();
        if c == 0 {
            return 5;
        }
        let mut i = 0;
        while i < 512 {
            store(BUFFER, i, 0);
            i += 1
        }
        let mut n = 1u16 << self.shift;
        while n > 1 {
            n -= 1;
            if !self.sector(self.first(c) + u32::from(n), true) {
                self.release(c);
                return 5;
            }
        }
        self.new_directory_entry(0, c, 1);
        self.new_directory_entry(32, if self.cwd == self.root { 0 } else { self.cwd }, 2);
        if !self.sector(self.first(c), true) || !self.sector(e.lba, false) {
            self.release(c);
            return 5;
        }
        self.new_directory_entry(e.offset, c, 0);
        if !self.sector(e.lba, true) {
            self.release(c);
            return 5;
        }
        if self.info(self.info, 1, 0) && self.info(self.backup_info, 1, 0) {
            0
        } else {
            5
        }
    }
    fn empty_directory(&self, mut cluster: u32) -> bool {
        let mut slot = 0;
        let mut lba = 0;
        let mut at = 0;
        let mut seen = 0u32;
        while seen < 65536 {
            match self.record(&mut cluster, &mut slot, &mut lba, &mut at) {
                0 => return true,
                1 => {}
                _ => return false,
            }
            let first = b(at);
            if first != 229
                && !(first == 46 && (b(at + 1) == 32 || (b(at + 1) == 46 && b(at + 2) == 32)))
            {
                return false;
            }
            seen += 1;
        }
        false
    }
    // Remove an associated VFAT name as well. A DOS rename changes the short
    // name checksum, and keeping those records would leave an invalid alias.
    fn drop_alias(&self, item: &Entry) -> bool {
        let mut cluster = self.cwd;
        let mut slot = 0;
        let mut lba = 0;
        let mut at = 0;
        let mut count = 0u16;
        let mut remaining = 0u16;
        let mut checksum = 0u16;
        let mut start_cluster = 0u32;
        let mut start_slot = 0;
        let mut seen = 0u32;
        while seen < 65536 {
            let prev_cluster = cluster;
            let prev_slot = slot;
            if self.record(&mut cluster, &mut slot, &mut lba, &mut at) != 1 {
                return false;
            }
            if lba == item.lba && at == item.offset {
                break;
            }
            let seq = b(at);
            if b(at + 11) != 15 || b(at + 12) != 0 || word(at + 26) != 0 {
                count = 0;
                remaining = 0;
            } else if (0x41..=0x54).contains(&seq) {
                count = seq - 0x40;
                remaining = count - 1;
                checksum = b(at + 13);
                start_cluster = prev_cluster;
                start_slot = prev_slot;
            } else if remaining != 0 && seq == remaining && b(at + 13) == checksum {
                remaining -= 1;
            } else {
                count = 0;
                remaining = 0;
            }
            seen += 1;
        }
        if seen == 65536 {
            return false;
        }
        if count == 0 || remaining != 0 {
            return true;
        }
        let mut sum = 0u16;
        let mut i = 0;
        while i < 11 {
            sum = (((sum >> 1) | ((sum & 1) << 7)) + b(at + i)) & 255;
            i += 1;
        }
        if sum != checksum {
            return true;
        }
        while count > 0 {
            if self.record(&mut start_cluster, &mut start_slot, &mut lba, &mut at) != 1 {
                return false;
            }
            store(BUFFER, at, 229);
            if !self.sector(lba, true) {
                return false;
            }
            count -= 1;
        }
        true
    }
    pub fn remove(&mut self, directory: bool, current: u32) -> u16 {
        let mut e = Entry::EMPTY;
        if !self.scan(0, &mut e) {
            return if directory { 3 } else { 2 };
        }
        if (e.attr & 16 != 0) != directory || e.attr & 1 != 0 || b(e.offset) == 46 {
            return 5;
        }
        if directory
            && (!self.valid(e.first) || e.first == current || !self.empty_directory(e.first))
        {
            return 5;
        }
        if !self.drop_alias(&e) || !self.sector(e.lba, false) {
            return 5;
        }
        store(BUFFER, e.offset, 229);
        if !self.sector(e.lba, true) {
            return 5;
        }
        let freed = self.release(e.first);
        if freed != u32::MAX
            && self.info(self.info, 0, freed)
            && self.info(self.backup_info, 0, freed)
        {
            0
        } else {
            5
        }
    }
    /// Source is packed at COMMAND+128, replacement at new_name in the same segment.
    pub fn rename(&mut self, new_name: u16) -> u16 {
        let mut item = Entry::EMPTY;
        if !self.scan(0, &mut item) {
            return 2;
        }
        if b(item.offset) == 46 {
            return 5;
        }
        super::super::copy_bytes(COMMAND, new_name, COMMAND, 128, 11);
        let mut collision = Entry::EMPTY;
        if self.scan(0, &mut collision) || collision.lba == u32::MAX {
            return 5;
        }
        if !self.drop_alias(&item) || !self.sector(item.lba, false) {
            return 5;
        }
        super::super::copy_bytes(COMMAND, 128, BUFFER, item.offset, 11);
        if self.sector(item.lba, true) { 0 } else { 5 }
    }
}

impl Volume {
    /// Select the parent and encode the final component as a DOS search mask.
    pub fn pattern(&mut self, seg: u16, start: u16, end: u16, dest: u16, off: u16) -> bool {
        let mut last = start;
        let mut i = start;
        while i < end {
            if byte(seg, i) == 92 {
                last = i + 1
            }
            i += 1
        }
        if last != start && !self.path(seg, start, last - start, true) {
            return false;
        }
        pack_mask(seg, last, end, dest, off)
    }
}
/// Convert both fields separately: '*' fills the rest of its current field,
/// and '?' matches even padding. A missing name denotes all entries.
pub fn pack_mask(seg: u16, mut i: u16, end: u16, dest: u16, off: u16) -> bool {
    let mut p = 0;
    while p < 11 {
        store(dest, off + p, if i == end { 63 } else { 32 });
        p += 1
    }
    if i == end {
        return true;
    }
    let mut field = 8;
    p = 0;
    let mut star = false;
    while i < end {
        let c = byte(seg, i);
        let c = if (97..=122).contains(&c) { c - 32 } else { c };
        i += 1;
        if c == 46 {
            if field == 11 || p == 0 {
                return false;
            }
            p = 8;
            field = 11;
            star = false;
        } else if c == 42 {
            while p < field {
                store(dest, off + p, 63);
                p += 1
            }
            star = true;
        } else if !star {
            if p >= field || c <= 32 || matches!(c,34|43|44|47|58..=62|91..=93|124) {
                return false;
            }
            store(dest, off + p, c);
            p += 1;
        }
    }
    true
}
pub fn match_mask(seg: u16, off: u16, src: u16, entry: u16) -> bool {
    let mut i = 0;
    while i < 11 {
        let c = byte(seg, off + i);
        if c != 63 && c != byte(src, entry + i) {
            return false;
        }
        i += 1;
    }
    true
}
pub fn unpack_name(src: u16, at: u16, dest: u16, off: u16) -> u16 {
    let mut i = 0;
    let mut len = 0;
    while i < 11 {
        let c = byte(src, at + i);
        if c != 32 {
            if i == 8 {
                store(dest, off + len, 46);
                len += 1
            }
            store(dest, off + len, c);
            len += 1
        }
        i += 1;
    }
    store(dest, off + len, 0);
    len
}

impl Volume {
    pub fn set_attributes(&self, flags: u16) -> u16 {
        let mut e = Entry::EMPTY;
        if !self.scan(0, &mut e) {
            return if e.lba == u32::MAX { 30 } else { 2 };
        }
        if flags & !0x37 != 0 {
            return 5;
        }
        store(BUFFER, e.offset + 11, (e.attr & 0x10) | (flags & 0x27));
        if self.sector(e.lba, true) { 0 } else { 29 }
    }
}

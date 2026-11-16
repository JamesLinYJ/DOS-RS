//! Sector-based FAT16/FAT32 on BIOS disk 80h. A sector buffer fits a near
//! segment; disk addresses, chain cursors and file lengths remain 32-bit.
use super::{BUFFER, COMMAND, byte, print, put, sector, store, word};

fn b(at: u16) -> u16 {
    byte(BUFFER, at)
}
fn dw(at: u16) -> u32 {
    (word(at) as u32) | ((word(at + 2) as u32) << 16)
}
fn set_word(at: u16, x: u16) {
    store(BUFFER, at, x);
    store(BUFFER, at + 1, x >> 8);
}
fn set_dw(at: u16, x: u32) {
    set_word(at, x as u16);
    set_word(at + 2, (x >> 16) as u16);
}
const EOC: u32 = 0x0fff_fff8;

pub struct Volume {
    start: u32,
    total: u32,
    fat: u32,
    fat_size: u32,
    data: u32,
    limit: u32,
    root: u32,
    cwd: u32,
    root_sector: u32,
    root_sectors: u16,
    shift: u16,
    first_table: u16,
    last_table: u16,
    info: u32,
    backup_info: u32,
    hint: u32,
}
struct Entry {
    lba: u32,
    offset: u16,
    first: u32,
    size: u32,
    attr: u16,
}
impl Entry {
    const EMPTY: Self = Self {
        lba: 0,
        offset: 0,
        first: 0,
        size: 0,
        attr: 0,
    };
}
impl Volume {
    pub const EMPTY: Self = Self {
        start: 0,
        total: 0,
        fat: 0,
        fat_size: 0,
        data: 0,
        limit: 0,
        root: 0,
        cwd: 0,
        root_sector: 0,
        root_sectors: 0,
        shift: 0,
        first_table: 0,
        last_table: 0,
        info: 0,
        backup_info: 0,
        hint: 2,
    };
    pub fn mounted(&self) -> bool {
        self.limit != 0
    }
    pub fn is32(&self) -> bool {
        self.root != 0
    }
    pub fn mount(&mut self) -> bool {
        if !sector(0, false) || word(510) != 0xaa55 {
            return false;
        }
        let mut start = 0;
        let mut length = u32::MAX;
        if word(11) != 512 || !(b(0) == 0xeb || b(0) == 0xe9) {
            let mut at = 446;
            while at < 510 {
                let kind = b(at + 4);
                if matches!(kind, 4 | 6 | 14 | 11 | 12) && (start == 0 || b(at) == 128) {
                    start = dw(at + 8);
                    length = dw(at + 12);
                    if b(at) == 128 {
                        break;
                    }
                }
                at += 16;
            }
            if start == 0 || length == 0 || start > u32::MAX - length || !sector(start, false) {
                return false;
            }
        }
        let reserved = word(14) as u32;
        let tables = b(16);
        let spc = b(13);
        if word(510) != 0xaa55
            || word(11) != 512
            || reserved == 0
            || tables == 0
            || tables > 8
            || spc == 0
            || spc > 128
            || (spc & (spc - 1)) != 0
        {
            return false;
        }
        let mut shift = 0;
        while (1u16 << shift) != spc {
            shift += 1;
        }
        let short = word(22);
        let fat_size = if short != 0 { short as u32 } else { dw(36) };
        let total = if word(19) != 0 {
            word(19) as u32
        } else {
            dw(32)
        };
        if total == 0
            || total > length
            || start > u32::MAX - total
            || reserved >= total
            || fat_size == 0
        {
            return false;
        }
        let root_entries = word(17);
        let root_sectors = (root_entries >> 4) + if root_entries & 15 != 0 { 1 } else { 0 };
        let mut root_rel = reserved;
        let mut t = 0;
        while t < tables {
            if fat_size >= total - root_rel {
                return false;
            }
            root_rel += fat_size;
            t += 1;
        }
        if root_sectors as u32 >= total - root_rel {
            return false;
        }
        let first = root_rel + root_sectors as u32;
        let count = (total - first) >> shift;
        if count < 4085 || count > 0x0fff_ffee {
            return false;
        }
        let fat32 = count >= 65525;
        if (short == 0) != fat32 || (root_entries == 0) != fat32 {
            return false;
        }
        let entries_shift = if fat32 { 7 } else { 8 };
        if fat_size <= ((count + 1) >> entries_shift) {
            return false;
        }
        let mut root = 0;
        let mut active = 0;
        let mut end = tables;
        let mut info = 0;
        let mut backup = 0;
        if fat32 {
            root = dw(44);
            if root < 2 || root > count + 1 || word(42) != 0 {
                return false;
            }
            let flags = word(40);
            if flags & 128 != 0 {
                active = flags & 15;
                if active >= tables {
                    return false;
                }
                end = active + 1;
            }
            let index = word(48) as u32;
            let copy = word(50) as u32;
            if index > 0 && index < reserved {
                info = start + index;
                if copy != 0 && copy + index < reserved {
                    backup = start + copy + index;
                }
            }
        }
        self.start = start;
        self.total = total;
        self.fat = start + reserved;
        self.fat_size = fat_size;
        self.root_sector = start + root_rel;
        self.root_sectors = root_sectors;
        self.data = start + first;
        self.root = root;
        self.cwd = root;
        self.shift = shift;
        self.first_table = active;
        self.last_table = end;
        self.limit = count + 1;
        self.info = info;
        self.backup_info = backup;
        self.hint = 2;
        if info != 0 && sector(info, false) && fsinfo_valid() {
            let hint = dw(492);
            if self.valid(hint) {
                self.hint = hint;
            }
        }
        true
    }
    fn valid(&self, c: u32) -> bool {
        c >= 2 && c <= self.limit
    }
    fn first(&self, c: u32) -> u32 {
        self.data + ((c - 2) << self.shift)
    }
    fn table(&self, t: u16, c: u32) -> u32 {
        self.fat + self.fat_size * t as u32 + (c >> if self.is32() { 7 } else { 8 })
    }
    fn slot(&self, c: u32) -> u16 {
        if self.is32() {
            ((c as u16) & 127) << 2
        } else {
            ((c as u16) & 255) << 1
        }
    }
    fn value(&self, c: u32) -> u32 {
        let at = self.slot(c);
        if self.is32() {
            dw(at) & 0x0fff_ffff
        } else {
            let n = word(at);
            if n >= 0xfff8 { EOC } else { n as u32 }
        }
    }
    fn next(&self, c: u32) -> u32 {
        if !self.valid(c) || !sector(self.table(self.first_table, c), false) {
            return 1;
        }
        self.value(c)
    }
    fn change(&self, c: u32, n: u32) -> bool {
        if !self.valid(c) {
            return false;
        }
        let mut table = self.first_table;
        while table < self.last_table {
            let lba = self.table(table, c);
            if !sector(lba, false) {
                return false;
            }
            let at = self.slot(c);
            if self.is32() {
                set_dw(at, (dw(at) & 0xf000_0000) | (n & 0x0fff_ffff));
            } else {
                set_word(at, n as u16);
            }
            if !sector(lba, true) {
                return false;
            }
            table += 1;
        }
        true
    }
    fn entry(&self, out: &mut Entry, lba: u32, at: u16) {
        out.lba = lba;
        out.offset = at;
        out.size = dw(at + 28);
        out.attr = b(at + 11);
        out.first = word(at + 26) as u32;
        if self.is32() {
            out.first |= ((word(at + 20) as u32) & 0xfff) << 16;
        }
    }
    // Lookup, print a listing, or locate a reusable directory slot.
    fn scan(&self, mode: u16, out: &mut Entry) -> bool {
        out.lba = 0;
        let mut c = self.cwd;
        let mut count = 0;
        loop {
            let base = if c != 0 {
                self.first(c)
            } else {
                self.root_sector
            };
            let sectors = if c != 0 {
                1u16 << self.shift
            } else {
                self.root_sectors
            };
            let mut i = 0;
            while i < sectors {
                let lba = base + i as u32;
                if !sector(lba, false) {
                    out.lba = u32::MAX;
                    return false;
                }
                let mut at = 0;
                while at < 512 {
                    let lead = b(at);
                    if mode == 2 && (lead == 0 || lead == 229) {
                        self.entry(out, lba, at);
                        return true;
                    }
                    if lead == 0 {
                        return mode == 1;
                    }
                    let attr = b(at + 11);
                    if lead != 229 && attr & 8 == 0 {
                        if mode == 1 {
                            let mut j = 0;
                            while j < 11 {
                                let x = b(at + j);
                                if j == 8 && x != 32 {
                                    put(46);
                                }
                                if x != 32 {
                                    put(x);
                                }
                                j += 1;
                            }
                            if attr & 16 != 0 {
                                print("/");
                            }
                            print("\r\n");
                        } else if mode == 0 {
                            let mut j = 0;
                            while j < 11 && b(at + j) == byte(COMMAND, 128 + j) {
                                j += 1;
                            }
                            if j == 11 {
                                self.entry(out, lba, at);
                                return true;
                            }
                        }
                    }
                    at += 32;
                }
                i += 1;
            }
            if c == 0 {
                return mode == 1;
            }
            c = self.next(c);
            if c >= EOC {
                return mode == 1;
            }
            count += 1;
            if !self.valid(c) || count >= self.limit - 1 {
                out.lba = u32::MAX;
                return false;
            }
        }
    }
    pub fn directory(&self) -> u32 {
        self.cwd
    }
    pub fn restore(&mut self, dir: u32) {
        self.cwd = dir;
    }
    /// Select the parent directory and pack the final short name, or select
    /// the entire directory for CD. Callers restore the old CWD on failure.
    pub fn path(&mut self, seg: u16, start: u16, len: u16, directory: bool) -> bool {
        if !self.mounted() || len == 0 {
            return false;
        }
        let mut i = 0;
        if byte(seg, start) == 92 {
            self.root();
            i = 1;
        }
        while i < len {
            let begin = i;
            while i < len && byte(seg, start.wrapping_add(i)) != 92 {
                i += 1;
            }
            let n = i - begin;
            let at = start.wrapping_add(begin);
            if i == len && !directory {
                return super::filename(seg, at, n);
            }
            if n == 2 && byte(seg, at) == 46 && byte(seg, at.wrapping_add(1)) == 46 {
                if !self.parent() {
                    return false;
                }
            } else if !(n == 0 || (n == 1 && byte(seg, at) == 46)) {
                if !super::filename(seg, at, n) || !self.enter() {
                    return false;
                }
            }
            i += 1;
        }
        directory
    }
    pub fn root(&mut self) {
        self.cwd = self.root;
    }
    pub fn parent(&mut self) -> bool {
        if self.cwd == self.root {
            return true;
        }
        if !sector(self.first(self.cwd), false) || b(32) != 46 || b(33) != 46 || b(43) & 16 == 0 {
            return false;
        }
        let mut e = Entry::EMPTY;
        self.entry(&mut e, self.first(self.cwd), 32);
        if e.first == 0 {
            self.root();
            true
        } else if self.valid(e.first) {
            self.cwd = e.first;
            true
        } else {
            false
        }
    }
    pub fn enter(&mut self) -> bool {
        let mut e = Entry::EMPTY;
        if self.scan(0, &mut e) && e.attr & 16 != 0 && self.valid(e.first) {
            self.cwd = e.first;
            true
        } else {
            false
        }
    }
    fn new_slot(&mut self, out: &mut Entry) -> bool {
        if self.scan(2, out) {
            return true;
        }
        if out.lba == u32::MAX {
            return false;
        }
        let mut c = self.cwd;
        let mut steps = 0;
        while self.valid(c) && steps < self.limit - 1 {
            let next = self.next(c);
            if next >= EOC {
                let fresh = self.allocate();
                if fresh == 0 {
                    return false;
                }
                let mut i = 0;
                while i < 512 {
                    store(BUFFER, i, 0);
                    i += 1;
                }
                i = 0;
                while i < (1u16 << self.shift) {
                    if !sector(self.first(fresh) + i as u32, true) {
                        self.change(fresh, 0);
                        return false;
                    }
                    i += 1;
                }
                if !self.change(c, fresh) {
                    self.change(fresh, 0);
                    return false;
                }
                out.lba = self.first(fresh);
                out.offset = 0;
                return self.info(self.info, 1, 0) && self.info(self.backup_info, 1, 0);
            }
            c = next;
            steps += 1;
        }
        false
    }
    pub fn list(&self) -> bool {
        let mut entry = Entry::EMPTY;
        self.scan(1, &mut entry)
    }
    pub fn describe(&self, first: &mut u32, len: &mut u32) -> bool {
        let mut entry = Entry::EMPTY;
        if !self.scan(0, &mut entry) || entry.attr & 16 != 0 {
            return false;
        }
        *first = entry.first;
        *len = entry.size;
        true
    }
    pub fn read_at(
        &self,
        mut c: u32,
        size: u32,
        offset: u32,
        count: u16,
        dest: u16,
        at: u16,
    ) -> u32 {
        if offset >= size || count == 0 {
            return 0;
        }
        let amount = if size - offset < count as u32 {
            size - offset
        } else {
            count as u32
        };
        if at as u32 + amount > 65536 {
            return u32::MAX;
        }
        let cluster_bytes = 512u32 << self.shift;
        let mut skip = offset >> (9 + self.shift);
        if skip >= self.limit - 1 {
            return u32::MAX;
        }
        while skip != 0 && self.valid(c) {
            c = self.next(c);
            skip -= 1;
        }
        let mut within = (offset & (cluster_bytes - 1)) as u16;
        let mut copied = 0u32;
        while copied < amount {
            if !self.valid(c) || !sector(self.first(c) + (within >> 9) as u32, false) {
                return u32::MAX;
            }
            let inside = within & 511;
            let room = 512 - inside;
            let take = if amount - copied < room as u32 {
                (amount - copied) as u16
            } else {
                room
            };
            super::copy_bytes(BUFFER, inside, dest, at + copied as u16, take);
            copied += take as u32;
            let next = (within as u32 & 0xffff_fe00) + 512;
            if next == cluster_bytes {
                within = 0;
                if copied < amount {
                    c = self.next(c);
                }
            } else {
                within = next as u16;
            }
        }
        copied
    }
    pub fn type_file(&self) -> bool {
        self.stream(0, u32::MAX, true) != u32::MAX
    }
    pub fn load(&self, dest: u16, max: u16) -> Option<u16> {
        let n = self.stream(dest, max as u32, false);
        if n == u32::MAX { None } else { Some(n as u16) }
    }
    fn stream(&self, mut dest: u16, max: u32, display: bool) -> u32 {
        let mut e = Entry::EMPTY;
        if !self.scan(0, &mut e) || e.attr & 16 != 0 || e.size > max {
            return u32::MAX;
        }
        if e.size == 0 {
            return if e.first == 0 { 0 } else { u32::MAX };
        }
        let mut c = e.first;
        let mut left = e.size;
        let mut steps = 0;
        while self.valid(c) && steps < self.limit - 1 {
            let mut i = 0;
            while i < (1u16 << self.shift) && left != 0 {
                if !sector(self.first(c) + i as u32, false) {
                    return u32::MAX;
                }
                let n = if left < 512 { left as u16 } else { 512 };
                let mut x = 0;
                while x < n {
                    let v = b(x);
                    if display {
                        put(v);
                    } else {
                        store(dest, x, v);
                    }
                    x += 1;
                }
                if !display {
                    dest += 32;
                }
                left -= n as u32;
                i += 1;
            }
            c = self.next(c);
            steps += 1;
            if left == 0 {
                return if c >= EOC { e.size } else { u32::MAX };
            }
        }
        u32::MAX
    }
    fn allocate(&mut self) -> u32 {
        let mut c = self.hint;
        let mut checked = 0;
        let mut cached = u32::MAX;
        while checked < self.limit - 1 {
            let lba = self.table(self.first_table, c);
            if lba != cached {
                if !sector(lba, false) {
                    return 0;
                }
                cached = lba;
            }
            let after = if c == self.limit { 2 } else { c + 1 };
            if self.value(c) == 0 {
                if !self.change(c, 0x0fff_ffff) {
                    return 0;
                }
                self.hint = after;
                return c;
            }
            c = after;
            checked += 1;
        }
        0
    }
    fn release(&self, mut c: u32) -> u32 {
        let mut freed = 0;
        while self.valid(c) && freed < self.limit - 1 {
            let next = self.next(c);
            if (next < EOC && !self.valid(next)) || !self.change(c, 0) {
                return u32::MAX;
            }
            freed += 1;
            if next >= EOC {
                return freed;
            }
            c = next;
        }
        if c == 0 { freed } else { u32::MAX }
    }
    fn info(&self, lba: u32, added: u32, freed: u32) -> bool {
        if lba == 0 {
            return true;
        }
        if !sector(lba, false) {
            return false;
        }
        if !fsinfo_valid() {
            return true;
        }
        let old = dw(488);
        let new = if old == u32::MAX
            || old > self.limit - 1
            || old < added
            || freed > self.limit - 1 - (old - added)
        {
            u32::MAX
        } else {
            old - added + freed
        };
        set_dw(488, new);
        set_dw(492, self.hint);
        sector(lba, true)
    }
    /// Stage the new chain, publish the directory entry, then reclaim the old
    /// chain. A full disk leaves an existing destination readable.
    pub fn save(&mut self, src: u16, size: u16) -> bool {
        self.save_chain(src, size as u32, 0)
    }
    /// COMMAND+160 holds the destination short name, COMMAND+128 the source.
    pub fn duplicate(&mut self, destination: u32) -> bool {
        let mut src = Entry::EMPTY;
        if !self.scan(0, &mut src)
            || src.attr & 16 != 0
            || (src.size != 0 && !self.valid(src.first))
        {
            return false;
        }
        let mut i = 0;
        while i < 11 {
            store(COMMAND, 128 + i, byte(COMMAND, 160 + i));
            i += 1;
        }
        self.restore(destination);
        self.save_chain(0, src.size, src.first)
    }
    fn save_chain(&mut self, src: u16, size: u32, mut source_cluster: u32) -> bool {
        let mut entry = Entry::EMPTY;
        let existing = self.scan(0, &mut entry);
        if entry.lba == u32::MAX {
            return false;
        }
        if existing && entry.attr & 0x19 != 0 {
            return false;
        }
        if !existing && !self.new_slot(&mut entry) {
            return false;
        }
        let old = if existing { entry.first } else { 0 };
        let mut first = 0;
        let mut tail = 0;
        let mut added = 0;
        let mut offset = 0u32;
        while offset < size {
            if src == 0 && !self.valid(source_cluster) {
                self.release(first);
                return false;
            }
            let c = self.allocate();
            if c == 0 {
                self.release(first);
                return false;
            }
            if tail == 0 {
                first = c;
            } else if !self.change(tail, c) {
                self.release(c);
                self.release(first);
                return false;
            }
            tail = c;
            added += 1;
            let mut i = 0;
            while i < (1u16 << self.shift) && offset < size {
                let n = if size - offset < 512 {
                    size - offset
                } else {
                    512
                };
                if src == 0 {
                    if !sector(self.first(source_cluster) + i as u32, false) {
                        self.release(first);
                        return false;
                    }
                } else {
                    let mut x = 0;
                    while x < 512 {
                        store(
                            BUFFER,
                            x,
                            if (x as u32) < n {
                                byte(src, offset as u16 + x)
                            } else {
                                0
                            },
                        );
                        x += 1;
                    }
                }
                if !sector(self.first(c) + i as u32, true) {
                    self.release(first);
                    return false;
                }
                offset += n;
                i += 1;
            }
            if src == 0 {
                source_cluster = self.next(source_cluster);
                if offset == size && source_cluster < EOC {
                    self.release(first);
                    return false;
                }
            }
        }
        if !sector(entry.lba, false) {
            self.release(first);
            return false;
        }
        let at = entry.offset;
        if !existing {
            let mut i = 0;
            while i < 32 {
                store(
                    BUFFER,
                    at + i,
                    if i < 11 { byte(COMMAND, 128 + i) } else { 0 },
                );
                i += 1;
            }
            // DOS epoch, January 1, 1980.
            set_word(at + 16, 0x21);
            set_word(at + 24, 0x21);
        }
        store(BUFFER, at + 11, if existing { entry.attr | 32 } else { 32 });
        set_word(at + 20, (first >> 16) as u16);
        set_word(at + 26, first as u16);
        set_dw(at + 28, size as u32);
        if !sector(entry.lba, true) {
            return false;
        }
        let freed = self.release(old);
        freed != u32::MAX
            && self.info(self.info, added, freed)
            && self.info(self.backup_info, added, freed)
    }
}
fn fsinfo_valid() -> bool {
    dw(0) == 0x4161_5252 && dw(484) == 0x6141_7272 && dw(508) == 0xaa55_0000
}

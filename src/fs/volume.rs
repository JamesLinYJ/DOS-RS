//! FAT geometry, allocation tables and directory sectors on a mounted volume.
use core::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Fat12,
    Fat16,
    Fat32,
}

impl Kind {
    pub fn label(self) -> [u8; 8] {
        match self {
            Self::Fat12 => *b"FAT12   ",
            Self::Fat16 => *b"FAT16   ",
            Self::Fat32 => *b"FAT32   ",
        }
    }

    fn mask(self) -> u32 {
        match self {
            Self::Fat12 => 0xfff,
            Self::Fat16 => 0xffff,
            Self::Fat32 => 0x0fff_ffff,
        }
    }
}

pub(super) fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

pub(super) fn dword(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

pub(super) struct Map {
    pub kind: Kind,
    pub bps: usize,
    pub spc: usize,
    pub fat_sectors: usize,
    pub data_sector: usize,
    pub max: u32,
    pub root: u32,
    root_at: usize,
    root_entries: usize,
    tables: Range<usize>,
    fat_start: usize,
    fsinfo: Option<usize>,
    pub backup: Option<usize>,
    reserved: usize,
}

impl Map {
    pub fn parse(image: &[u8]) -> Option<Self> {
        if image.get(510..512)? != [0x55, 0xaa] {
            return None;
        }
        let bps = usize::from(word(image, 11));
        let spc = usize::from(image[13]);
        let reserved = usize::from(word(image, 14));
        let fats = usize::from(image[16]);
        if !matches!(bps, 512 | 1024 | 2048 | 4096)
            || !spc.is_power_of_two()
            || spc > 128
            || reserved == 0
            || fats == 0
        {
            return None;
        }
        let root_entries = usize::from(word(image, 17));
        let short_total = usize::from(word(image, 19));
        let total = if short_total != 0 {
            short_total
        } else {
            dword(image, 32) as usize
        };
        if total > image.len() / bps {
            return None;
        }
        let short_fat = usize::from(word(image, 22));
        let fat_sectors = if short_fat != 0 {
            short_fat
        } else {
            dword(image, 36) as usize
        };
        let root_sector = reserved.checked_add(fats.checked_mul(fat_sectors)?)?;
        let data_sector = root_sector.checked_add((root_entries * 32).div_ceil(bps))?;
        let count = total.checked_sub(data_sector)? / spc;
        let kind = match count {
            1..4085 => Kind::Fat12,
            4085..65525 => Kind::Fat16,
            65525..=0x0fff_ffee => Kind::Fat32,
            _ => return None,
        };
        if (kind == Kind::Fat32) != (short_fat == 0) || (kind == Kind::Fat32) != (root_entries == 0)
        {
            return None;
        }
        let mut tables = 0..fats;
        let mut root = 0;
        let mut fsinfo = None;
        let mut backup = None;
        if kind == Kind::Fat32 {
            if word(image, 42) != 0 {
                return None;
            }
            root = dword(image, 44);
            let flags = word(image, 40);
            if flags & 0x80 != 0 {
                let active = usize::from(flags & 0x0f);
                if active >= fats {
                    return None;
                }
                tables = active..active + 1;
            }
            let metadata_sector = |at| {
                let sector = usize::from(word(image, at));
                (sector > 0 && sector < reserved).then_some(sector)
            };
            fsinfo = metadata_sector(48);
            backup = metadata_sector(50);
        }
        let map = Self {
            kind,
            bps,
            spc,
            fat_sectors,
            data_sector,
            max: count as u32 + 1,
            root,
            root_at: root_sector * bps,
            root_entries,
            tables,
            fat_start: reserved * bps,
            fsinfo,
            backup,
            reserved,
        };
        let (offset, width) = map.position(map.max);
        if offset + width > fat_sectors * bps || (kind == Kind::Fat32 && !map.valid(root)) {
            return None;
        }
        Some(map)
    }

    pub fn valid(&self, cluster: u32) -> bool {
        (2..=self.max).contains(&cluster) && cluster < self.kind.mask() - 15
    }

    pub fn cluster_bytes(&self) -> usize {
        self.bps * self.spc
    }

    pub fn data(&self, cluster: u32) -> Range<usize> {
        debug_assert!(self.valid(cluster));
        let start = self.data_sector * self.bps + (cluster as usize - 2) * self.cluster_bytes();
        start..start + self.cluster_bytes()
    }

    fn position(&self, cluster: u32) -> (usize, usize) {
        let n = cluster as usize;
        match self.kind {
            Kind::Fat12 => (n + n / 2, 2),
            Kind::Fat16 => (n * 2, 2),
            Kind::Fat32 => (n * 4, 4),
        }
    }

    fn fat_offset(&self, copy: usize) -> usize {
        self.fat_start + copy * self.fat_sectors * self.bps
    }

    fn value(&self, image: &[u8], cluster: u32) -> u32 {
        let at = self.fat_offset(self.tables.start) + self.position(cluster).0;
        match self.kind {
            Kind::Fat12 if cluster & 1 != 0 => u32::from(word(image, at) >> 4),
            Kind::Fat12 => u32::from(word(image, at) & 0xfff),
            Kind::Fat16 => u32::from(word(image, at)),
            Kind::Fat32 => dword(image, at) & self.kind.mask(),
        }
    }

    pub fn set(&self, image: &mut super::media::Bytes, cluster: u32, value: u32) {
        debug_assert!(self.valid(cluster));
        for copy in self.tables.clone() {
            let at = self.fat_offset(copy) + self.position(cluster).0;
            match self.kind {
                Kind::Fat32 => {
                    let bits = dword(image, at) & !self.kind.mask() | value & self.kind.mask();
                    image.edit(at..at + 4).copy_from_slice(&bits.to_le_bytes());
                }
                kind => {
                    let old = word(image, at);
                    let bits = match kind {
                        Kind::Fat12 if cluster & 1 != 0 => old & 0xf | (value as u16) << 4,
                        Kind::Fat12 => old & 0xf000 | (value as u16) & 0xfff,
                        _ => value as u16,
                    };
                    image.edit(at..at + 2).copy_from_slice(&bits.to_le_bytes());
                }
            }
        }
    }

    pub fn next(&self, image: &[u8], cluster: u32) -> Option<u32> {
        let next = self.value(image, cluster);
        self.valid(next).then_some(next)
    }

    // Validate before mutations. A free, reserved or cyclic link invalidates the entire chain.
    pub fn chain(&self, image: &[u8], first: u32) -> Option<(usize, u32)> {
        if first == 0 {
            return Some((0, 0));
        }
        let mut current = first;
        for count in 1..self.max {
            if !self.valid(current) {
                return None;
            }
            let next = self.value(image, current);
            if next >= self.kind.mask() - 7 {
                return Some((count as usize, current));
            }
            current = next;
        }
        None
    }

    pub fn release(&self, image: &mut super::media::Bytes, mut cluster: u32) {
        while cluster != 0 {
            let next = self.next(image, cluster);
            self.set(image, cluster, 0);
            cluster = next.unwrap_or(0);
        }
    }

    pub fn free<'a>(&'a self, image: &'a [u8], start: u32) -> impl Iterator<Item = u32> + 'a {
        (start..=self.max.min(self.kind.mask() - 16)).filter(move |&n| self.value(image, n) == 0)
    }

    // Callers reserve the total number of clusters before committing any data.
    pub fn allocate(&self, image: &mut super::media::Bytes, cursor: &mut u32) -> u32 {
        let n = self
            .free(image, *cursor)
            .next()
            .expect("preflight reserved enough clusters");
        *cursor = n + 1;
        self.set(image, n, self.kind.mask());
        image.edit(self.data(n)).fill(0);
        n
    }

    pub fn refresh_fsinfo(&self, image: &mut super::media::Bytes) {
        let Some(primary) = self.fsinfo else { return };
        let mut free = self.free(image, 2);
        let first = free.next();
        let count = u32::from(first.is_some()) + free.count() as u32;
        for sector in [
            Some(primary),
            self.backup.and_then(|b| b.checked_add(primary)),
        ]
        .into_iter()
        .flatten()
        {
            if sector >= self.reserved {
                continue;
            }
            let page = image.edit(sector * self.bps..(sector + 1) * self.bps);
            if dword(page, 0) == 0x41615252
                && dword(page, 484) == 0x61417272
                && dword(page, 508) == 0xaa550000
            {
                page[488..492].copy_from_slice(&count.to_le_bytes());
                page[492..496].copy_from_slice(&first.unwrap_or(u32::MAX).to_le_bytes());
            }
        }
    }

    pub fn first(&self, raw: &[u8]) -> u32 {
        let low = u32::from(word(raw, 26));
        if self.kind == Kind::Fat32 {
            low | (u32::from(word(raw, 20) & 0xfff) << 16)
        } else {
            low
        }
    }

    pub fn set_first(&self, raw: &mut [u8], cluster: u32) {
        if self.kind == Kind::Fat32 {
            raw[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        }
        raw[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
    }

    pub fn directory<'a>(&'a self, image: &'a [u8], dir: u32) -> Option<Slots<'a>> {
        let cluster = if dir == 0 { self.root } else { dir };
        let (range, count, last) = if cluster == 0 {
            (
                self.root_at..self.root_at + self.root_entries * 32,
                self.root_entries,
                0,
            )
        } else {
            let (clusters, last) = self.chain(image, cluster)?;
            (
                self.data(cluster),
                clusters * (self.cluster_bytes() / 32),
                last,
            )
        };
        Some(Slots {
            map: self,
            image,
            cluster,
            range,
            left: count,
            last,
        })
    }
}

pub(super) struct Slots<'a> {
    map: &'a Map,
    image: &'a [u8],
    cluster: u32,
    range: Range<usize>,
    left: usize,
    pub last: u32,
}

impl Iterator for Slots<'_> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        if self.left == 0 {
            return None;
        }
        if self.range.is_empty() {
            self.cluster = self.map.next(self.image, self.cluster)?;
            self.range = self.map.data(self.cluster);
        }
        self.left -= 1;
        let at = self.range.start;
        self.range.start += 32;
        Some(at)
    }
}

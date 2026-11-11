//! Mutable volume for the command shell.
//!
//! The image is copied once. Later creates, deletes, and renames change this
//! copy. Cluster numbers still use [`crate::dos::fat`]. A highest cluster of
//! 4086 or more is a 16-bit table; below that the entries are 12-bit.

use super::volume;

const CAPACITY: usize = 4096 * 512;
const FAT_ROOM: usize = 8192;

struct Disk {
    bytes: [u8; CAPACITY],
    cwd: u16,
}

static mut DISK: Disk = Disk {
    bytes: [0; CAPACITY],
    cwd: 0,
};

struct Map {
    bytes_per_sector: usize,
    fat_at: usize,
    fat_bytes: usize,
    root_at: usize,
    root_entries: usize,
    data_sector: usize,
    max_cluster: u16,
}

/// Sectors per cluster, bytes per sector, free units, and total units.
///
/// The scan matches `DISK_INFO`: cluster 2 through the highest cluster, with
/// a zero entry counted free. The total is one less than the highest cluster.
pub fn media_id() -> u8 {
    storage().bytes.get(21).copied().unwrap_or(0xF0)
}

pub fn allocation() -> Option<(u16, u16, u16, u16)> {
    let disk = storage();
    let map = layout(disk)?;
    if map.max_cluster < 2 {
        return None;
    }
    let sectors_per_cluster = u16::from(*disk.bytes.get(13)?);
    let bytes_per_sector = u16::from_le_bytes([*disk.bytes.get(11)?, *disk.bytes.get(12)?]);
    if sectors_per_cluster == 0 || bytes_per_sector == 0 {
        return None;
    }
    let fat = disk.bytes.get(map.fat_at..map.fat_at + map.fat_bytes)?;
    let mut free = 0u16;
    let mut left = map.max_cluster.wrapping_sub(1);
    let mut cluster = 2u16;
    while left != 0 {
        match crate::dos::fat::unpack(fat, map.max_cluster, cluster) {
            Some(0) => free = free.wrapping_add(1),
            Some(_) => {}
            None => return None,
        }
        cluster = cluster.wrapping_add(1);
        left = left.wrapping_sub(1);
    }
    Some((
        sectors_per_cluster,
        bytes_per_sector,
        free,
        map.max_cluster.wrapping_sub(1),
    ))
}

pub fn boot() {
    let disk = storage();
    disk.bytes.fill(0);
    let image = volume::IMAGE;
    let count = image.len().min(disk.bytes.len());
    disk.bytes[..count].copy_from_slice(&image[..count]);
    disk.cwd = 0;
}

pub fn file_len(name: &[u8; 11]) -> Option<usize> {
    let disk = storage();
    let map = layout(disk)?;
    let found = find(disk, &map, disk.cwd, name)?;
    if found.attr & 0x10 != 0 {
        return None;
    }
    Some(found.size)
}

pub fn read_head(name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    let disk = storage();
    let map = layout(disk)?;
    let found = find(disk, &map, disk.cwd, name)?;
    if found.attr & 0x10 != 0 {
        return None;
    }
    let want = found.size.min(out.len());
    copy_cluster(disk, &map, found.cluster, want, out)
}

pub fn read_file(name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    let disk = storage();
    let map = layout(disk)?;
    let found = find(disk, &map, disk.cwd, name)?;
    if found.attr & 0x10 != 0 {
        return None;
    }
    copy_cluster(disk, &map, found.cluster, found.size, out)
}

pub fn write_file(name: &[u8; 11], data: &[u8]) -> bool {
    write_named(name, data, 0x20)
}

pub fn write_named(name: &[u8; 11], data: &[u8], attr: u8) -> bool {
    if data.len() > 128 * 512 {
        return false;
    }
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    if let Some(found) = find(disk, &map, disk.cwd, name) {
        free_cluster(disk, &map, found.cluster);
        clear_entry(disk, &map, found.dir, found.index);
    }
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(slot) = free_slot(disk, &map, disk.cwd) else {
        return false;
    };
    let count = data.len().div_ceil(map.bytes_per_sector).max(1);
    let Some(cluster) = alloc_chain(disk, &map, count) else {
        return false;
    };
    if !store_chain(disk, &map, cluster, data) {
        return false;
    }
    let mut raw = [0u8; 32];
    raw[..11].copy_from_slice(name);
    raw[11] = attr & 0x27;
    raw[26..28].copy_from_slice(&cluster.to_le_bytes());
    let Ok(size) = u32::try_from(data.len()) else {
        return false;
    };
    raw[28..32].copy_from_slice(&size.to_le_bytes());
    write_entry(disk, &map, disk.cwd, slot, &raw)
}

pub fn remove_file(name: &[u8; 11]) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    if found.attr & 0x10 != 0 {
        return false;
    }
    free_cluster(disk, &map, found.cluster);
    clear_entry(disk, &map, found.dir, found.index)
}

pub fn rename_file(from: &[u8; 11], to: &[u8; 11]) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    if find(disk, &map, disk.cwd, to).is_some() {
        return false;
    }
    let Some(found) = find(disk, &map, disk.cwd, from) else {
        return false;
    };
    let mut raw = found.raw;
    raw[..11].copy_from_slice(to);
    write_entry(disk, &map, found.dir, found.index, &raw)
}

pub fn make_dir(name: &[u8; 11]) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    if find(disk, &map, disk.cwd, name).is_some() {
        return false;
    }
    let parent = disk.cwd;
    let Some(slot) = free_slot(disk, &map, parent) else {
        return false;
    };
    let Some(cluster) = alloc(disk, &map) else {
        return false;
    };
    if !format_directory(disk, &map, cluster, parent) {
        return false;
    }
    let mut raw = [0u8; 32];
    raw[..11].copy_from_slice(name);
    raw[11] = 0x10;
    raw[26..28].copy_from_slice(&cluster.to_le_bytes());
    write_entry(disk, &map, parent, slot, &raw)
}

pub fn remove_dir(name: &[u8; 11]) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    if found.attr & 0x10 == 0 || !dir_empty(disk, &map, found.cluster) {
        return false;
    }
    free_cluster(disk, &map, found.cluster);
    clear_entry(disk, &map, found.dir, found.index)
}

pub fn change_dir(path: &[u8]) -> bool {
    let disk = storage();
    if path == b"\\" || path.is_empty() {
        disk.cwd = 0;
        return true;
    }
    if path == b".." {
        if disk.cwd == 0 {
            return true;
        }
        let Some(map) = layout(disk) else {
            return false;
        };
        disk.cwd = parent_of(disk, &map, disk.cwd);
        return true;
    }
    let Some(name) = canonical(path) else {
        return false;
    };
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, disk.cwd, &name) else {
        return false;
    };
    if found.attr & 0x10 == 0 {
        return false;
    }
    disk.cwd = found.cluster;
    true
}

pub fn stamp(name: &[u8; 11]) -> Option<(u16, u16)> {
    let disk = storage();
    let map = layout(disk)?;
    let found = find(disk, &map, disk.cwd, name)?;
    let time = u16::from_le_bytes([found.raw[22], found.raw[23]]);
    let date = u16::from_le_bytes([found.raw[24], found.raw[25]]);
    Some((time, date))
}

pub fn set_stamp(name: &[u8; 11], time: u16, date: u16) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    let mut raw = found.raw;
    let time_bytes = time.to_le_bytes();
    let date_bytes = date.to_le_bytes();
    raw[22] = time_bytes[0];
    raw[23] = time_bytes[1];
    raw[24] = date_bytes[0];
    raw[25] = date_bytes[1];
    write_entry(disk, &map, found.dir, found.index, &raw)
}

pub fn attribute(name: &[u8; 11]) -> Option<u8> {
    let disk = storage();
    let map = layout(disk)?;
    Some(find(disk, &map, disk.cwd, name)?.attr)
}

pub fn set_attribute(name: &[u8; 11], attr: u8) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    let Some(found) = find(disk, &map, disk.cwd, name) else {
        return false;
    };
    if found.attr & 0x10 != 0 {
        return false;
    }
    let mut raw = found.raw;
    raw[11] = attr & 0x27;
    write_entry(disk, &map, found.dir, found.index, &raw)
}

pub fn exists(name: &[u8; 11]) -> bool {
    let disk = storage();
    let Some(map) = layout(disk) else {
        return false;
    };
    find(disk, &map, disk.cwd, name).is_some()
}

struct Found {
    dir: u16,
    index: usize,
    cluster: u16,
    size: usize,
    attr: u8,
    raw: [u8; 32],
}

fn storage() -> &'static mut Disk {
    unsafe { &mut *core::ptr::addr_of_mut!(DISK) }
}

fn layout(disk: &Disk) -> Option<Map> {
    let image = &disk.bytes;
    if image.len() < 512 || image[510] != 0x55 || image[511] != 0xAA {
        return None;
    }
    let bytes_per_sector = u16::from_le_bytes([image[11], image[12]]) as usize;
    if bytes_per_sector != 512 || !image.len().is_multiple_of(bytes_per_sector) {
        return None;
    }
    let reserved = u16::from_le_bytes([image[14], image[15]]) as usize;
    let fats = image[16] as usize;
    let root_entries = u16::from_le_bytes([image[17], image[18]]) as usize;
    let total = u16::from_le_bytes([image[19], image[20]]) as usize;
    let sectors_per_fat = u16::from_le_bytes([image[22], image[23]]) as usize;
    if fats == 0 || sectors_per_fat == 0 || total == 0 {
        return None;
    }
    let root_sectors = root_entries.saturating_mul(32).div_ceil(bytes_per_sector);
    let data_sector = reserved + fats * sectors_per_fat + root_sectors;
    if data_sector >= total {
        return None;
    }
    Some(Map {
        bytes_per_sector,
        fat_at: reserved * bytes_per_sector,
        fat_bytes: fats * sectors_per_fat * bytes_per_sector,
        root_at: (reserved + fats * sectors_per_fat) * bytes_per_sector,
        root_entries,
        data_sector,
        max_cluster: ((total - data_sector) as u16) + 1,
    })
}

fn find(disk: &Disk, map: &Map, dir: u16, name: &[u8; 11]) -> Option<Found> {
    let mut index = 0usize;
    let limit = entry_count(map, dir);
    while index < limit {
        let raw = entry(disk, map, dir, index)?;
        if raw[0] == 0 {
            return None;
        }
        let label = raw[11] & 0x08 != 0;
        if raw[0] != 0xE5 && !label && raw[..11] == *name {
            return Some(Found {
                dir,
                index,
                cluster: u16::from_le_bytes([raw[26], raw[27]]),
                size: u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]) as usize,
                attr: raw[11],
                raw,
            });
        }
        index += 1;
    }
    None
}

fn entry(disk: &Disk, map: &Map, dir: u16, index: usize) -> Option<[u8; 32]> {
    let at = entry_at(disk, map, dir, index)?;
    let mut raw = [0u8; 32];
    raw.copy_from_slice(disk.bytes.get(at..at + 32)?);
    Some(raw)
}

fn entry_at(disk: &Disk, map: &Map, dir: u16, index: usize) -> Option<usize> {
    if dir == 0 {
        return Some(map.root_at + index * 32);
    }
    let base = cluster_at(map, dir)?;
    let _ = disk;
    Some(base + index * 32)
}

fn entry_count(map: &Map, dir: u16) -> usize {
    if dir == 0 {
        map.root_entries
    } else {
        map.bytes_per_sector / 32
    }
}

fn free_slot(disk: &Disk, map: &Map, dir: u16) -> Option<usize> {
    let mut index = 0usize;
    while index < entry_count(map, dir) {
        let raw = entry(disk, map, dir, index)?;
        if raw[0] == 0 || raw[0] == 0xE5 {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn write_entry(disk: &mut Disk, map: &Map, dir: u16, index: usize, raw: &[u8; 32]) -> bool {
    let Some(at) = entry_at(disk, map, dir, index) else {
        return false;
    };
    let Some(slot) = disk.bytes.get_mut(at..at + 32) else {
        return false;
    };
    slot.copy_from_slice(raw);
    true
}

fn clear_entry(disk: &mut Disk, map: &Map, dir: u16, index: usize) -> bool {
    let mut raw = [0u8; 32];
    raw[0] = 0xE5;
    write_entry(disk, map, dir, index, &raw)
}

fn take_fat(disk: &Disk, map: &Map) -> Option<[u8; FAT_ROOM]> {
    if map.fat_bytes == 0 || map.fat_bytes > FAT_ROOM {
        return None;
    }
    let mut fat = [0u8; FAT_ROOM];
    fat[..map.fat_bytes].copy_from_slice(disk.bytes.get(map.fat_at..map.fat_at + map.fat_bytes)?);
    Some(fat)
}

fn put_fat(disk: &mut Disk, map: &Map, fat: &[u8; FAT_ROOM]) -> bool {
    let Some(slot) = disk.bytes.get_mut(map.fat_at..map.fat_at + map.fat_bytes) else {
        return false;
    };
    slot.copy_from_slice(&fat[..map.fat_bytes]);
    true
}

fn copy_cluster(disk: &Disk, map: &Map, cluster: u16, size: usize, out: &mut [u8]) -> Option<usize> {
    if size > out.len() || cluster < 2 {
        return None;
    }
    let fat = take_fat(disk, map)?;
    let mut current = cluster;
    let mut filled = 0usize;
    while filled < size {
        let base = cluster_at(map, current)?;
        let chunk = map.bytes_per_sector.min(size - filled);
        let bytes = disk.bytes.get(base..base + chunk)?;
        out[filled..filled + chunk].copy_from_slice(bytes);
        filled += chunk;
        if filled >= size {
            break;
        }
        let next = crate::dos::fat::unpack(&fat, map.max_cluster, current)?;
        if crate::dos::fat::is_eof(map.max_cluster, next) {
            return None;
        }
        current = next;
    }
    Some(filled)
}

fn cluster_at(map: &Map, cluster: u16) -> Option<usize> {
    if cluster < 2 {
        return None;
    }
    Some((map.data_sector + usize::from(cluster - 2)) * map.bytes_per_sector)
}

fn alloc_chain(disk: &mut Disk, map: &Map, count: usize) -> Option<u16> {
    if count == 0 {
        return None;
    }
    let first = alloc(disk, map)?;
    let mut previous = first;
    let mut made = 1usize;
    while made < count {
        let next = alloc(disk, map)?;
        if !link_cluster(disk, map, previous, next) {
            return None;
        }
        previous = next;
        made += 1;
    }
    Some(first)
}

fn link_cluster(disk: &mut Disk, map: &Map, cluster: u16, next: u16) -> bool {
    let Some(mut fat) = take_fat(disk, map) else {
        return false;
    };
    if !crate::dos::fat::pack(&mut fat, map.max_cluster, cluster, next) {
        return false;
    }
    put_fat(disk, map, &fat)
}

fn store_chain(disk: &mut Disk, map: &Map, cluster: u16, data: &[u8]) -> bool {
    let mut current = cluster;
    let mut filled = 0usize;
    loop {
        let Some(base) = cluster_at(map, current) else {
            return false;
        };
        let Some(slot) = disk.bytes.get_mut(base..base + map.bytes_per_sector) else {
            return false;
        };
        slot.fill(0);
        if filled < data.len() {
            let chunk = map.bytes_per_sector.min(data.len() - filled);
            slot[..chunk].copy_from_slice(&data[filled..filled + chunk]);
            filled += chunk;
        }
        if filled >= data.len() {
            return true;
        }
        let Some(fat) = take_fat(disk, map) else {
            return false;
        };
        let Some(next) = crate::dos::fat::unpack(&fat, map.max_cluster, current) else {
            return false;
        };
        if crate::dos::fat::is_eof(map.max_cluster, next) {
            return filled >= data.len();
        }
        current = next;
    }
}

fn alloc(disk: &mut Disk, map: &Map) -> Option<u16> {
    let mut fat = take_fat(disk, map)?;
    let mut cluster = 2u16;
    while cluster <= map.max_cluster {
        if crate::dos::fat::unpack(&fat, map.max_cluster, cluster) == Some(0) {
            if !crate::dos::fat::pack(&mut fat, map.max_cluster, cluster, 0xFFFF) {
                return None;
            }
            if !put_fat(disk, map, &fat) {
                return None;
            }
            if let Some(base) = cluster_at(map, cluster)
                && let Some(slot) = disk.bytes.get_mut(base..base + map.bytes_per_sector)
            {
                slot.fill(0);
            }
            return Some(cluster);
        }
        cluster += 1;
    }
    None
}

fn free_cluster(disk: &mut Disk, map: &Map, cluster: u16) {
    if cluster < 2 {
        return;
    }
    let Some(mut fat) = take_fat(disk, map) else {
        return;
    };
    let mut current = cluster;
    let mut guard = 0u16;
    while current >= 2 && guard < map.max_cluster {
        let next = crate::dos::fat::unpack(&fat, map.max_cluster, current).unwrap_or(0);
        let _ = crate::dos::fat::pack(&mut fat, map.max_cluster, current, 0);
        if next < 2 || crate::dos::fat::is_eof(map.max_cluster, next) {
            break;
        }
        current = next;
        guard += 1;
    }
    let _ = put_fat(disk, map, &fat);
}

fn format_directory(disk: &mut Disk, map: &Map, cluster: u16, parent: u16) -> bool {
    let dot = dotted(b".          ", cluster);
    let dotdot = dotted(b"..         ", parent);
    let Some(base) = cluster_at(map, cluster) else {
        return false;
    };
    let Some(slot) = disk.bytes.get_mut(base..base + 64) else {
        return false;
    };
    slot[..32].copy_from_slice(&dot);
    slot[32..64].copy_from_slice(&dotdot);
    true
}

fn dotted(name: &[u8; 11], cluster: u16) -> [u8; 32] {
    let mut raw = [0u8; 32];
    raw[..11].copy_from_slice(name);
    raw[11] = 0x10;
    raw[26..28].copy_from_slice(&cluster.to_le_bytes());
    raw
}

fn dir_empty(disk: &Disk, map: &Map, cluster: u16) -> bool {
    let mut index = 0usize;
    while index < entry_count(map, cluster) {
        let Some(raw) = entry(disk, map, cluster, index) else {
            return false;
        };
        if raw[0] == 0 {
            return true;
        }
        if raw[0] != 0xE5 && raw[..11] != *b".          " && raw[..11] != *b"..         " {
            return false;
        }
        index += 1;
    }
    true
}

fn parent_of(disk: &Disk, map: &Map, cluster: u16) -> u16 {
    entry(disk, map, cluster, 1)
        .map(|raw| u16::from_le_bytes([raw[26], raw[27]]))
        .unwrap_or(0)
}

pub fn current_path(out: &mut [u8]) -> usize {
    let disk = storage();
    if disk.cwd < 2 {
        return 0;
    }
    let Some(map) = layout(disk) else {
        return 0;
    };
    let mut names = [[b' '; 11]; 16];
    let mut depth = 0usize;
    let mut cluster = disk.cwd;
    while cluster >= 2 && depth < names.len() {
        let parent = parent_of(disk, &map, cluster);
        let Some(name) = named_in(disk, &map, parent, cluster) else {
            break;
        };
        names[depth] = name;
        depth += 1;
        cluster = parent;
    }
    let mut used = 0usize;
    while depth > 0 {
        depth -= 1;
        if used > 0 {
            if used >= out.len() {
                break;
            }
            out[used] = b'\\';
            used += 1;
        }
        used = write_component(out, used, &names[depth]);
    }
    used
}

fn named_in(disk: &Disk, map: &Map, parent: u16, cluster: u16) -> Option<[u8; 11]> {
    let mut index = 0usize;
    let limit = entry_count(map, parent);
    while index < limit {
        let raw = entry(disk, map, parent, index)?;
        if raw[0] == 0 {
            return None;
        }
        let start = u16::from_le_bytes([raw[26], raw[27]]);
        let dot = raw[..11] == *b".          " || raw[..11] == *b"..         ";
        if raw[0] != 0xE5 && raw[11] & 0x08 == 0 && !dot && start == cluster {
            let mut name = [b' '; 11];
            name.copy_from_slice(&raw[..11]);
            return Some(name);
        }
        index += 1;
    }
    None
}

fn write_component(out: &mut [u8], mut used: usize, name: &[u8; 11]) -> usize {
    let mut index = 0usize;
    while index < 8 && name[index] != b' ' && used < out.len() {
        out[used] = name[index];
        used += 1;
        index += 1;
    }
    if name[8] != b' ' && used < out.len() {
        out[used] = b'.';
        used += 1;
        index = 8;
        while index < 11 && name[index] != b' ' && used < out.len() {
            out[used] = name[index];
            used += 1;
            index += 1;
        }
    }
    used
}

pub fn working_directory() -> u16 {
    storage().cwd
}

pub fn set_working(cluster: u16) {
    storage().cwd = cluster;
}

pub fn parent_directory(cluster: u16) -> u16 {
    if cluster < 2 {
        return 0;
    }
    let disk = storage();
    let Some(map) = layout(disk) else {
        return 0;
    };
    parent_of(disk, &map, cluster)
}

pub struct DirMatch {
    pub name: [u8; 11],
    pub attr: u8,
    pub time: u16,
    pub date: u16,
    pub size: u32,
    pub index: u16,
}

pub fn find_match(dir: u16, template: &[u8; 11], search_attr: u8, start: u16) -> Option<DirMatch> {
    let disk = storage();
    let map = layout(disk)?;
    let mut index = usize::from(start);
    let limit = entry_count(&map, dir);
    while index < limit {
        let raw = entry(disk, &map, dir, index)?;
        if raw[0] == 0 {
            return None;
        }
        if raw[0] != 0xE5 {
            let mut name = [b' '; 11];
            name.copy_from_slice(&raw[..11]);
            if attribute_visible(raw[11], search_attr) && names_match(template, &name) {
                let size = u32::from_le_bytes([raw[28], raw[29], raw[30], raw[31]]);
                return Some(DirMatch {
                    name,
                    attr: raw[11],
                    time: u16::from_le_bytes([raw[22], raw[23]]),
                    date: u16::from_le_bytes([raw[24], raw[25]]),
                    size,
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                });
            }
        }
        index += 1;
    }
    None
}

pub fn directory_cluster(parent: u16, name: &[u8; 11]) -> Option<u16> {
    let disk = storage();
    let map = layout(disk)?;
    let found = find(disk, &map, parent, name)?;
    if found.attr & 0x10 == 0 {
        return None;
    }
    Some(found.cluster)
}

fn attribute_visible(file_attr: u8, search_attr: u8) -> bool {
    const SPECIAL: u8 = 0x02 | 0x04 | 0x08 | 0x10;
    file_attr & SPECIAL & !search_attr == 0
}

fn names_match(template: &[u8; 11], name: &[u8; 11]) -> bool {
    let mut index = 0usize;
    while index < 11 {
        if template[index] != b'?' && template[index] != name[index] {
            return false;
        }
        index += 1;
    }
    true
}

pub fn canonical(text: &[u8]) -> Option<[u8; 11]> {
    let mut name = [b' '; 11];
    let mut extension = false;
    let mut base = 0usize;
    let mut ext = 0usize;
    let mut seen = false;
    for byte in text {
        if *byte == 0 || *byte == b' ' {
            break;
        }
        seen = true;
        if *byte == b'.' && !extension {
            extension = true;
            continue;
        }
        let upper = if byte.is_ascii_lowercase() {
            byte.wrapping_sub(b'a').wrapping_add(b'A')
        } else {
            *byte
        };
        if !extension && base < 8 {
            name[base] = upper;
            base += 1;
        } else if extension && ext < 3 {
            name[8 + ext] = upper;
            ext += 1;
        }
    }
    if seen { Some(name) } else { None }
}

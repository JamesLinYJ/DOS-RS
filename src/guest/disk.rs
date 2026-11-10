//! Mutable FAT12 volume for the command shell.
//!
//! The image is copied once. Later creates, deletes, and renames change this
//! copy. Cluster numbers still use [`crate::dos::fat`].

use super::volume;

const CAPACITY: usize = 64 * 1024;

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

pub fn boot() {
    let disk = storage();
    disk.bytes.fill(0);
    let image = volume::IMAGE;
    let count = image.len().min(disk.bytes.len());
    disk.bytes[..count].copy_from_slice(&image[..count]);
    disk.cwd = 0;
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
    if data.len() > 512 {
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
    let Some(cluster) = alloc(disk, &map) else {
        return false;
    };
    if !store_cluster(disk, &map, cluster, data) {
        return false;
    }
    let mut raw = [0u8; 32];
    raw[..11].copy_from_slice(name);
    raw[11] = 0x20;
    raw[26..28].copy_from_slice(&cluster.to_le_bytes());
    raw[28..32].copy_from_slice(&(data.len() as u32).to_le_bytes());
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

fn copy_cluster(disk: &Disk, map: &Map, cluster: u16, size: usize, out: &mut [u8]) -> Option<usize> {
    if size > out.len() || cluster < 2 {
        return None;
    }
    let base = cluster_at(map, cluster)?;
    let count = size.min(map.bytes_per_sector);
    out[..count].copy_from_slice(disk.bytes.get(base..base + count)?);
    Some(count)
}

fn store_cluster(disk: &mut Disk, map: &Map, cluster: u16, data: &[u8]) -> bool {
    let Some(base) = cluster_at(map, cluster) else {
        return false;
    };
    if let Some(slot) = disk.bytes.get_mut(base..base + map.bytes_per_sector) {
        slot.fill(0);
        let count = data.len().min(slot.len());
        slot[..count].copy_from_slice(&data[..count]);
        true
    } else {
        false
    }
}

fn cluster_at(map: &Map, cluster: u16) -> Option<usize> {
    if cluster < 2 {
        return None;
    }
    Some((map.data_sector + usize::from(cluster - 2)) * map.bytes_per_sector)
}

fn alloc(disk: &mut Disk, map: &Map) -> Option<u16> {
    let mut fat = [0u8; 512];
    fat[..map.fat_bytes.min(512)].copy_from_slice(
        disk.bytes.get(map.fat_at..map.fat_at + map.fat_bytes.min(512))?,
    );
    let mut cluster = 2u16;
    while cluster <= map.max_cluster {
        if crate::dos::fat::unpack(&fat, map.max_cluster, cluster) == Some(0) {
            if !crate::dos::fat::pack(&mut fat, map.max_cluster, cluster, 0x0FFF) {
                return None;
            }
            let end = map.fat_at + map.fat_bytes.min(512);
            disk.bytes[map.fat_at..end].copy_from_slice(&fat[..map.fat_bytes.min(512)]);
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
    let mut fat = [0u8; 512];
    let fat_len = map.fat_bytes.min(512);
    let Some(source) = disk.bytes.get(map.fat_at..map.fat_at + fat_len) else {
        return;
    };
    fat[..fat_len].copy_from_slice(source);
    let _ = crate::dos::fat::pack(&mut fat, map.max_cluster, cluster, 0);
    let end = map.fat_at + fat_len;
    disk.bytes[map.fat_at..end].copy_from_slice(&fat[..fat_len]);
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

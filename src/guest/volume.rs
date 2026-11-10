//! FAT12 directory and cluster walk for the guest volume.
//!
//! Cluster numbers and the end-of-file test come from [`crate::dos::fat`].

pub const IMAGE: &[u8] = include_bytes!("../../disks/guest12.img");

pub fn read_named(image: &[u8], name: &[u8; 11], out: &mut [u8]) -> Option<usize> {
    let volume = describe(image)?;
    let entry = find_entry(image, &volume, name)?;
    let size = u32::from_le_bytes([entry[28], entry[29], entry[30], entry[31]]) as usize;
    if size > out.len() {
        return None;
    }
    let mut cluster = u16::from_le_bytes([entry[26], entry[27]]);
    let mut filled = 0usize;
    let fat = image.get(volume.fat_at..volume.fat_at + volume.fat_bytes)?;
    while filled < size {
        if cluster < 2 {
            return None;
        }
        let sector = volume.data_sector + usize::from(cluster - 2) * volume.sectors_per_cluster;
        let mut within = 0usize;
        while within < volume.cluster_bytes && filled < size {
            let source = sector * volume.bytes_per_sector + within;
            *out.get_mut(filled)? = *image.get(source)?;
            filled += 1;
            within += 1;
        }
        if filled >= size {
            break;
        }
        let next = crate::dos::fat::unpack(fat, volume.max_cluster, cluster)?;
        if crate::dos::fat::is_eof(volume.max_cluster, next) {
            return None;
        }
        cluster = next;
    }
    Some(size)
}

struct Layout {
    bytes_per_sector: usize,
    sectors_per_cluster: usize,
    cluster_bytes: usize,
    fat_at: usize,
    fat_bytes: usize,
    root_at: usize,
    root_entries: usize,
    data_sector: usize,
    max_cluster: u16,
}

fn describe(image: &[u8]) -> Option<Layout> {
    if image.len() < 512 || image[510] != 0x55 || image[511] != 0xAA {
        return None;
    }
    let bytes_per_sector = u16::from_le_bytes([image[11], image[12]]) as usize;
    if bytes_per_sector != 512 || !image.len().is_multiple_of(bytes_per_sector) {
        return None;
    }
    let sectors_per_cluster = image[13] as usize;
    let reserved = u16::from_le_bytes([image[14], image[15]]) as usize;
    let fats = image[16] as usize;
    let root_entries = u16::from_le_bytes([image[17], image[18]]) as usize;
    let total = u16::from_le_bytes([image[19], image[20]]) as usize;
    let sectors_per_fat = u16::from_le_bytes([image[22], image[23]]) as usize;
    if sectors_per_cluster == 0 || fats == 0 || sectors_per_fat == 0 || total == 0 {
        return None;
    }
    let root_sectors = root_entries.saturating_mul(32).div_ceil(bytes_per_sector);
    let data_sector = reserved + fats * sectors_per_fat + root_sectors;
    if data_sector >= total {
        return None;
    }
    let data_sectors = total - data_sector;
    Some(Layout {
        bytes_per_sector,
        sectors_per_cluster,
        cluster_bytes: sectors_per_cluster * bytes_per_sector,
        fat_at: reserved * bytes_per_sector,
        fat_bytes: fats * sectors_per_fat * bytes_per_sector,
        root_at: (reserved + fats * sectors_per_fat) * bytes_per_sector,
        root_entries,
        data_sector,
        max_cluster: (data_sectors / sectors_per_cluster) as u16 + 1,
    })
}

fn find_entry<'a>(image: &'a [u8], volume: &Layout, name: &[u8; 11]) -> Option<&'a [u8]> {
    let mut index = 0usize;
    while index < volume.root_entries {
        let at = volume.root_at + index * 32;
        let entry = image.get(at..at + 32)?;
        if entry[0] == 0 {
            return None;
        }
        let label = entry[11] & 0x08 != 0;
        if entry[0] != 0xE5 && !label && entry[..11] == *name {
            return Some(entry);
        }
        index += 1;
    }
    None
}

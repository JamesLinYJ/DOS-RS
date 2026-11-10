//! `IsEof`, `UNPACK`, `PACK`, and `MAPCLUSTER` from `DOS/FAT.ASM`.
//!
//! The boot floppy is FAT12: 60 data clusters, so the highest cluster is 61,
//! which is below 4086. A separate table with that limit at 4086 takes the
//! FAT16 compare. `ALLOC.ASM` is the memory arena. Cluster allocate and
//! release live in `DOS/ROM.ASM`.

pub const FAT_LIMIT: u16 = 4096 - 10;

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");

/// Highest cluster number on the boot image. Data clusters are numbered from 2.
pub fn boot_max_cluster() -> u16 {
    let bps = u16::from_le_bytes([IMAGE[11], IMAGE[12]]) as usize;
    let spc = IMAGE[13] as usize;
    let reserved = u16::from_le_bytes([IMAGE[14], IMAGE[15]]) as usize;
    let fats = IMAGE[16] as usize;
    let root = u16::from_le_bytes([IMAGE[17], IMAGE[18]]) as usize;
    let total = u16::from_le_bytes([IMAGE[19], IMAGE[20]]) as usize;
    let spf = u16::from_le_bytes([IMAGE[22], IMAGE[23]]) as usize;
    let root_secs = (root * 32 + bps - 1) / bps;
    let data = total - reserved - fats * spf - root_secs;
    (data / spc) as u16 + 1
}

pub fn boot_fat() -> &'static [u8] {
    let reserved = u16::from_le_bytes([IMAGE[14], IMAGE[15]]) as usize;
    let bps = u16::from_le_bytes([IMAGE[11], IMAGE[12]]) as usize;
    let start = reserved * bps;
    &IMAGE[start..start + bps]
}

/// `JAE` after the 12-bit or 16-bit compare. `0x0FF0` is an end mark on FAT12.
pub fn is_eof(max_cluster: u16, value: u16) -> bool {
    if max_cluster >= FAT_LIMIT {
        value >= 0xfff8
    } else {
        value == 0x0ff0 || value >= 0x0ff8
    }
}

/// Byte offset of a cluster and whether its 12 bits sit in the high half.
pub fn map_cluster(max_cluster: u16, cluster: u16) -> Option<(usize, bool)> {
    if cluster > max_cluster {
        return None;
    }
    let fat16 = max_cluster >= FAT_LIMIT;
    let mut offset = u32::from(cluster);
    if !fat16 {
        offset >>= 1;
    }
    offset += u32::from(cluster);
    let high = !fat16 && cluster & 1 != 0;
    Some((offset as usize, high))
}

/// `UNPACK`. `None` is the bad-cluster path. Zero means a free cluster.
pub fn unpack(fat: &[u8], max_cluster: u16, cluster: u16) -> Option<u16> {
    let (offset, high) = map_cluster(max_cluster, cluster)?;
    if offset + 1 >= fat.len() {
        return None;
    }
    let mut di = u16::from_le_bytes([fat[offset], fat[offset + 1]]);
    if high {
        di >>= 4;
    } else if max_cluster < FAT_LIMIT {
        di &= 0x0fff;
    }
    Some(di)
}

/// `PACK`. The other nibble of a shared FAT12 word stays in place.
pub fn pack(fat: &mut [u8], max_cluster: u16, cluster: u16, data: u16) -> bool {
    let Some((offset, high)) = map_cluster(max_cluster, cluster) else {
        return false;
    };
    if offset + 1 >= fat.len() {
        return false;
    }
    let mut si = u16::from_le_bytes([fat[offset], fat[offset + 1]]);
    let mut dx = data;
    if high {
        dx <<= 4;
        si &= 0x000f;
    } else if max_cluster >= FAT_LIMIT {
        si = 0;
    } else {
        si &= 0xf000;
        dx &= 0x0fff;
    }
    si |= dx;
    let bytes = si.to_le_bytes();
    fat[offset] = bytes[0];
    fat[offset + 1] = bytes[1];
    true
}

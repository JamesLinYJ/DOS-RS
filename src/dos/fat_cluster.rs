pub const FAT_LIMIT: u16 = 4096 - 10;

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
    if offset > usize::MAX as u32 {
        return None;
    }
    Some((offset as usize, high))
}

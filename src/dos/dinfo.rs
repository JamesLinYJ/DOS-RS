//! `DISK_INFO` from `DOS/DINFO.ASM`.
//!
//! A free-count of -1 is stale, so the FAT is scanned from cluster 2 through
//! `max_cluster`. The returned total is `max_cluster - 1`. A stored count that
//! is already below `max_cluster` is trusted and the scan is skipped.

use super::fat;

pub struct Info {
    pub free: u16,
    pub total: u16,
    pub sector_size: u16,
    pub sectors_per_cluster: u8,
    pub media: u8,
}

/// Local `DISK_INFO`. `free_cnt` of -1 forces `DoScan`.
pub fn disk_info(
    fat_bytes: &[u8],
    max_cluster: u16,
    free_cnt: i16,
    cluster_mask: u8,
    sector_size: u16,
    media: u8,
) -> Info {
    let mut buffer = free_cnt as u16;
    let count = max_cluster;
    if free_cnt == -1 || buffer >= count {
        buffer = 0;
        let mut handle = 2u16;
        let mut left = count.wrapping_sub(1);
        while left != 0 {
            if fat::unpack(fat_bytes, max_cluster, handle) == Some(0) {
                buffer = buffer.wrapping_add(1);
            }
            handle = handle.wrapping_add(1);
            left = left.wrapping_sub(1);
        }
    }
    Info {
        free: buffer,
        total: count.wrapping_sub(1),
        sector_size,
        sectors_per_cluster: cluster_mask.wrapping_add(1),
        media,
    }
}

pub fn show() {
    let max_cluster = fat::boot_max_cluster();
    let info = disk_info(fat::boot_fat(), max_cluster, -1, 0, 512, 0xf8);
    crate::bios::mscon::con_write_bytes(b"DINFO DX");
    put_hex16(info.free);
    crate::bios::mscon::con_write_bytes(b" BX");
    put_hex16(info.total);
    crate::bios::mscon::con_write_bytes(b" CX");
    put_hex16(info.sector_size);
    crate::bios::mscon::con_write_bytes(b" AL");
    put_hex8(info.sectors_per_cluster);
    crate::bios::mscon::con_write_bytes(b" AH");
    put_hex8(info.media);
    crate::bios::mscon::con_write_bytes(b"\r\n");
}

fn put_hex16(value: u16) {
    put_hex8((value >> 8) as u8);
    put_hex8(value as u8);
}

fn put_hex8(value: u8) {
    let hex = b"0123456789ABCDEF";
    crate::bios::mscon::con_write_bytes(&[
        hex[(value >> 4) as usize],
        hex[(value & 0x0f) as usize],
    ]);
}

//! Sector reads from `DOS/DISK.ASM`.
//!
//! `FATSecRd` tries each FAT copy. `DREAD` is the single-sector read
//! `GETBUFFR` uses for a data or directory sector. The bytes come from a
//! mutable copy of the boot image, so a dirty buffer can be written back.

use super::misc::Dpb;

const SECTOR: usize = 512;

pub const DISK_BYTES: usize = 64 * SECTOR;

static DISK: crate::kernel_cell::KernelCell<[u8; DISK_BYTES]> =
    crate::kernel_cell::KernelCell::new(*include_bytes!("../../disks/msload.img"));

fn sectors(sector: u32, count: u16) -> Option<core::ops::Range<usize>> {
    let start = usize::try_from(sector).ok()?.checked_mul(SECTOR)?;
    let end = start.checked_add(usize::from(count) * SECTOR)?;
    (count > 0 && end <= DISK_BYTES).then_some(start..end)
}

pub fn dread(sector: u32, count: u16, dst: &mut [u8]) -> bool {
    let Some(range) = sectors(sector, count) else {
        return false;
    };
    let Some(out) = dst.get_mut(..range.len()) else {
        return false;
    };
    out.copy_from_slice(&DISK.borrow()[range]);
    true
}

pub fn dwrite(sector: u32, count: u16, src: &[u8]) -> bool {
    let Some(range) = sectors(sector, count) else {
        return false;
    };
    let Some(bytes) = src.get(..range.len()) else {
        return false;
    };
    DISK.borrow()[range].copy_from_slice(bytes);
    true
}

/// `FATSecRd`. The first copy that reads wins. Later copies are `fat_size` apart.
pub fn fat_sec_read(dpb: &Dpb, sector: u32, dst: &mut [u8]) -> bool {
    let mut buffer = sector;
    let mut left = dpb.fat_count;
    if left == 0 {
        return dread(buffer, 1, dst);
    }
    while left > 0 {
        if dread(buffer, 1, dst) {
            return true;
        }
        buffer = buffer.wrapping_add(u32::from(dpb.fat_size));
        left -= 1;
    }
    false
}

/// `DirRead` for the root. `dir_start` 0 means the directory is the linear
/// run at `dir_sec`, not a cluster chain.
pub fn dir_block(_dir_start: u16, dir_sec: u32, block: u16) -> u32 {
    dir_sec.wrapping_add(u32::from(block))
}

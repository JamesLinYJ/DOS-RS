//! Sector reads from `DOS/DISK.ASM`.
//!
//! `FATSecRd` tries each FAT copy. `DREAD` is the single-sector read
//! `GETBUFFR` uses for a data or directory sector. The bytes come from a
//! mutable copy of the boot image, so a dirty buffer can be written back.

use super::misc::Dpb;

const IMAGE: &[u8] = include_bytes!("../../disks/msload.img");
const SECTOR: usize = 512;

pub const DISK_BYTES: usize = 64 * SECTOR;

static mut DISK: [u8; DISK_BYTES] = [0; DISK_BYTES];
static mut READY: bool = false;

pub fn ensure() {
    unsafe {
        if READY {
            return;
        }
        let n = IMAGE.len().min(DISK_BYTES);
        DISK[..n].copy_from_slice(&IMAGE[..n]);
        READY = true;
    }
}

/// `DREAD` / `DSKREAD` for `count` sectors at `sector` into `dst`.
pub fn dread(sector: u32, count: u16, dst: &mut [u8]) -> bool {
    ensure();
    let bytes = count as usize * SECTOR;
    let start = sector as usize * SECTOR;
    if bytes == 0 || bytes > dst.len() || start.saturating_add(bytes) > DISK_BYTES {
        return false;
    }
    unsafe {
        dst[..bytes].copy_from_slice(&DISK[start..start + bytes]);
    }
    true
}

/// Write sectors. `BUFWRITE` uses this for a dirty buffer.
pub fn dwrite(sector: u32, count: u16, src: &[u8]) -> bool {
    ensure();
    let bytes = count as usize * SECTOR;
    let start = sector as usize * SECTOR;
    if bytes == 0 || bytes > src.len() || start.saturating_add(bytes) > DISK_BYTES {
        return false;
    }
    unsafe {
        DISK[start..start + bytes].copy_from_slice(&src[..bytes]);
    }
    true
}

/// `FATSecRd`. The first copy that reads wins. Later copies are `fat_size` apart.
pub fn fat_sec_read(dpb: &Dpb, sector: u32, dst: &mut [u8]) -> bool {
    let mut dx = sector;
    let mut left = dpb.fat_count;
    if left == 0 {
        return dread(dx, 1, dst);
    }
    while left > 0 {
        if dread(dx, 1, dst) {
            return true;
        }
        dx = dx.wrapping_add(u32::from(dpb.fat_size));
        left -= 1;
    }
    false
}

/// `DirRead` for the root. `dir_start` 0 means the directory is the linear
/// run at `dir_sec`, not a cluster chain.
pub fn dir_block(dir_start: u16, dir_sec: u32, block: u16) -> u32 {
    if dir_start == 0 {
        dir_sec.wrapping_add(u32::from(block))
    } else {
        dir_sec.wrapping_add(u32::from(block))
    }
}

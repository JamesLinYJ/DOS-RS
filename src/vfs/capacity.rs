//! Filesystem statistics in allocation units. The user ABI contains eleven little-endian u64 values.
use super::{dos, driver};
use dosrs_native_posix::{Errno, Result};

#[derive(Clone, Copy, Default)]
pub(crate) struct Capacity {
    pub block_size: u64,
    pub unit: u64,
    pub blocks: u64,
    pub free: u64,
    pub available: u64,
    pub fsid: u64,
    pub flags: u64,
    pub name_max: u64,
    pub fs_type: u64,
}
impl Capacity {
    pub fn bytes(self) -> [u8; 88] {
        let mut raw = [0u8; 88];
        let fields = [
            self.block_size,
            self.unit,
            self.blocks,
            self.free,
            self.available,
            0,
            0,
            self.fsid,
            self.flags,
            self.name_max,
            self.fs_type,
        ];
        for (slot, value) in raw.chunks_exact_mut(8).zip(fields) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        raw
    }
}
pub(crate) fn dos_volume(dir: u32) -> Result<Capacity> {
    let volume = driver::at(dir);
    let (spc, bps, free, total) = volume.space(dir).ok_or(Errno::Io)?;
    let (serial, _, kind) = volume.volume(dir).ok_or(Errno::Io)?;
    if kind.starts_with(b"BTRFS") {
        return super::posix::capacity();
    }
    let unit = u64::from(spc) * u64::from(bps);
    let (name_max, fs_type) = if kind.starts_with(b"FAT") {
        (12, 0x4d44)
    } else if kind.starts_with(b"NTFS") {
        (255, 0x5346544e)
    } else {
        return Err(Errno::NotSupported);
    };
    Ok(Capacity {
        block_size: unit,
        unit,
        blocks: u64::from(total),
        free: u64::from(free),
        available: if volume.read_only() {
            0
        } else {
            u64::from(free)
        },
        fsid: u64::from(u32::from_le_bytes(serial)),
        flags: 2 | u64::from(volume.read_only()),
        name_max,
        fs_type,
    })
}
pub(crate) fn dos_path(cwd: dos::Directories, path: &[u8]) -> Result<Capacity> {
    if path.is_empty() {
        return Err(Errno::NoEntry);
    }
    let (base, tail) = cwd.locate(path).ok_or(Errno::NoEntry)?;
    if let Some(dir) = dos::walk(base, tail) {
        return dos_volume(dir);
    }
    let (dir, name) = dos::file_in(base, tail).ok_or(Errno::NoEntry)?;
    driver::at(dir).attr(dir, &name).ok_or(Errno::NoEntry)?;
    dos_volume(dir)
}

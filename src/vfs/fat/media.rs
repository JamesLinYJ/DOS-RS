//! Separate boot-volume and ATA-volume storage. Writes track the exact sectors to flush.
use super::IMAGE;
#[cfg(not(test))]
use super::volume::Map;
use crate::kernel_cell::{Guard, KernelCell};
use core::ops::{Deref, Range};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

use super::HARD;
const LIMIT: usize = 64 << 20;
const SECTOR: usize = 512;
#[cfg(not(test))]
static DEVICE: KernelCell<Option<dosrs_block::BlockWindow>> = KernelCell::new(None);
static BOOT: KernelCell<[u8; IMAGE.len()]> = KernelCell::new([0; IMAGE.len()]);
static HARD_DISK: KernelCell<[u8; LIMIT]> = KernelCell::new([0; LIMIT]);
static DIRTY: KernelCell<[u64; LIMIT / SECTOR / 64]> = KernelCell::new([0; LIMIT / SECTOR / 64]);
static LENGTH: AtomicU32 = AtomicU32::new(0);
static NEEDS_FLUSH: AtomicBool = AtomicBool::new(false);

pub(super) enum Bytes {
    Boot(Guard<'static, [u8; IMAGE.len()]>),
    Hard(Guard<'static, [u8; LIMIT]>),
}
impl Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Boot(data) => &data[..],
            Self::Hard(data) => &data[..LENGTH.load(Relaxed) as usize],
        }
    }
}
impl Bytes {
    pub(super) fn edit(&mut self, range: Range<usize>) -> &mut [u8] {
        match self {
            Self::Boot(data) => &mut data[range],
            Self::Hard(data) => {
                let mut dirty = DIRTY.borrow();
                for sector in range.start / SECTOR..range.end.div_ceil(SECTOR) {
                    dirty[sector / 64] |= 1 << (sector % 64);
                }
                &mut data[range]
            }
        }
    }
}

pub(super) struct Disk {
    pub bytes: Bytes,
    pub cwd: u32,
}
pub(super) fn disk(dir: u32) -> Disk {
    let bytes = if dir & HARD == 0 {
        Bytes::Boot(BOOT.borrow())
    } else {
        Bytes::Hard(HARD_DISK.borrow())
    };
    Disk {
        bytes,
        cwd: dir & !HARD,
    }
}
pub(crate) fn boot() {
    BOOT.borrow().copy_from_slice(IMAGE);
}

// Accept a raw FAT volume, never infer FAT from the size of an unformatted or NTFS disk.
#[cfg(not(test))]
pub(crate) fn mount(dev: dosrs_block::BlockWindow) -> bool {
    use dosrs_block::BlockRead;
    let mut data = HARD_DISK.borrow();
    let mut sector = [0u8; SECTOR];
    if dev.read_at(0, &mut sector).is_err()
        || sector[510..] != [0x55, 0xaa]
        || u16::from_le_bytes([sector[11], sector[12]]) != 512
    {
        return false;
    }
    let short = u16::from_le_bytes([sector[19], sector[20]]);
    let count = if short == 0 {
        u32::from_le_bytes(sector[32..36].try_into().unwrap())
    } else {
        u32::from(short)
    };
    if count == 0 || u64::from(count) > dev.size_bytes() / 512 || count as usize > LIMIT / SECTOR {
        return false;
    }
    let size = count as usize * SECTOR;
    data[..SECTOR].copy_from_slice(&sector);
    if Map::parse(&data[..size]).is_none() {
        return false;
    }
    if dev.read_at(SECTOR as u64, &mut data[SECTOR..size]).is_err() {
        return false;
    }
    *DEVICE.borrow() = Some(dev);
    LENGTH.store(size as u32, Relaxed);
    true
}

pub(crate) fn flush(dir: u32) -> bool {
    if dir & HARD == 0 {
        return true;
    }
    #[cfg(not(test))]
    {
        use dosrs_block::BlockDevice;
        let dev = DEVICE.borrow();
        let Some(dev) = dev.as_ref() else {
            return false;
        };
        let data = HARD_DISK.borrow();
        let mut dirty = DIRTY.borrow();
        for (group, bits) in dirty.iter_mut().enumerate() {
            while *bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                let sector = group * 64 + bit;
                let start = sector * SECTOR;
                if dev
                    .write_at(start as u64, &data[start..start + SECTOR])
                    .is_err()
                {
                    return false;
                }
                *bits &= !(1 << bit);
                NEEDS_FLUSH.store(true, Relaxed);
            }
        }
        if NEEDS_FLUSH.load(Relaxed) {
            if dev.flush().is_err() {
                return false;
            }
            NEEDS_FLUSH.store(false, Relaxed);
        }
    }
    true
}

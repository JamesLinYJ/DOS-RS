//! Separate boot-volume and ATA-volume storage. Writes track the exact sectors to flush.
use super::{IMAGE, volume::Map};
use crate::kernel_cell::{Guard, KernelCell};
use core::ops::{Deref, Range};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

pub(super) const HARD: u32 = 1 << 31;
const LIMIT: usize = 64 << 20;
const SECTOR: usize = 512;
static BOOT: KernelCell<[u8; IMAGE.len()]> = KernelCell::new([0; IMAGE.len()]);
static HARD_DISK: KernelCell<[u8; LIMIT]> = KernelCell::new([0; LIMIT]);
static DIRTY: KernelCell<[u64; LIMIT / SECTOR / 64]> = KernelCell::new([0; LIMIT / SECTOR / 64]);
static LENGTH: AtomicU32 = AtomicU32::new(0);
static SELECTED: AtomicU32 = AtomicU32::new(0);
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
pub(super) fn available() -> bool {
    LENGTH.load(Relaxed) != 0
}
pub(super) fn cwd() -> u32 {
    SELECTED.load(Relaxed)
}
pub(super) fn select(dir: u32) {
    SELECTED.store(dir, Relaxed);
}

pub(super) fn boot() {
    BOOT.borrow().copy_from_slice(IMAGE);
    select(0);
    #[cfg(not(test))]
    if !available() && mount() {
        crate::puts("Mounted persistent FAT volume C:\\ on IDE.\n");
    }
}

// Accept a raw FAT volume, never infer FAT from the size of an unformatted or NTFS disk.
#[cfg(not(test))]
fn mount() -> bool {
    use crate::bios::ata;
    let Some(sectors) = ata::capacity() else {
        return false;
    };
    let mut data = HARD_DISK.borrow();
    let mut sector = [0u8; SECTOR];
    if !ata::read(0, &mut sector)
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
    if count == 0 || count > sectors || count as usize > LIMIT / SECTOR {
        return false;
    }
    let size = count as usize * SECTOR;
    data[..SECTOR].copy_from_slice(&sector);
    if Map::parse(&data[..size]).is_none() {
        return false;
    }
    if !ata::read_sectors(1, &mut data[SECTOR..size]) {
        return false;
    }
    LENGTH.store(size as u32, Relaxed);
    true
}

pub(super) fn flush(dir: u32) -> bool {
    if dir & HARD == 0 {
        return true;
    }
    #[cfg(not(test))]
    {
        use crate::bios::ata;
        let data = HARD_DISK.borrow();
        let mut dirty = DIRTY.borrow();
        for (group, bits) in dirty.iter_mut().enumerate() {
            while *bits != 0 {
                let bit = bits.trailing_zeros() as usize;
                let sector = group * 64 + bit;
                let start = sector * SECTOR;
                if !ata::write(
                    sector as u32,
                    data[start..start + SECTOR].try_into().unwrap(),
                ) {
                    return false;
                }
                *bits &= !(1 << bit);
                NEEDS_FLUSH.store(true, Relaxed);
            }
        }
        if NEEDS_FLUSH.load(Relaxed) {
            if !ata::flush() {
                return false;
            }
            NEEDS_FLUSH.store(false, Relaxed);
        }
    }
    true
}
